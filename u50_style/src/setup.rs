//! `--setup` support: provisions a uv-managed `CPython` and venv inside
//! u50's cache (`~/.cache/u50/style50`) and installs the missing formatter
//! backends into it in-process (uv library calls — no pip subprocesses),
//! so future runs can resolve the tools from the cache without root or a
//! system-wide install.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde_json::Value;
use tokio::runtime::Runtime;
use uv_cache::Cache;
use uv_cache_info::CacheInfo;
use uv_client::{BaseClient, BaseClientBuilder};
use uv_distribution_filename::WheelFilename;
use uv_distribution_types::{CachedDirectUrlDist, CachedDist};
use uv_installer::Installer;
use uv_pep508::VerbatimUrl;
use uv_preview::Preview;
use uv_pypi_types::{HashDigest, HashDigests, ParsedUrl, VerbatimParsedUrl};
use uv_python::downloads::{DownloadResult, ManagedPythonDownloadList, PythonDownloadRequest};
use uv_python::managed::{ManagedPythonInstallation, ManagedPythonInstallations};
use uv_python::{Interpreter, PythonEnvironment, VersionRequest};
use uv_redacted::DisplaySafeUrl;
use uv_virtualenv::{OnExisting, Prompt, Seed, create_venv};

use crate::formatter::{ToolOrigin, cache_dir, locate_tool};
use crate::language::Language;

/// The managed `CPython` version provisioned by `--setup` for the cache
/// venv.
const PINNED_PYTHON: &str = "3.14";

/// Pip package versions pinned for `--setup` downloads.
///
/// These MUST match `tests/tool-versions.txt` — the CI/doc source of truth
/// used to generate the golden fixtures (`tests/fixtures/`) and verified
/// byte-identical. Bumping a version here requires updating that file and
/// regenerating the golden fixtures together. Pinning keeps `--setup`
/// installs byte-compatible with the fixtures.
const PINNED_VERSIONS: &[(&str, &str)] = &[
    ("clang-format", "22.1.8"),
    ("autopep8", "2.3.2"),
    ("jsbeautifier", "2.0.3"),
    ("cssbeautifier", "2.0.3"),
    // 3.0.6 and earlier ship sdists only; 3.0.9+ ship wheels. 3.0.11 is
    // pinned (first stable wheel release line) and verified byte-identical
    // to 3.0.6 on the golden fixture.
    ("djhtml", "3.0.11"),
    ("sqlparse", "0.5.3"),
];

/// Hardcoded transitive runtime dependencies of the backend packages.
///
/// The uv-based installer installs exactly the wheels it is handed (no
/// dependency resolution), so each backend's runtime dependencies must be
/// fetched explicitly. Unpinned names resolve to the latest release via
/// the `PyPI` JSON API.
const TRANSITIVE_DEPS: &[(&str, &[&str])] = &[
    ("autopep8", &["pycodestyle"]),
    ("jsbeautifier", &["editorconfig", "six"]),
    ("cssbeautifier", &["editorconfig", "six"]),
];

/// Wheel platform-tag preference ranks: pure-Python beats manylinux
/// (matching the target architecture), which beats a bare `linux_*` tag;
/// anything else (musllinux, windows, macos, ...) is rejected.
const WHEEL_RANK_REJECT: u8 = 0;
const WHEEL_RANK_LINUX: u8 = 1;
const WHEEL_RANK_MANYLINUX: u8 = 2;
const WHEEL_RANK_PURE: u8 = 3;

/// Whether a wheel spec is a missing backend itself or one of its
/// transitive dependencies (dependencies get no summary line of their
/// own; their failures are reported through the parent backend).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A missing backend package.
    Primary,
    /// A transitive dependency of a [`Role::Primary`] package.
    Dependency,
}

/// One backend package's pinned version, if pinned in
/// [`PINNED_VERSIONS`].
fn pinned_version(package: &str) -> Option<&'static str> {
    PINNED_VERSIONS
        .iter()
        .find(|(pkg, _)| *pkg == package)
        .map(|(_, version)| *version)
}

