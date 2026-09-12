//! The toolchain resolver plugin: resolves binaries from the Rust
//! toolchain (rustfmt parity — provision-unsupported by design).

use std::path::PathBuf;

use anyhow::Result;

use crate::resolver::{Resolved, ResolverConfig, ResolverPlugin, ToolSpec};

/// The toolchain resolver: finds binaries in the active Rust toolchain
/// (rustup home or the cargo bin dir).
pub struct ToolchainResolver;

impl ResolverPlugin for ToolchainResolver {
    fn id(&self) -> &'static str {
        "toolchain"
    }

    fn supports(&self, spec: &ToolSpec) -> bool {
        matches!(spec.config, ResolverConfig::Toolchain { .. })
    }

    fn resolve(&self, spec: &ToolSpec) -> Option<Resolved> {
        let ResolverConfig::Toolchain { binary } = &spec.config else {
            return None;
        };
        for base in rust_toolchain_dirs() {
            let path = base.join("bin").join(format!("{binary}.exe"));
            if path.is_file() {
                return Some(Resolved { path, origin: "found (toolchain)" });
            }
            let path = base.join("bin").join(binary);
            if path.is_file() {
                return Some(Resolved { path, origin: "found (toolchain)" });
            }
        }
        None
    }

    fn provision(&self, _spec: &ToolSpec) -> Result<()> {
        anyhow::bail!(
            "the toolchain resolver does not provision (the Rust toolchain manages its own binaries)"
        )
    }
}

/// The Rust toolchain bin dirs (rustup toolchains + cargo home).
fn rust_toolchain_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        // Active toolchain from the rustup settings file is not parsed;
        // all toolchains are scanned instead (same approach as
        // u50_style's rust.rs).
        let toolchains = home.join(".rustup").join("toolchains");
        if let Ok(entries) = std::fs::read_dir(&toolchains) {
            for entry in entries.flatten() {
                dirs.push(entry.path());
            }
        }
        dirs.push(home.join(".cargo"));
    }
    dirs
}
