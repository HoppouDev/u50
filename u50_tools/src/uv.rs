//! The uv provisioning pipeline: uv-managed CPython, venvs, and the
//! in-process package installation (extracted from u50_style's setup
//! into the shared crate so every domain consumes the same machinery).

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use uv_cache::Cache;
use uv_client::{BaseClient, BaseClientBuilder};
use uv_python::downloads::{DownloadResult, ManagedPythonDownloadList, PythonDownloadRequest};
use uv_python::managed::{ManagedPythonInstallation, ManagedPythonInstallations};
use uv_python::{Interpreter, VersionRequest};
use uv_virtualenv::{OnExisting, Prompt, Seed, create_venv};

/// The pinned interpreter version (shared with style50 through uv's
/// interpreter cache — no duplicate download).
pub const PINNED_PYTHON: &str = "3.14";

/// The venv's `bin` directory: `Scripts` on Windows, `bin` elsewhere.
#[must_use]
pub fn venv_bin_dir(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    }
}

/// The file name a console script/binary is installed under:
/// `tool.exe` on Windows, `tool` elsewhere.
#[must_use]
pub fn tool_file_name(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_owned()
    }
}

fn venv_python(venv: &Path) -> PathBuf {
    let bin = venv_bin_dir(venv);
    let python = bin.join(tool_file_name("python"));
    if python.is_file() {
        return python;
    }
    bin.join(tool_file_name("python3"))
}

/// The domain cache root: `<base>/u50/<domain>` where `<base>` is the
/// absolute XDG override or the platform cache directory.
#[must_use]
pub fn cache_root_for(domain: &str) -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return xdg.join("u50").join(domain);
    }
    dirs::cache_dir().map_or_else(
        || PathBuf::from(".").join(".u50-cache").join(domain),
        |base| base.join("u50").join(domain),
    )
}

/// The domain cache root from a tool spec.
#[must_use]
pub fn cache_root_for_spec(spec: &crate::resolver::ToolSpec) -> PathBuf {
    cache_root_for(&spec.domain)
}

/// Provisions (or reopens) a domain venv and returns the interpreter
/// path. Cache-first: skips provisioning when the python binary
/// exists. `prompt` names the venv (shown by the interpreter).
///
/// # Errors
/// Returns an error when provisioning fails.
pub fn ensure_venv(domain: &str, cache_root: &Path) -> Result<PathBuf> {
    let venv_path = cache_root.join("venv");
    let python = venv_python(&venv_path);
    if !python.is_file() {
        if venv_path.exists() {
            std::fs::remove_dir_all(&venv_path).context("remove broken venv")?;
        }
        provision(domain, cache_root)?;
    }
    // Re-resolve after provisioning (Windows venvs have no python3
    // shim, so the pre-provision lookup may return a stale fallback).
    let python = venv_python(&venv_path);
    Ok(python)
}

/// Provisions the uv-managed CPython ([`PINNED_PYTHON`]) and creates
/// the venv (style50 `setup` parity).
fn provision(domain: &str, cache_root: &Path) -> Result<()> {
    // uv gates some APIs behind preview mode; without this the crate
    // panics on first use (style50 `setup` parity).
    uv_preview::set(uv_preview::Preview::default()).context("preview init")?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?
        .block_on(async {
            let client_builder = BaseClientBuilder::default();
            let download_client = client_builder
                .clone()
                .retries(0)
                .build()
                .context("download client build")?;
            let uv_cache = Cache::from_path(cache_root.join("uv"))
                .init()
                .await
                .context("uv cache init")?;
            let venv_path = cache_root.join("venv");
            let interpreter =
                provision_python(&client_builder, &download_client, &uv_cache).await?;
            create_venv(
                &venv_path,
                interpreter,
                Prompt::Static(domain.to_owned()),
                false,
                OnExisting::Allow,
                false,
                Seed::Disabled,
                false,
            )
            .context("create venv")?;
            anyhow::Ok(())
        })
}

/// Downloads and installs the managed CPython into uv's install root
/// (shared across domains) and queries the interpreter.
async fn provision_python(
    client_builder: &BaseClientBuilder<'_>,
    client: &BaseClient,
    uv_cache: &Cache,
) -> Result<Interpreter> {
    let retry_policy = client_builder.retry_policy();
    let download_list = ManagedPythonDownloadList::new(client_builder, uv_cache, None)
        .await
        .context("download list")?;
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
    let _lock = installations.lock().await.context("installations lock")?;

    let fetched = download
        .fetch_with_retry(
            client,
            &retry_policy,
            &installation_dir,
            &scratch_dir,
            false,
            None,
            None,
            None,
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
