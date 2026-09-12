//! The check50 Python environment: a uv-managed `CPython` and the
//! check-domain venv at `<cache>/u50/check50/venv`, provisioned on
//! first use with the same in-process uv machinery style50 uses
//! (Phase 5 folds this into the shared `u50_tools` crate). The shipped
//! [`crate::python::CHECK50_PACKAGE`] is staged into the venv's
//! site-packages on every provisioning pass so it can never go stale.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context as _, Result};

use crate::python::CHECK50_PACKAGE;

/// The u50 check cache root: the absolute `$XDG_CACHE_HOME` override
/// when set (all platforms), else the platform cache base of
/// `dirs::cache_dir()`, then `u50/check50`.
///
/// # Errors
/// Returns an error when no cache base is determinable.
pub fn cache_dir() -> Result<PathBuf> {
    Ok(u50_tools::uv::cache_root_for("check50"))
}

/// The venv's `bin` directory: `Scripts` on Windows (`.exe` shims),
/// `bin` elsewhere.
#[must_use]
pub fn venv_bin_dir(venv: &Path) -> PathBuf {
    u50_tools::uv::venv_bin_dir(venv)
}

/// The file name a console script/binary is installed under:
/// `tool.exe` on Windows, `tool` elsewhere.
#[must_use]
pub fn tool_file_name(tool: &str) -> String {
    u50_tools::uv::tool_file_name(tool)
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
    // Re-resolve: provisioning may have created the primary `python`
    // that was absent before (Windows venvs have no `python3` shim,
    // so the pre-provision lookup returns the nonexistent fallback).
    let python = venv_python(&venv_path);
    let site_packages = site_packages(&venv_path)?;
    stage_package(&site_packages)?;
    Ok(python)
}

/// The venv's pure site-packages directory, from the standard layout:
/// `Lib\site-packages` on Windows (no version component),
/// `lib/python3.x/site-packages` on POSIX.
fn site_packages(venv: &Path) -> Result<PathBuf> {
    if cfg!(windows) {
        return Ok(venv.join("Lib").join("site-packages"));
    }
    let lib = venv.join("lib");
    for entry in std::fs::read_dir(&lib).context("read the venv lib dir")? {
        let entry = entry.context("read the venv lib dir")?;
        if entry.file_name().to_string_lossy().starts_with("python3") {
            return Ok(entry.path().join("site-packages"));
        }
    }
    anyhow::bail!("no python3.x directory in {}", lib.display())
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

/// Provisions the check venv via the shared `u50_tools` uv pipeline.
fn provision(cache_root: &Path) -> Result<()> {
    u50_tools::uv::ensure_venv("u50-check", cache_root).map(|_| ())
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