/// The pinned wheel spec for `package`: `<pkg>==<version>` when pinned
/// in [`PINNED_VERSIONS`], the bare package name otherwise. Test-only:
/// production resolves via [`pinned_version`] and the `PyPI` JSON API.
#[cfg(test)]
fn pip_spec(package: &str) -> String {
    pinned_version(package).map_or_else(
        || package.to_owned(),
        |version| format!("{package}=={version}"),
    )
}

/// The hardcoded transitive dependencies of `package` (empty when none
/// are declared in [`TRANSITIVE_DEPS`]).
fn transitive_deps(package: &str) -> &'static [&'static str] {
    TRANSITIVE_DEPS
        .iter()
        .find(|(pkg, _)| *pkg == package)
        .map_or(&[], |(_, deps)| *deps)
}

/// Builds the full wheel spec list for `missing`: every backend package
/// with its pinned version (or `None` to resolve latest via `PyPI`),
/// followed by its transitive dependencies, deduped by package name
/// (first occurrence wins, so a primary never gets downgraded to a
/// dependency by a later parent).
fn wheel_specs(missing: &[(String, String)]) -> Vec<(String, Option<String>, Role)> {
    let mut specs: Vec<(String, Option<String>, Role)> = Vec::new();
    for (package, _) in missing {
        if !specs.iter().any(|(name, _, _)| name == package) {
            specs.push((
                package.clone(),
                pinned_version(package).map(str::to_owned),
                Role::Primary,
            ));
        }
        for &dep in transitive_deps(package) {
            if !specs.iter().any(|(name, _, _)| name == dep) {
                specs.push((dep.to_owned(), None, Role::Dependency));
            }
        }
    }
    specs
}

