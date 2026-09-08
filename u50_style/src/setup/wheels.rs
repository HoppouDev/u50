//! Wheel discovery, verification, download, and extraction: everything
//! between a package name and an unzipped archive dir the installer can
//! consume.

use anyhow::{Context, Result, bail};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde_json::Value;
use sha2::{Digest, Sha256};

use uv_cache_info::CacheInfo;
use uv_client::BaseClient;
use uv_distribution_filename::WheelFilename;
use uv_distribution_types::{CachedDirectUrlDist, CachedDist};
use uv_pep508::VerbatimUrl;
use uv_pypi_types::{HashDigest, HashDigests, ParsedUrl, VerbatimParsedUrl};
use uv_redacted::DisplaySafeUrl;

pub(crate) const WHEEL_RANK_REJECT: u8 = 0;
pub(crate) const WHEEL_RANK_LINUX: u8 = 1;
pub(crate) const WHEEL_RANK_WINDOWS: u8 = 1;
pub(crate) const WHEEL_RANK_MANYLINUX: u8 = 2;
pub(crate) const WHEEL_RANK_PURE: u8 = 3;

pub(crate) fn wheel_rank(filename: &str) -> u8 {
    let Some(platform_tag) = filename
        .rsplit_once('-')
        .map(|(_, tag)| tag.trim_end_matches(".whl"))
    else {
        return WHEEL_RANK_REJECT;
    };
    if platform_tag == "any" {
        return WHEEL_RANK_PURE;
    }
    if std::env::consts::OS == "windows" {
        let installable = matches!(
            (std::env::consts::ARCH, platform_tag),
            ("x86_64", "win_amd64") | ("aarch64", "win_arm64") | (_, "win32")
        );
        return if installable {
            WHEEL_RANK_WINDOWS
        } else {
            WHEEL_RANK_REJECT
        };
    }
    let arch = std::env::consts::ARCH;
    if platform_tag.starts_with("manylinux") && platform_tag.contains(arch) {
        return WHEEL_RANK_MANYLINUX;
    }
    if platform_tag.starts_with("linux_") && platform_tag.contains(arch) {
        return WHEEL_RANK_LINUX;
    }
    WHEEL_RANK_REJECT
}

/// Computes the distinct missing pip packages, in first-seen language
/// order: iterates [`Language::ALL`], skips languages whose backing tool
/// `is_resolved`, and dedups by pip package (C/C++/Java all share
/// `clang-format`, so they collapse to one entry). Pure decision logic so
/// the missing-backend computation is unit-testable without any
/// provisioning; the uv install path itself is exercised by manual smoke
/// runs (it needs network access).
async fn pypi_json(client: &BaseClient, url: &DisplaySafeUrl, context: &str) -> Result<Value> {
    let json: Value = client
        .for_host(url)
        .get(url.as_str())
        .send()
        .await
        .with_context(|| format!("{context}: request failed"))?
        .error_for_status()
        .with_context(|| format!("{context}: unexpected HTTP status"))?
        .json()
        .await
        .with_context(|| format!("{context}: invalid JSON body"))?;
    Ok(json)
}

/// Resolves `package`'s wheel version: the pin from [`PINNED_VERSIONS`]
/// when present, otherwise the latest release via the `PyPI` JSON API.
async fn resolve_version(
    client: &BaseClient,
    package: &str,
    version: Option<&str>,
) -> Result<String> {
    if let Some(version) = version {
        return Ok(version.to_owned());
    }
    let url = DisplaySafeUrl::parse(&format!("https://pypi.org/pypi/{package}/json"))
        .with_context(|| format!("pypi url for {package}"))?;
    let json = pypi_json(client, &url, &format!("pypi json for {package}")).await?;
    json.pointer("/info/version")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .with_context(|| format!("pypi json for {package}: no info.version"))
}

