//! The check50 Python environment: a uv-managed `CPython` and the
//! check-domain venv at `<cache>/u50/check50/venv`, provisioned on
//! first use with the same in-process uv machinery style50 uses
//! (Phase 5 folds this into the shared `u50_tools` crate). The shipped
//! [`crate::python::CHECK50_PACKAGE`] is staged into the venv's
//! site-packages on every provisioning pass so it can never go stale.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use uv_cache::Cache;
use uv_client::{BaseClient, BaseClientBuilder};
use uv_python::downloads::{DownloadResult, ManagedPythonDownloadList, PythonDownloadRequest};
use uv_python::managed::{ManagedPythonInstallation, ManagedPythonInstallations};
use uv_python::{Interpreter, VersionRequest};
use uv_virtualenv::{OnExisting, Prompt, Seed, create_venv};

use crate::python::CHECK50_PACKAGE;

/// The pinned interpreter version (style50 parity: same uv-managed
/// build, shared through uv's interpreter cache).
const PINNED_PYTHON: &str = "3.14";

/// The u50 check cache root: the absolute `$XDG_CACHE_HOME` override
/// when set (all platforms), else the platform cache base of
/// `dirs::cache_dir()`, then `u50/check50`.
///
/// # Errors
/// Returns an error when no cache base is determinable.
pub fn cache_dir() -> Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(xdg.join("u50").join("check50"));
    }
    let base = dirs::cache_dir().context("cannot determine the u50 check cache directory")?;
    Ok(base.join("u50").join("check50"))
}

/// The venv's `bin` directory: `Scripts` on Windows (`.exe` shims),
/// `bin` elsewhere.
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

/// The check50 interpreter: `<cache>/u50/check50/venv`'s python,
/// provisioned (uv-managed `CPython` + venv + the shipped `check50`
/// package staged into site-packages) and cached for the process
/// lifetime.
///
/// # Errors
/// Returns an error when provisioning or staging fails.
pub fn interpreter() -> Result<&'static PathBuf> {
    static INTERPRETER: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    INTERPRETER
        .get_or_init(|| ensure().map_err(|e| format!("{e:#}")))
        .as_ref()
        .map_err(|e| anyhow::anyhow!(e.clone()))
}

/// Provisions (or reopens) the check venv and stages the shipped
/// `check50` package into its site-packages.
fn ensure() -> Result<PathBuf> {
    let cache_root = cache_dir()?;
    let venv_path = cache_root.join("venv");
    let python = venv_python(&venv_path);
    if !python.is_file() {
        if venv_path.exists() {
            // A venv dir without interpreters is broken; recreate it.
            std::fs::remove_dir_all(&venv_path).context("remove broken check venv")?;
        }
        provision(&cache_root)?;
    }
    let site_packages = site_packages(&python)?;
    stage_package(&site_packages)?;
    Ok(python)
}

/// Queries the venv interpreter's pure site-packages directory by
/// asking the interpreter itself (robust across layouts).
fn site_packages(python: &Path) -> Result<PathBuf> {
    let out = std::process::Command::new(python)
        .args([
            "-c",
            "import sysconfig; print(sysconfig.get_paths()['purelib'])",
        ])
        .output()
        .context("query venv site-packages")?;
    anyhow::ensure!(
        out.status.success(),
        "query venv site-packages failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    anyhow::ensure!(!path.as_os_str().is_empty(), "empty site-packages path");
    Ok(path)
}

/// Copies the shipped `check50` package into site-packages (fresh on
/// every provisioning pass so the embedded source can never go stale).
fn stage_package(site_packages: &Path) -> Result<()> {
    let target = site_packages.join("check50");
    if target.exists() {
        std::fs::remove_dir_all(&target).context("remove stale check50 package")?;
    }
    std::fs::create_dir_all(&target).context("create check50 package dir")?;
    for file in CHECK50_PACKAGE.files() {
        let path = target.join(file.path());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        std::fs::write(&path, file.contents())
            .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

/// Provisions the uv-managed `CPython` ([`PINNED_PYTHON`]) and creates
/// the check venv when absent (style50 `setup` parity).
fn provision(cache_root: &Path) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?
        .block_on(async {
            let client_builder = BaseClientBuilder::default();
            // Python distribution downloads retry internally
            // (`fetch_with_retry`), so their client disables middleware
            // retries — mirroring uv and style50's setup.
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
                Prompt::Static("u50-check".into()),
                false,             // system_site_packages
                OnExisting::Allow, // idempotent re-runs
                false,             // relocatable
                Seed::Disabled,
                false, // upgradeable
            )
            .context("create check venv")?;
            anyhow::Ok(())
        })
}

/// Downloads and installs the managed `CPython` ([`PINNED_PYTHON`]) into
/// uv's install root (shared with style50) and queries the interpreter.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_venv_python_resolves_per_platform() {
        // The venv exists: the primary `python` name is used.
        let venv = tempfile::tempdir().expect("tempdir");
        let bin = venv_bin_dir(venv.path());
        std::fs::create_dir_all(&bin).expect("create bin");
        std::fs::write(bin.join(tool_file_name("python")), "").expect("create python");
        let python = venv_python(venv.path());
        assert_eq!(
            python,
            bin.join(tool_file_name("python")),
            "the primary python name wins when it exists"
        );
        // Without it, the `python3` fallback applies.
        std::fs::remove_file(bin.join(tool_file_name("python"))).expect("remove python");
        let fallback = venv_python(venv.path());
        assert_eq!(
            fallback,
            bin.join(tool_file_name("python3")),
            "the python3 fallback applies when python is absent"
        );
    }
}