/// Ranks a wheel filename by platform-tag preference ([`WHEEL_RANK_PURE`]
/// is best, [`WHEEL_RANK_REJECT`] means not installable on this platform).
fn wheel_rank(filename: &str) -> u8 {
    let Some(platform_tag) = filename
        .rsplit_once('-')
        .map(|(_, tag)| tag.trim_end_matches(".whl"))
    else {
        return WHEEL_RANK_REJECT;
    };
    if platform_tag == "any" {
        return WHEEL_RANK_PURE;
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
fn missing_backends(is_resolved: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut missing: Vec<(String, String)> = Vec::new();
    for &language in &Language::ALL {
        let Some(tool) = language.required_tool() else {
            continue;
        };
        if is_resolved(tool) {
            continue;
        }
        let pip_package = language.pip_package();
        if !missing.iter().any(|(pkg, _)| pkg == pip_package) {
            missing.push((pip_package.to_owned(), tool.to_owned()));
        }
    }
    missing
}

/// The per-backend summary outcome reported by [`setup_missing`].
struct BackendOutcome {
    package: String,
    tool: String,
    /// `None` when the backend installed successfully; the failure
    /// reason otherwise.
    failure: Option<String>,
}

/// Installs missing formatter backends into the cache. Missing tools are
/// detected per language; a uv-managed `CPython` ([`PINNED_PYTHON`]) and a
/// venv at `<cache>/venv` are provisioned if absent, then each missing
/// backend package (plus its hardcoded transitive dependencies, see
/// [`TRANSITIVE_DEPS`]) is resolved, downloaded and unpacked in parallel
/// (one spawned task and one spinner per package) and installed into the
/// venv with `uv-installer`.
///
/// A package counts as installed only when its backing tool is resolvable
/// from the cache bin dir afterwards. Per-package summary lines are
/// printed (`installed: <pkg> (<tool>)` / `failed: <pkg>: <reason>`).
/// Dependency failures are reported through the parent package so it does
/// not double-report.
///
/// The missing-backend decision path is unit-tested in
/// [`missing_backends`]; the uv provisioning path is covered by manual
/// smoke runs (network), not unit tests.
///
/// # Errors
/// Returns an error (CLI exit code 3) when uv provisioning fails or any
/// package failed to install.
pub fn setup_missing() -> Result<()> {
    // Distinct missing pip packages, in first-seen language order.
    let missing = missing_backends(|tool| locate_tool(tool).is_some());

    if missing.is_empty() {
        println!("all formatter backends are already available");
        return Ok(());
    }
    println!(
        "installing {} package(s) into {}",
        missing.len(),
        cache_dir().display()
    );
    install_backends(&missing)
}

/// The shared install core, used by `u50 style --setup` and by the
/// formatter's lazy auto-provisioning: initializes uv's preview state,
/// drives the async provisioning pipeline ([`provision_backends`]) on a
/// local runtime, prints the per-package summary lines (`installed:` /
/// `failed:`), and bails when anything failed.
///
/// # Errors
/// Returns an error when uv provisioning fails or any package failed to
/// install.
fn install_backends(missing: &[(String, String)]) -> Result<()> {
    // Several uv crates read the process-global preview state; initialize
    // it before touching any uv API.
    uv_preview::set(Preview::default()).context("preview init")?;

    // Stays synchronous (the CLI and the formatter hook call this
    // synchronously); the uv provisioning path is async, so drive it on a
    // local runtime.
    let runtime = Runtime::new().context("tokio runtime")?;
    let outcomes = runtime.block_on(provision_backends(&cache_dir(), missing))?;

    let mut any_failure = false;
    for outcome in &outcomes {
        match &outcome.failure {
            None => println!("installed: {} ({})", outcome.package, outcome.tool),
            Some(reason) => {
                any_failure = true;
                println!("failed: {}: {reason}", outcome.package);
            }
        }
    }

    if any_failure {
        bail!("one or more formatter backends failed to install")
    }
    Ok(())
}

/// Lazily auto-provisions a single formatter backend into the cache on
/// first use: a no-op when `tool` already resolves from the cache,
/// otherwise maps the tool to its pip package via [`Language::ALL`] and
/// installs it (plus its transitive dependencies) through the same uv
/// library path as `u50 style --setup`. Called from the formatter hook —
/// never from the provisioning path itself — so it cannot recurse.
///
/// # Errors
/// Returns an error when no pip package is known for `tool` or when the
/// install fails; the caller only warns and lets the spawn error happen
/// naturally.
pub(crate) fn ensure_backend(tool: &str) -> Result<()> {
    if locate_tool(tool).is_some() {
        return Ok(());
    }
    let package = Language::ALL
        .iter()
        .find(|&&language| language.required_tool() == Some(tool))
        .map(|language| language.pip_package())
        .with_context(|| format!("no known pip package provides tool `{tool}`"))?;
    println!("installing 1 package(s) into {}", cache_dir().display());
    install_backends(&[(package.to_owned(), tool.to_owned())])
}

/// The async provisioning pipeline: uv cache, venv, parallel wheel
/// fetches, install, and per-backend verification. Returns one
/// [`BackendOutcome`] per entry in `missing`.
#[allow(clippy::too_many_lines)]
async fn provision_backends(
    cache_root: &Path,
    missing: &[(String, String)],
) -> Result<Vec<BackendOutcome>> {
    // One client builder for the whole run. `BaseClientBuilder::build()`
    // returns a middleware-wrapped client that already applies uv's
    // retry policy (3 retries, exponential backoff), a 10s connect
    // timeout, a 30s per-request read timeout, and a uv user agent.
    let client_builder = BaseClientBuilder::default();
    // The retry-enabled client backs the PyPI JSON and wheel GETs.
    let client = client_builder.build().context("http client build")?;
    // Python distribution downloads retry internally (`fetch_with_retry`),
    // so their client disables middleware retries to avoid double-retrying
    // — mirroring uv's own `installation.rs`.
    let download_client = client_builder
        .clone()
        .retries(0)
        .build()
        .context("download client build")?;
    // Persistent uv cache (NOT a temp dir): wheel archives persist across
    // runs, and the installer rejects symlink link-mode against temp
    // caches.
    let uv_cache = Cache::from_path(cache_root.join("uv"))
        .init()
        .await
        .context("uv cache init")?;
    let venv = ensure_venv(cache_root, &uv_cache, &client_builder, &download_client).await?;

    let specs = wheel_specs(missing);

    // Parallel fetches: one spawned task (and one spinner) per package.
    let multi = MultiProgress::new();
    let style = ProgressStyle::with_template("{spinner:.green} {msg}")
        .expect("static spinner template")
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ");
    let wheels_dir = cache_root.join("wheels");
    let handles: Vec<_> = specs
        .clone()
        .into_iter()
        .map(|(name, version, role)| {
            let pb = multi.add(ProgressBar::new_spinner());
            pb.set_style(style.clone());
            pb.set_message(format!("fetching {name}"));
            let wheels_dir = wheels_dir.clone();
            let client = client.clone();
            let pb = pb.clone();
            tokio::spawn(async move {
                let result = fetch_wheel(&client, &name, version.as_deref(), &wheels_dir).await;
                match &result {
                    Ok(_) => pb.finish_with_message(format!("{name}: done")),
                    Err(_) => pb.finish_with_message(format!("{name}: FAILED")),
                }
                (name, role, result.map_err(|e| format!("{e:#}")))
            })
        })
        .collect();
    let mut fetches: Vec<(String, Role, Result<CachedDist, String>)> = Vec::new();
    for (handle, (name, _, role)) in handles.into_iter().zip(&specs) {
        fetches.push(match handle.await {
            Ok(fetch) => fetch,
            Err(e) => (name.clone(), *role, Err(format!("fetch task failed: {e}"))),
        });
    }

    // Install every successfully fetched wheel into the venv in one
    // blocking uv call; an install failure fails all backends.
    let dists: Vec<CachedDist> = fetches
        .iter()
        .filter_map(|(_, _, result)| result.as_ref().ok().cloned())
        .collect();
    let install_error = if dists.is_empty() {
        None
    } else {
        Installer::new(&venv, Preview::default())
            .with_cache(&uv_cache)
            .with_installer_metadata(false)
            .install_blocking(dists)
            .err()
            .map(|e| format!("install: {e:#}"))
    };

    // Per-backend verification: a backend counts as installed when its
    // wheel and all its transitive deps were fetched and installed and
    // its tool resolves from the cache bin dir afterwards.
    let mut outcomes = Vec::new();
    for (package, tool) in missing {
        let mut failure = fetches
            .iter()
            .find(|(name, role, _)| name == package && *role == Role::Primary)
            .and_then(|(_, _, result)| result.as_ref().err().cloned());
        if failure.is_none() {
            for dep in transitive_deps(package) {
                let dep_failure = fetches
                    .iter()
                    .find(|(name, _, _)| name == dep)
                    .and_then(|(_, _, result)| result.as_ref().err().cloned());
                if let Some(reason) = dep_failure {
                    failure = Some(format!("{dep}: {reason}"));
                    break;
                }
            }
        }
        if failure.is_none() {
            failure.clone_from(&install_error);
        }
        if failure.is_none()
            && !locate_tool(tool).is_some_and(|(_, origin)| origin == ToolOrigin::Cache)
        {
            failure = Some(format!(
                "tool `{tool}` still not found in the cache after install"
            ));
        }
        outcomes.push(BackendOutcome {
            package: package.clone(),
            tool: tool.clone(),
            failure,
        });
    }
    Ok(outcomes)
}

/// Ensures the uv-managed venv at `<cache_root>/venv` exists: when its
/// interpreters are missing, a broken venv dir (if any) is removed and a
/// fresh managed `CPython` ([`PINNED_PYTHON`]) + venv are provisioned.
/// Reopening via [`PythonEnvironment::from_root`] keeps the happy path
/// cheap and reuses one interpreter query.
async fn ensure_venv(
    cache_root: &Path,
    uv_cache: &Cache,
    client_builder: &BaseClientBuilder<'_>,
    client: &BaseClient,
) -> Result<PythonEnvironment> {
    let venv_path = cache_root.join("venv");
    let python = venv_path.join("bin").join("python");
    let python3 = venv_path.join("bin").join("python3");
    if !python.is_file() && !python3.is_file() {
        if venv_path.exists() {
            // A venv dir without interpreters is broken; recreate it.
            fs_err::remove_dir_all(&venv_path).context("remove broken venv")?;
        }
        let interpreter = provision_python(client_builder, client, uv_cache).await?;
        create_venv(
            &venv_path,
            interpreter,
            Prompt::Static("u50-style".into()),
            false,             // system_site_packages
            OnExisting::Allow, // idempotent re-runs
            false,             // relocatable
            Seed::Disabled,
            false, // upgradeable
        )
        .context("create venv")?;
    }
    PythonEnvironment::from_root(&venv_path, uv_cache).context("open venv")
}

/// Downloads and installs a managed `CPython` ([`PINNED_PYTHON`]) into
/// uv's install root and queries its interpreter.
async fn provision_python(
    client_builder: &BaseClientBuilder<'_>,
    client: &BaseClient,
    uv_cache: &Cache,
) -> Result<Interpreter> {
    let retry_policy = client_builder.retry_policy();
    let download_list = ManagedPythonDownloadList::new(client_builder, uv_cache, None)
        .await
        .context("download list")?;
    // `client` was built with `retries(0)`: uv's download path retries
    // internally (`fetch_with_retry`), so middleware retries stay disabled.
    let request = PythonDownloadRequest::default()
        .with_version(VersionRequest::from_str(PINNED_PYTHON).context("version request")?)
        .fill()
        .context("fill request")?;
    let download = download_list
        .find(&request)
        .context("find download")?
        .clone();
    tracing::debug!(download = %download.key(), "provisioning managed python");

    let installations = ManagedPythonInstallations::from_settings(None)
        .context("installations dir")?
        .init()
        .context("init installations")?;
    let installation_dir = installations.root().to_path_buf();
    let scratch_dir = installations.scratch();
    // Hold the lock for the duration of fetch+unpack, like uv does.
    let _lock = installations.lock().await.context("installations lock")?;

    let fetched = download
        .fetch_with_retry(
            client,
            &retry_policy,
            &installation_dir,
            &scratch_dir,
            false, // reinstall
            None,  // python_install_mirror
            None,  // pypy_install_mirror
            None,  // reporter
        )
        .await
        .context("fetch managed python")?;
    let path = match fetched {
        DownloadResult::AlreadyAvailable(path) | DownloadResult::Fetched(path) => path,
    };
    let installation = ManagedPythonInstallation::new(path, &download);
    let executable = installation.executable(false);
    let interpreter = Interpreter::query(&executable, uv_cache).context("interpreter query")?;
    tracing::debug!(
        version = %interpreter.python_version(),
        executable = %executable.display(),
        "managed python ready"
    );
    Ok(interpreter)
}

/// GETs a `PyPI` JSON API endpoint through uv's middleware client: the
/// retry policy (3 attempts, exponential backoff), the 10s connect and
/// 30s per-request read timeouts, and the user agent all come from
/// [`BaseClientBuilder::build`].
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
#[allow(clippy::too_many_lines)]
async fn fetch_wheel(
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
        if rank > pick.map_or(WHEEL_RANK_REJECT, |(_, best)| best) {
            pick = Some((url, rank));
        }
    }
    let Some((entry, _)) = pick else {
        bail!("no compatible wheel for {package}=={version}");
    };
    let filename = entry
        .get("filename")
        .and_then(Value::as_str)
        .context("no filename")?
        .to_string();
    let wheel_url = entry
        .get("url")
        .and_then(Value::as_str)
        .context("no url")?
        .to_string();
    let sha256 = entry
        .pointer("/digests/sha256")
        .and_then(Value::as_str)
        .context("no sha256")?
        .to_string();

    let display_url = DisplaySafeUrl::parse(&wheel_url).context("wheel url")?;
    let archive = wheels_dir.join(filename.trim_end_matches(".whl"));

    // Reuse a previously fetched wheel: the archive dir name embeds the
    // package version, so a version bump invalidates it naturally.
    if dist_info_dir(&filename).is_some_and(|dir| archive.join(dir).is_dir()) {
        return wheel_dist(&filename, display_url, &sha256, archive);
    }

    // Download the wheel, then unzip it into a cache archive dir: the
    // installer installs from an *unzipped* wheel tree (it reads
    // `<prefix>.dist-info/WHEEL` from `dist.path()`), mirroring uv's own
    // `archive-v0` layout.
    tokio::fs::create_dir_all(wheels_dir).await?;
    let wheel_path = wheels_dir.join(&filename);
    let bytes = client
        .for_host(&display_url)
        .get(&wheel_url)
        .send()
        .await
        .with_context(|| format!("wheel download request for {package}=={version}"))?
        .error_for_status()
        .with_context(|| format!("wheel download for {package}=={version}"))?
        .bytes()
        .await
        .with_context(|| format!("wheel download body for {package}=={version}"))?;
    tokio::fs::write(&wheel_path, &bytes).await?;
    if archive.exists() {
        tokio::fs::remove_dir_all(&archive)
            .await
            .with_context(|| format!("remove stale wheel archive {}", archive.display()))?;
    }
    tokio::fs::create_dir_all(&archive).await?;
    uv_extract::unzip(
        fs_err::File::open(&wheel_path).context("open wheel")?,
        &archive,
    )
    .context("unzip wheel")?;

    wheel_dist(&filename, display_url, &sha256, archive)
}

/// The `<name>-<version>.dist-info` directory a wheel archive contains:
/// the first two dash-separated components of the wheel filename stem
/// (wheel filenames carry no dashes in either part). Best-effort: even a
/// malformed stem yields a name (e.g. `just-a` from `just-a-name`), which
/// simply never matches a real dist-info dir, so reuse fails harmlessly.
fn dist_info_dir(wheel_stem: &str) -> Option<String> {
    let (name, rest) = wheel_stem.split_once('-')?;
    let (version, _) = rest.split_once('-')?;
    Some(format!("{name}-{version}.dist-info"))
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

#[cfg(test)]
mod tests {
    use super::{
        Role, WHEEL_RANK_REJECT, dist_info_dir, missing_backends, pip_spec, transitive_deps,
        wheel_rank, wheel_specs,
    };

    #[test]
    fn all_backends_resolved_yields_nothing() {
        assert!(missing_backends(|_| true).is_empty());
    }

    #[test]
    fn no_backend_resolved_lists_all_packages_with_clang_format_deduped() {
        let missing = missing_backends(|_| false);
        assert_eq!(
            missing,
            vec![
                ("clang-format".to_owned(), "clang-format".to_owned()),
                ("autopep8".to_owned(), "autopep8".to_owned()),
                ("jsbeautifier".to_owned(), "js-beautify".to_owned()),
                ("djhtml".to_owned(), "djhtml".to_owned()),
                ("cssbeautifier".to_owned(), "css-beautify".to_owned()),
                ("sqlparse".to_owned(), "sqlformat".to_owned()),
            ],
            "first-seen language order; C/C++/Java dedup to one clang-format entry"
        );
        assert_eq!(missing.len(), 6);
    }

    #[test]
    fn partial_resolution_skips_only_resolved_packages_in_order() {
        // clang-format (C/Cpp/Java) and djhtml (HTML) present: the
        // remaining four packages survive, in first-seen order.
        let missing = missing_backends(|tool| tool == "clang-format" || tool == "djhtml");
        assert_eq!(
            missing,
            vec![
                ("autopep8".to_owned(), "autopep8".to_owned()),
                ("jsbeautifier".to_owned(), "js-beautify".to_owned()),
                ("cssbeautifier".to_owned(), "css-beautify".to_owned()),
                ("sqlparse".to_owned(), "sqlformat".to_owned()),
            ]
        );
    }

    #[test]
    fn every_pip_package_is_pinned_to_the_tool_versions_fixture() {
        // `tool-versions.txt` is the CI/doc source of truth; every package
        // reachable from Language::ALL must carry a matching pin.
        let txt = include_str!("../tests/tool-versions.txt");
        for &language in &crate::language::Language::ALL {
            let pkg = language.pip_package();
            let (_, version) = super::PINNED_VERSIONS
                .iter()
                .find(|(p, _)| *p == pkg)
                .unwrap_or_else(|| panic!("{pkg} has no pin in PINNED_VERSIONS"));
            assert!(
                txt.lines().any(|l| *l == format!("{pkg}=={version}")),
                "pin {pkg}=={version} must appear verbatim in tests/tool-versions.txt"
            );
        }
    }

    #[test]
    fn pip_spec_pins_known_packages_and_passes_unknown_bare() {
        assert_eq!(pip_spec("autopep8"), "autopep8==2.3.2");
        assert_eq!(pip_spec("clang-format"), "clang-format==22.1.8");
        assert_eq!(pip_spec("not-a-pinned-package"), "not-a-pinned-package");
    }

    #[test]
    fn dist_info_dir_is_best_effort_for_malformed_stems() {
        assert_eq!(
            dist_info_dir("autopep8-2.3.2-py2.py3-none-any"),
            Some("autopep8-2.3.2.dist-info".to_owned())
        );
        assert_eq!(
            dist_info_dir(
                "clang_format-22.1.8-py2.py3-none-manylinux_2_27_x86_64.manylinux_2_28_x86_64"
            ),
            Some("clang_format-22.1.8.dist-info".to_owned())
        );
        // Malformed stems still yield a best-effort name; a wrong name
        // never matches a real dist-info dir, so reuse fails harmlessly.
        assert_eq!(
            dist_info_dir("just-a-name"),
            Some("just-a.dist-info".to_owned())
        );
        assert_eq!(dist_info_dir("nodashes"), None);
    }

    #[test]
    fn wheel_specs_add_transitive_deps_after_the_primary() {
        let missing = vec![("autopep8".to_owned(), "autopep8".to_owned())];
        assert_eq!(
            wheel_specs(&missing),
            vec![
                (
                    "autopep8".to_owned(),
                    Some("2.3.2".to_owned()),
                    Role::Primary
                ),
                ("pycodestyle".to_owned(), None, Role::Dependency),
            ]
        );
    }

    #[test]
    fn wheel_specs_dedup_shared_transitive_deps() {
        // `editorconfig` and `six` are deps of both jsbeautifier and
        // cssbeautifier: they must be fetched once, after their first
        // parent.
        let missing = vec![
            ("jsbeautifier".to_owned(), "js-beautify".to_owned()),
            ("cssbeautifier".to_owned(), "css-beautify".to_owned()),
        ];
        let specs = wheel_specs(&missing);
        let names: Vec<&str> = specs.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec!["jsbeautifier", "editorconfig", "six", "cssbeautifier"]
        );
        assert!(
            specs
                .iter()
                .filter(|(name, _, role)| *role == Role::Dependency
                    && (name == "editorconfig" || name == "six"))
                .count()
                == 2,
            "each shared dep is listed exactly once"
        );
    }

    #[test]
    fn wheel_rank_prefers_pure_then_manylinux_then_linux_and_rejects_rest() {
        let arch = std::env::consts::ARCH;
        let pure = "pkg-1.0-py3-none-any.whl".to_owned();
        let manylinux = format!("pkg-1.0-cp314-cp314-manylinux_2_17_{arch}.whl");
        let linux = format!("pkg-1.0-cp314-cp314-linux_{arch}.whl");
        let win = "pkg-1.0-cp314-cp314-win_amd64.whl".to_owned();
        let musl = format!("pkg-1.0-cp314-cp314-musllinux_1_2_{arch}.whl");
        assert!(wheel_rank(&pure) > wheel_rank(&manylinux));
        assert!(wheel_rank(&manylinux) > wheel_rank(&linux));
        assert!(wheel_rank(&linux) > WHEEL_RANK_REJECT);
        assert_eq!(wheel_rank(&win), WHEEL_RANK_REJECT);
        assert_eq!(wheel_rank(&musl), WHEEL_RANK_REJECT);
    }

    #[test]
    fn transitive_deps_table_is_consistent_with_the_pins() {
        for (parent, deps) in super::TRANSITIVE_DEPS {
            assert!(
                super::PINNED_VERSIONS.iter().any(|(p, _)| p == parent),
                "{parent} is a backend package and must be pinned"
            );
            for dep in *deps {
                assert!(
                    !super::PINNED_VERSIONS.iter().any(|(p, _)| p == dep),
                    "{dep} is a transitive dependency and must not be pinned \
                     (it resolves to the latest release via PyPI)"
                );
                assert!(
                    transitive_deps(dep).is_empty(),
                    "{dep} must not itself declare transitive deps (no dep chains)"
                );
            }
        }
    }
}
