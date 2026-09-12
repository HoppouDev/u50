//! The resolver plugin system: every tool declares *which* resolver
//! plugin provisions it, and resolver plugins register in their own
//! registry (adding a provisioning mechanism is one module + one
//! registration line).

use std::path::PathBuf;

/// What a resolver found: the binary path and the user-facing origin
/// label (`found (cache)` / `found (system)` / `found (toolchain)`).
#[derive(Debug, Clone)]
pub struct Resolved {
    pub path: PathBuf,
    pub origin: &'static str,
}

/// The resolver-specific configuration, interpreted by the plugin:
/// `uv` -> pip package + version pin; `toolchain` -> binary name;
/// `system` -> search locations + minimum version; `download` ->
/// pinned URL + SHA-256 + platform mapping.
#[derive(Debug, Clone)]
pub enum ResolverConfig {
    /// The uv resolver: a pip package with an optional version pin.
    Uv {
        package: String,
        version: Option<String>,
    },
    /// The toolchain resolver: a binary name to find in the Rust
    /// toolchain (rustfmt parity).
    Toolchain { binary: String },
    /// The system resolver: bounded standard-location lookup only.
    System {
        binary: String,
        locations: Vec<PathBuf>,
    },
    /// The download resolver: a pinned URL + SHA-256 per platform.
    Download { platforms: Vec<PlatformBinary> },
}

/// One pinned platform binary for the download resolver.
#[derive(Debug, Clone)]
pub struct PlatformBinary {
    /// Platform key (e.g. "linux-64", "windows-x64").
    pub platform: String,
    /// The pinned download URL.
    pub url: String,
    /// The pinned SHA-256 of the download.
    pub sha256: String,
    /// Path to the binary inside the extracted archive.
    pub binary_path: String,
}

/// One tool's resolver declaration (see `ToolSpec`).
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// Unique tool name.
    pub name: String,
    /// The cache domain this tool belongs to (e.g. "style50", "check50").
    pub domain: String,
    /// The resolver plugin this tool prefers.
    pub resolver: String,
    /// Optional fallback resolvers, tried in order.
    pub fallback: Vec<String>,
    /// Resolver-specific configuration.
    pub config: ResolverConfig,
}

/// One provisioning mechanism, implemented as a plugin.
pub trait ResolverPlugin: Sync {
    /// Stable id ("uv", "toolchain", "system", "download").
    fn id(&self) -> &'static str;
    /// Can this resolver serve the tool (config shape + platform)?
    fn supports(&self, spec: &ToolSpec) -> bool;
    /// Locate an existing instance, cache-first.
    fn resolve(&self, spec: &ToolSpec) -> Option<Resolved>;
    /// Provision on demand. Resolvers that cannot provision return Err
    /// carrying the actionable guidance instead.
    fn provision(&self, spec: &ToolSpec) -> anyhow::Result<()>;
}

/// Resolves a tool through its declared resolver (cache-first), trying
/// the fallback chain when the primary can't serve it.
pub fn resolve_tool(spec: &ToolSpec, plugins: &[Box<dyn ResolverPlugin>]) -> Option<Resolved> {
    for resolver_id in std::iter::once(&spec.resolver).chain(spec.fallback.iter()) {
        if let Some(plugin) = plugins.iter().find(|p| p.id() == resolver_id)
            && plugin.supports(spec)
            && let Some(resolved) = plugin.resolve(spec)
        {
            return Some(resolved);
        }
    }
    None
}

/// Provisions a tool through its declared resolver.
///
/// # Errors
/// Returns the resolver's error when provisioning fails (including
/// the guidance message from resolvers that cannot provision).
pub fn provision_tool(spec: &ToolSpec, plugins: &[Box<dyn ResolverPlugin>]) -> anyhow::Result<()> {
    for resolver_id in std::iter::once(&spec.resolver).chain(spec.fallback.iter()) {
        if let Some(plugin) = plugins.iter().find(|p| p.id() == resolver_id)
            && plugin.supports(spec)
        {
            return plugin.provision(spec);
        }
    }
    anyhow::bail!(
        "no resolver plugin `{}` (or fallbacks) supports tool `{}`",
        spec.resolver,
        spec.name
    )
}

/// Standard platform key for the current host (for the download
/// resolver's platform mapping).
#[must_use]
pub fn host_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-x64"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "osx-arm64"
        } else {
            "osx-64"
        }
    } else if cfg!(target_arch = "aarch64") {
        "linux-aarch64"
    } else {
        "linux-64"
    }
}
