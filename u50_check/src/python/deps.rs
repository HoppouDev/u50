//! pip `dependencies:` provisioning (Phase 6 of
//! `docs/U50_CHECK_PYTHON_PLAN.md)`: dependencies install into the check
//! venv via its pip (a Python subprocess — the standard for
//! user-declared dependencies; the formatter packages in style50 use
//! the in-process uv-installer because they are u50-controlled).
//!
//! The check venv is created with a seeded pip (the provisioning calls
//! `python -m ensurepip` after venv creation when pip is absent).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Installs `dependencies` into the check venv via its pip (lazily on
/// first use; idempotent for already-installed packages). Returns the
/// venv interpreter path for the bridge to use.
///
/// # Errors
/// Returns an error when pip install fails or the venv is missing.
pub fn ensure_dependencies(interpreter: &Path, requirements: &[String]) -> Result<PathBuf> {
    if requirements.is_empty() {
        return Ok(interpreter.to_path_buf());
    }
    install_requirements(interpreter, requirements)?;
    Ok(interpreter.to_path_buf())
}

/// Installs requirements via the venv's pip.
fn install_requirements(python: &Path, requirements: &[String]) -> Result<()> {
    // Ensure pip is available (the venv may have been created without it).
    let ensure = std::process::Command::new(python)
        .args(["-m", "ensurepip", "--upgrade"])
        .output()
        .context("run ensurepip")?;
    let _ = ensure; // idempotent; a non-zero exit means pip already exists

    let out = std::process::Command::new(python)
        .args(["-m", "pip", "install", "--no-cache-dir"])
        .args(requirements)
        .output()
        .context("run pip install")?;
    anyhow::ensure!(
        out.status.success(),
        "pip install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    tracing::debug!(requirements = ?requirements, "deps installed");
    Ok(())
}