/// Resolves, downloads, and unpacks the best wheel for
/// `package == version` (or the latest release when `version` is `None`)
/// via the `PyPI` JSON API, and wraps it as a [`CachedDist`] that
/// `uv-installer` accepts. Wheels land in `wheels_dir` (the `.whl` file
/// plus an unzipped `<name>-<ver>` archive dir).
///
/// Previously fetched wheels are reused: when the unzipped archive dir
/// for the exact wheel filename already exists (the archive name embeds
/// the package version, so a version bump invalidates it) and contains
/// its `<name>-<version>.dist-info` directory, the download and unzip
/// steps are skipped.
pub(crate) async fn fetch_wheel(
    client: &BaseClient,
    package: &str,
    version: Option<&str>,
    wheels_dir: &PathBuf,
) -> Result<CachedDist> {
    let version = resolve_version(client, package, version)
        .await
        .with_context(|| format!("resolve {package} version"))?;
    let json_url =
        DisplaySafeUrl::parse(&format!("https://pypi.org/pypi/{package}/{version}/json"))
            .with_context(|| format!("pypi url for {package}=={version}"))?;
    let json = pypi_json(
        client,
        &json_url,
        &format!("pypi json for {package}=={version}"),
    )
    .await?;

    let urls = json
        .get("urls")
        .and_then(Value::as_array)
        .context("pypi json: no urls")?;
    let mut pick: Option<(&Value, u8)> = None;
    for url in urls {
        if url.get("packagetype").and_then(Value::as_str) != Some("bdist_wheel") {
            continue;
        }
        let Some(filename) = url.get("filename").and_then(Value::as_str) else {
            continue;
        };
        let rank = wheel_rank(filename);
        if !wheel_python_compatible(filename.trim_end_matches(".whl")) {
            continue;
        }
        if rank > pick.map_or(WHEEL_RANK_REJECT, |(_, best)| best) {
            pick = Some((url, rank));
        }
    }
    let Some((entry, _)) = pick else {
        bail!("no compatible wheel for {package}=={version}");
    };
    let pick = WheelPick {
        filename: entry
            .get("filename")
            .and_then(Value::as_str)
            .context("no filename")?
            .to_string(),
        url: entry
            .get("url")
            .and_then(Value::as_str)
            .context("no url")?
            .to_string(),
        sha256: entry
            .pointer("/digests/sha256")
            .and_then(Value::as_str)
            .context("no sha256")?
            .to_string(),
    };

    validate_wheel_filename(&pick.filename)?;
    let display_url = DisplaySafeUrl::parse(&pick.url).context("wheel url")?;
    let stem = pick.filename.trim_end_matches(".whl");
    let archive = wheels_dir.join(stem);

    // Reuse a previously fetched wheel: the archive dir name embeds the
    // package version, so a version bump invalidates it naturally. The
    // extraction is atomic (unzip to a `.tmp` sibling, then rename), and
    // a `<archive>.sha256` sidecar records the digest the archive was
    // extracted from — reuse requires the dist-info tree AND a sidecar
    // matching the digest `PyPI` currently publishes, so a torn or
    // outdated archive is never mistaken for the complete extraction.
    let sidecar = wheels_dir.join(format!("{stem}.sha256"));
    if dist_info_dir(&pick.filename).is_some_and(|dir| archive.join(dir).is_dir())
        && std::fs::read_to_string(&sidecar)
            .is_ok_and(|digest| digest.trim().eq_ignore_ascii_case(&pick.sha256))
    {
        return wheel_dist(&pick.filename, display_url, &pick.sha256, archive);
    }

    // Download the wheel (enforcing its published sha256), then unzip it
    // into a cache archive dir: the installer installs from an
    // *unzipped* wheel tree (it reads `<prefix>.dist-info/WHEEL` from
    // `dist.path()`), mirroring uv's own `archive-v0` layout.
    download_and_extract_wheel(client, package, &version, &pick, wheels_dir, &archive).await?;

    wheel_dist(&pick.filename, display_url, &pick.sha256, archive)
}

