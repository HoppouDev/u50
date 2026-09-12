//! The system resolver plugin: discover-only (bounded standard
//! locations), never installs — the read-only escape hatch.

use anyhow::Result;

use crate::resolver::{Resolved, ResolverConfig, ResolverPlugin, ToolSpec};

/// The system resolver: bounded standard-location lookup only.
pub struct SystemResolver;

impl ResolverPlugin for SystemResolver {
    fn id(&self) -> &'static str {
        "system"
    }

    fn supports(&self, spec: &ToolSpec) -> bool {
        matches!(spec.config, ResolverConfig::System { .. })
    }

    fn resolve(&self, spec: &ToolSpec) -> Option<Resolved> {
        let ResolverConfig::System { binary, locations } = &spec.config else {
            return None;
        };
        for location in locations {
            let path = location.join(binary);
            if path.is_file() {
                return Some(Resolved { path, origin: "found (system)" });
            }
            let path = location.join(format!("{binary}.exe"));
            if path.is_file() {
                return Some(Resolved { path, origin: "found (system)" });
            }
        }
        None
    }

    fn provision(&self, spec: &ToolSpec) -> Result<()> {
        anyhow::bail!(
            "the system resolver does not provision: install `{}` with your system package manager (e.g. apt/dnf/brew install {})",
            spec.name,
            spec.name,
        )
    }
}
