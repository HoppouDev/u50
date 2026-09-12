//! Python-check support: the provisioned `CPython` venv, the shipped
//! `check50` package, and the process bridge (Phase 0-2 of
//! `docs/U50_CHECK_PYTHON_PLAN.md`).

pub mod bridge;
pub mod venv;

use include_dir::include_dir;

/// The `check50` Python package shipped by u50 and staged into the
/// check venv's site-packages on every provisioning pass.
pub static CHECK50_PACKAGE: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/python/check50");
