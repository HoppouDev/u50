pub mod uv;

use std::path::PathBuf;

/// What a resolver found
#[derive(Debug, Clone)]
pub struct Resolved {
    pub path: PathBuf,
    pub origin: &'static str,
}

/// One pinned platform binary for a download resolver
#[derive(Debug, Clone)]
pub struct PlatformBinary {
    /// Platform key (`"linux-64"`, `"windows-x64"` etc.)
    pub platform: String,
    /// The pinned download URL
    pub url: String,
    /// The pinned SHA-256 of the download
    pub sha256: String,
    /// Path to the binary inside the extracted archive
    pub binary_path: String,
}

/// The resolver configuration (per-plugin)
#[derive(Debug, Clone)]
pub enum ResolverConfig {
    Uv {
        package: String,
        version: Option<String>,
    },

    Toolchain {
        binary: String,
    },

    System {
        binary: String,
        locations: Vec<PathBuf>,
    },

    Download {
        platforms: Vec<PlatformBinary>,
    },
}

/// One tool's resolver declaration
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// Unique tool name
    pub name: String,

    /// The cache domain this tool belongs to
    pub domain: String,

    /// Resolver ids to try, in order
    pub resolvers: Vec<String>,

    /// Resolver configuration
    pub config: ResolverConfig,
}

pub trait ResolverPlugin: Sync {
    /// Stable id
    fn id(&self) -> &'static str;

    /// Check if a resolver can serve a given tool
    fn supports(&self, spec: &ToolSpec) -> bool;

    /// Locates an existing instance, cache checked first
    fn resolve(&self, spec: &ToolSpec) -> Option<Resolved>;

    /// Provisions the tool on demand
    fn provision(&self, spec: &ToolSpec) -> anyhow::Result<()>;
}

/// A registered `ResolverPlugin` wraps the trait object so `inventory` can collect it
pub struct Registration(pub &'static dyn ResolverPlugin);

inventory::collect!(Registration);

/// Every registered resolver plugin, in registration order
pub fn all() -> impl Iterator<Item = &'static dyn ResolverPlugin> {
    inventory::iter::<Registration>.into_iter().map(|r| r.0)
}

/// Looks up a registered plugin by its id
#[must_use]
pub fn by_id(id: &str) -> Option<&'static dyn ResolverPlugin> {
    all().find(|plugin| plugin.id() == id)
}

/// Resolves a tool through its declared resolver
#[must_use]
pub fn resolve_tool(spec: &ToolSpec) -> Option<Resolved> {
    for resolver_id in &spec.resolvers {
        if let Some(plugin) = by_id(resolver_id)
            && plugin.supports(spec)
            && let Some(resolved) = plugin.resolve(spec)
        {
            return Some(resolved);
        }
    }
    None
}

/// Provisions a tool through its declared resolver
pub fn provision_tool(spec: &ToolSpec) -> anyhow::Result<()> {
    for resolver_id in &spec.resolvers {
        if let Some(plugin) = by_id(resolver_id)
            && plugin.supports(spec)
        {
            return plugin.provision(spec);
        }
    }
    anyhow::bail!(
        "no resolver plugin `{}` supports tool `{}`",
        spec.resolvers.join(", "),
        spec.name
    )
}

/// Standard platform key for the current host
#[must_use]
pub fn host_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "windows-aarch64"
        } else {
            "windows-x64"
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureResolver(&'static str, bool);

    impl ResolverPlugin for FixtureResolver {
        fn id(&self) -> &'static str {
            self.0
        }

        fn supports(&self, spec: &ToolSpec) -> bool {
            matches!(&spec.config, ResolverConfig::Toolchain { binary } if binary == "fixture-tool")
        }

        fn resolve(&self, _spec: &ToolSpec) -> Option<Resolved> {
            self.1.then(|| Resolved {
                path: PathBuf::from("/fixture/bin/tool"),
                origin: "found (fixture)",
            })
        }

        fn provision(&self, _spec: &ToolSpec) -> anyhow::Result<()> {
            Ok(())
        }
    }

    static PRIMARY: FixtureResolver = FixtureResolver("test-fixture-resolver-primary", false);
    static FALLBACK: FixtureResolver = FixtureResolver("test-fixture-resolver-fallback", true);

    inventory::submit! { Registration(&PRIMARY) }
    inventory::submit! { Registration(&FALLBACK) }

    fn fixture_spec() -> ToolSpec {
        ToolSpec {
            name: "fixture-tool".to_owned(),
            domain: "test".to_owned(),
            resolvers: vec![
                "test-fixture-resolver-primary".to_owned(),
                "test-fixture-resolver-fallback".to_owned(),
            ],
            config: ResolverConfig::Toolchain {
                binary: "fixture-tool".to_owned(),
            },
        }
    }

    #[test]
    fn registered_plugin_is_discoverable_by_id() {
        assert!(by_id("test-fixture-resolver-primary").is_some());
    }

    #[test]
    fn resolve_tool_falls_back_when_primary_finds_nothing() {
        let resolved = resolve_tool(&fixture_spec()).expect("fallback resolves");
        assert_eq!(resolved.origin, "found (fixture)");
    }

    #[test]
    fn resolve_tool_returns_none_when_no_resolver_supports_it() {
        let spec = ToolSpec {
            resolvers: vec!["does-not-exist".to_owned()],
            ..fixture_spec()
        };
        assert!(resolve_tool(&spec).is_none());
    }

    #[test]
    fn provision_tool_errors_when_no_resolver_supports_it() {
        let spec = ToolSpec {
            resolvers: vec!["does-not-exist".to_owned()],
            ..fixture_spec()
        };
        assert!(provision_tool(&spec).is_err());
    }

    #[test]
    fn provision_tool_delegates_to_the_supporting_resolver() {
        assert!(provision_tool(&fixture_spec()).is_ok());
    }

    #[test]
    fn host_platform_is_one_of_the_known_keys() {
        let known = [
            "windows-aarch64",
            "windows-x64",
            "osx-arm64",
            "osx-64",
            "linux-aarch64",
            "linux-64",
        ];
        assert!(known.contains(&host_platform()));
    }
}
