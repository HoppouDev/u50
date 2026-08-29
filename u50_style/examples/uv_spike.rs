//! Phase 0 spike: prove the three uv library provisioning primitives
//! (u50_style/UV_PROVISIONING_PLAN.md):
//!
//!   (a) uv-python downloads/installs a managed CPython 3.14 and reports its location,
//!   (b) uv-virtualenv creates a venv from that interpreter,
//!   (c) uv-installer installs wheels fetched from PyPI into the venv and the
//!       console script runs.
//!
//! Run with: cargo run -p u50_style --example uv_spike --release
#![warn(clippy::pedantic)]

use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use uv_cache::Cache;
use uv_cache_info::CacheInfo;
use uv_client::BaseClientBuilder;
use uv_distribution_filename::WheelFilename;
use uv_distribution_types::{CachedDirectUrlDist, CachedDist};
use uv_installer::Installer;
use uv_pypi_types::{HashDigest, HashDigests, ParsedUrl, VerbatimParsedUrl};
use uv_pep508::VerbatimUrl;
use uv_preview::Preview;
use uv_python::downloads::{DownloadResult, ManagedPythonDownloadList, PythonDownloadRequest};
use uv_python::managed::{ManagedPythonInstallation, ManagedPythonInstallations};
use uv_python::{Interpreter, VersionRequest};
use uv_redacted::DisplaySafeUrl;
use uv_virtualenv::{OnExisting, Prompt, Seed, create_venv};

const PY_VERSION: &str = "3.14";
const CACHE_DIR: &str = "/tmp/u50_spike_cache";
const VENV_DIR: &str = "/tmp/u50_spike_venv";

/// Wheels to install in (c). `autopep8` is the pinned tool version;
/// `pycodestyle` is its runtime dependency (hardcoded for the spike —
/// real dependency resolution is Phase 1 work).
const WHEELS: &[(&str, &str)] = &[("autopep8", "2.3.2"), ("pycodestyle", "2.14.0")];

#[tokio::main]
async fn main() -> Result<()> {
    // Shared cache: interpreter-info queries (Interpreter::query) and wheel
    // storage both want one. Persistent (not temp) so re-runs are cheap.
    let cache = Cache::from_path(CACHE_DIR)
        .init()
        .await
        .context("cache init")?;

    // ---- (a) managed CPython 3.14 -------------------------------------
    let client_builder = BaseClientBuilder::default();
    let retry_policy = client_builder.retry_policy();
    let download_list = ManagedPythonDownloadList::new(&client_builder, &cache, None)
        .await
        .context("download list")?;
    // uv disables its own retries here because downloads retry internally.
    let client = client_builder.retries(0).build().context("client build")?;
    let request = PythonDownloadRequest::default()
        .with_version(VersionRequest::from_str(PY_VERSION).context("version request")?)
        .fill()
        .context("fill request")?;
    let download = download_list
        .find(&request)
        .context("find download")?
        .clone();
    println!("[a] download: {}", download.key());

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
            &client,
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
    println!("[a] managed python: {}", executable.display());

    let interpreter = Interpreter::query(&executable, &cache).context("interpreter query")?;
    println!(
        "[a] interpreter: CPython {} at {}",
        interpreter.python_version(),
        executable.display()
    );

    // ---- (b) venv ------------------------------------------------------
    let venv_path = Path::new(VENV_DIR);
    let venv = create_venv(
        venv_path,
        interpreter,
        Prompt::Static("u50-spike".to_string()),
        false,              // system_site_packages
        OnExisting::Allow,  // idempotent re-runs
        false,              // relocatable
        Seed::Disabled,
        false,              // upgradeable
    )
    .context("create venv")?;
    println!("[b] venv: {}", venv.root().display());
    println!("[b] venv python: {}", venv.python_executable().display());
    println!("[b] venv scripts dir: {}", venv.scripts().display());

    // ---- (c) install wheels fetched from PyPI --------------------------
    let mut dists = Vec::new();
    for (package, version) in WHEELS {
        let dist = fetch_wheel(package, version).await.with_context(|| format!("resolve {package} {version}"))?;
        println!("[c] wheel ready: {}", dist.path().display());
        dists.push(dist);
    }
    Installer::new(&venv, Preview::default())
        .with_cache(&cache)
        .with_installer_metadata(false)
        .install_blocking(dists)
        .context("install wheels")?;
    println!("[c] installed {} wheels", WHEELS.len());

    // Assert the console script exists and runs.
    let script = venv.scripts().join("autopep8");
    if !script.is_file() {
        bail!("console script missing: {}", script.display());
    }
    let output = Command::new(&script)
        .arg("--version")
        .output()
        .with_context(|| format!("spawn {}", script.display()))?;
    anyhow::ensure!(
        output.status.success(),
        "{} failed: {}",
        script.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    println!(
        "[c] autopep8 --version -> {}",
        String::from_utf8_lossy(&output.stdout).trim()
    );
    println!("SPIKE OK");
    Ok(())
}

/// Resolve the pure-python wheel for `package == version` via the PyPI JSON
/// API, download it into the cache dir, and wrap it as a [`CachedDist`] that
/// `uv-installer` accepts.
async fn fetch_wheel(package: &str, version: &str) -> Result<CachedDist> {
    let json: Value = reqwest::get(format!("https://pypi.org/pypi/{package}/{version}/json"))
        .await?
        .error_for_status()?
        .json()
        .await
        .context("pypi json")?;

    let urls = json
        .get("urls")
        .and_then(Value::as_array)
        .context("pypi json: no urls")?;
    let mut pick = None;
    for url in urls {
        if url.get("packagetype").and_then(Value::as_str) != Some("bdist_wheel") {
            continue;
        }
        let Some(filename) = url.get("filename").and_then(Value::as_str) else {
            continue;
        };
        let pure = filename.contains("-py2.py3-none-any") || filename.contains("-py3-none-any");
        if pure {
            pick = Some(url);
            break;
        }
        pick = pick.or(Some(url));
    }
    let Some(entry) = pick else {
        bail!("no wheel for {package}=={version}");
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

    let dir = Path::new(CACHE_DIR).join("spike-wheels");
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(&filename);
    let bytes = reqwest::get(&wheel_url)
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    tokio::fs::write(&path, &bytes).await?;

    let display_url = DisplaySafeUrl::parse(&wheel_url).context("wheel url")?;
    Ok(CachedDist::Url(CachedDirectUrlDist {
        filename: WheelFilename::from_str(&filename).context("wheel filename")?,
        url: VerbatimParsedUrl {
            parsed_url: ParsedUrl::try_from(display_url.clone()).context("parsed url")?,
            verbatim: VerbatimUrl::from_url(display_url),
        },
        path: path.into_boxed_path(),
        hashes: HashDigests::from(vec![HashDigest::from_str(&format!("sha256:{sha256}"))
            .context("hash digest")?]),
        cache_info: CacheInfo::default(),
        build_info: None,
    }))
}
