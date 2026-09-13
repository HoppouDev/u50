//! `FlaskCapability`: installs the pinned Flask stack into the check
//! venv via the uv resolver, lazily on first use (Phase 7).

use anyhow::Result;

#[allow(dead_code)] // scaffolding for the capability layer
/// The pinned Flask stack (check50 parity: these are the packages
/// `check50.flask` needs).
pub const FLASK_REQUIREMENTS: &[&str] = &["flask"];

#[allow(dead_code)] // scaffolding for the capability layer
/// Provisions Flask into the check venv (idempotent: a no-op when
/// already provisioned).
///
/// # Errors
/// Returns an error when the install fails.
pub fn ensure_flask() -> Result<()> {
    let requirements: Vec<String> = FLASK_REQUIREMENTS
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    crate::python::deps::ensure_dependencies(crate::python::venv::interpreter()?, &requirements)
        .map(|_| ())
}