/// Downloads the wheel at `wheel_url`, enforces the sha256 digest `PyPI`
/// published for it, and extracts it atomically into `archive`: unzip
/// into a `.tmp` sibling dir, then rename onto the final archive dir
/// (removing any stale final dir first — rename onto a non-empty dir
/// would fail). A crash mid-unzip leaves only the `.tmp` sibling
/// behind, never a half-extracted final archive, and the downloaded
/// bytes never touch the cache unless the digest matches. The unzip is
/// blocking CPU/IO work, so it runs on the blocking thread pool.
#[allow(clippy::too_many_lines)] // one linear download→verify→extract pipeline
async fn download_and_extract_wheel(
    client: &BaseClient,
    package: &str,
    version: &str,
    pick: &WheelPick,
    wheels_dir: &PathBuf,
    archive: &Path,
) -> Result<()> {
    let filename = pick.filename.as_str();
    let display_url = DisplaySafeUrl::parse(&pick.url).context("wheel url")?;
    tokio::fs::create_dir_all(wheels_dir).await?;
    let wheel_path = wheels_dir.join(filename);
    let bytes = client
        .for_host(&display_url)
        .get(&pick.url)
        .send()
        .await
        .with_context(|| format!("wheel download request for {package}=={version}"))?
        .error_for_status()
        .with_context(|| format!("wheel download for {package}=={version}"))?
        .bytes()
        .await
        .with_context(|| format!("wheel download body for {package}=={version}"))?;
    // Enforce the digest `PyPI` published for this wheel BEFORE the bytes
    // touch the cache: a corrupted or tampered download must never be
    // written, let alone unzipped and installed.
    let actual = hex_encode(&Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(&pick.sha256) {
        bail!(
            "sha256 mismatch for {package}=={version} ({filename}): \
             expected {}, got {actual}",
            pick.sha256
        );
    }
    tokio::fs::write(&wheel_path, &bytes).await?;

    // Extract atomically: unzip into a `.tmp` sibling dir, then rename
    // onto the final archive dir (removing any stale final dir first —
    // rename onto a non-empty dir would fail). A crash mid-unzip then
    // leaves only the `.tmp` sibling behind, never a half-extracted
    // final archive. The temp name embeds the pid so two processes no
    // longer race on one fixed sibling (the advisory provisioning lock
    // is the primary guard; this keeps even unlocked callers safe). The
    // unzip is blocking CPU/IO work, so it runs on the blocking thread
    // pool.
    let stem = filename.trim_end_matches(".whl");
    let tmp_archive = wheels_dir.join(format!(".{stem}.{}.tmp", std::process::id()));
    if tmp_archive.exists() {
        tokio::fs::remove_dir_all(&tmp_archive)
            .await
            .with_context(|| format!("remove stale temp archive {}", tmp_archive.display()))?;
    }
    let result = tokio::task::spawn_blocking({
        let tmp_archive = tmp_archive.clone();
        let wheel_path = wheel_path.clone();
        move || -> anyhow::Result<()> {
            let wheel = fs_err::File::open(&wheel_path).context("open wheel")?;
            uv_extract::unzip(wheel, &tmp_archive).context("unzip wheel")?;
            Ok(())
        }
    })
    .await
    .context("wheel unzip task")?;
    if let Err(e) = result {
        // Never leave a partial `.tmp` extraction behind.
        let _ = tokio::fs::remove_dir_all(&tmp_archive).await;
        return Err(e);
    }
    if archive.exists() {
        tokio::fs::remove_dir_all(archive)
            .await
            .with_context(|| format!("remove stale wheel archive {}", archive.display()))?;
    }
    tokio::fs::rename(&tmp_archive, archive)
        .await
        .with_context(|| format!("finalize wheel archive {}", archive.display()))?;
    // Record the digest the archive was extracted from, so the reuse
    // fast path can re-verify it against what `PyPI` currently publishes.
    tokio::fs::write(wheels_dir.join(format!("{stem}.sha256")), &pick.sha256)
        .await
        .context("write wheel archive sha256 sidecar")?;
    Ok(())
}

/// Validates a wheel filename from the (HTTPS-fetched) index response
/// before it touches any path: the filename feeds both the wheel write
/// and `remove_dir_all` calls on the archive/tmp siblings, so a hostile
/// entry with separators or `..` must not escape `wheels_dir`.
///
/// # Errors
/// Returns an error when the filename is empty, carries path separators,
/// or contains a `..` component.
fn validate_wheel_filename(filename: &str) -> Result<()> {
    if filename.is_empty()
        || Path::new(filename)
            .file_name()
            .is_none_or(|name| name != std::ffi::OsStr::new(filename))
        || filename.contains("..")
    {
        bail!("hostile wheel filename from the index response: {filename:?}");
    }
    Ok(())
}

/// Whether the wheel's Python/ABI tags are usable by the pinned `CPython`
/// (3.14): `abi3` wheels are forward-compatible across `CPython` minors,
/// pure wheels use the `py` tags, and every other wheel must target
/// `cp314` exactly — a `cp312`/`cp313` platform wheel would fit the
/// platform tag yet fail to import at runtime.
fn wheel_python_compatible(stem: &str) -> bool {
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 3 {
        return false;
    }
    let python_tag = parts[parts.len() - 3];
    let abi_tag = parts[parts.len() - 2];
    if abi_tag == "abi3" {
        return true;
    }
    let python_ok = python_tag == "py2.py3" || python_tag == "py3" || python_tag == "cp314";
    python_ok && (abi_tag == "none" || abi_tag == "cp314")
}

/// Lowercase hex encoding of a raw digest (e.g. for comparing against
/// `PyPI`'s `digests.sha256`).
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

/// The `<name>-<version>.dist-info` directory a wheel archive contains:
/// the first two dash-separated components of the wheel filename stem
/// (wheel filenames carry no dashes in either part). Best-effort: even a
/// malformed stem yields a name (e.g. `just-a` from `just-a-name`), which
/// simply never matches a real dist-info dir, so reuse fails harmlessly.
pub(crate) fn dist_info_dir(wheel_stem: &str) -> Option<String> {
    let (name, rest) = wheel_stem.split_once('-')?;
    let (version, _) = rest.split_once('-')?;
    Some(format!("{name}-{version}.dist-info"))
}

/// The picked wheel for a package: its filename, download URL, and the
/// sha256 digest `PyPI` published for it.
struct WheelPick {
    filename: String,
    url: String,
    sha256: String,
}

/// Wraps a fetched (or reused) wheel as a [`CachedDist`] that
/// `uv-installer` accepts, pointing at the unzipped `archive` dir.
fn wheel_dist(
    filename: &str,
    display_url: DisplaySafeUrl,
    sha256: &str,
    archive: PathBuf,
) -> Result<CachedDist> {
    Ok(CachedDist::Url(CachedDirectUrlDist {
        filename: WheelFilename::from_str(filename).context("wheel filename")?,
        url: VerbatimParsedUrl {
            parsed_url: ParsedUrl::try_from(display_url.clone()).context("parsed url")?,
            verbatim: VerbatimUrl::from_url(display_url),
        },
        path: archive.into_boxed_path(),
        hashes: HashDigests::from(vec![
            HashDigest::from_str(&format!("sha256:{sha256}")).context("hash digest")?,
        ]),
        cache_info: CacheInfo::default(),
        build_info: None,
    }))
}
