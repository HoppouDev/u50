//! The uv-managed `CPython` and the cache venv: provisioned when absent,
//! reopened when present.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};

use uv_cache::Cache;
use uv_client::{BaseClient, BaseClientBuilder};
use uv_python::downloads::{DownloadResult, ManagedPythonDownloadList, PythonDownloadRequest};
use uv_python::managed::{ManagedPythonInstallation, ManagedPythonInstallations};
use uv_python::{Interpreter, PythonEnvironment, VersionRequest};
use uv_virtualenv::{OnExisting, Prompt, Seed, create_venv};

use crate::formatter::{tool_file_name, venv_bin_dir};

const PINNED_PYTHON: &str = "3.14";

pub(crate) async fn ensure_venv(
    cache_root: &Path,
    uv_cache: &Cache,
    client_builder: &BaseClientBuilder<'_>,
    client: &BaseClient,
) -> Result<PythonEnvironment> {
    let venv_path = cache_root.join("venv");
    let venv_bin = venv_bin_dir(&venv_path);
    let python = venv_bin.join(tool_file_name("python"));
    let python3 = venv_bin.join(tool_file_name("python3"));
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
