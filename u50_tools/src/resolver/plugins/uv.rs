//! The uv resolver plugin: pip packages installed via the in-process
//! provisioning pipeline (style50 `setup` parity, shared across
//! domains through `crate::uv`).

use std::path::Path;

use anyhow::Result;

use crate::resolver::{Resolved, ResolverConfig, ResolverPlugin, ToolSpec};
use crate::uv;

/// The uv resolver: locates pip-installed tools in the domain venv's
/// bin dir and provisions them with the in-process uv pipeline.
pub struct UvResolver;

impl ResolverPlugin for UvResolver {
    fn id(&self) -> &'static str {
        "uv"
    }

    fn supports(&self, spec: &ToolSpec) -> bool {
        matches!(spec.config, ResolverConfig::Uv { .. })
    }

    fn resolve(&self, spec: &ToolSpec) -> Option<Resolved> {
        let ResolverConfig::Uv { package, .. } = &spec.config else {
            return None;
        };
        // The tool binary lives in the domain venv's bin dir. The venv
        // path is passed via the config (the domain prefix is the
        // caller's cache root).
        let cache_root = uv::cache_root_for_spec(spec);
        let bin = uv::venv_bin_dir(&cache_root.join("venv"));
        let tool = bin.join(uv::tool_file_name(package));
        if tool.is_file() {
            Some(Resolved { path: tool, origin: "found (cache)" })
        } else {
            None
        }
    }

    fn provision(&self, spec: &ToolSpec) -> Result<()> {
        let ResolverConfig::Uv { package, .. } = &spec.config else {
            anyhow::bail!("uv resolver: not a uv tool spec");
        };
        let cache_root = uv::cache_root_for(spec);
        uv::ensure_venv(&spec.name, &cache_root)?;
        // Package installation into the venv is delegated to the
        // domain's provisioning (the plan's Phase 6 for pip deps); the
        // resolver ensures the venv exists and reports success.
        tracing::debug!(package = %package, tool = %spec.name, "uv resolver: venv ready");
        Ok(())
    }
}
