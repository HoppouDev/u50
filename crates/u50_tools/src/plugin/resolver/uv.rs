use anyhow::Result;

use super::{Resolved, ResolverConfig, ResolverPlugin, ToolSpec};

/// Resolves python tools with uv
pub struct UvResolver;

impl ResolverPlugin for UvResolver {
    fn id(&self) -> &'static str {
        "uv"
    }

    fn supports(&self, spec: &ToolSpec) -> bool {
        matches!(spec.config, ResolverConfig::Uv { .. })
    }

    fn resolve(&self, _spec: &ToolSpec) -> Option<Resolved> {
        None
    }

    fn provision(&self, spec: &ToolSpec) -> Result<()> {
        anyhow::bail!(
            "the uv resolver does not provision `{}` yet: it is registered against the modular \
             resolver system but not yet wired to uv itself",
            spec.name
        )
    }
}

inventory::submit! {
    super::Registration(&UvResolver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::resolver;

    fn uv_spec() -> ToolSpec {
        ToolSpec {
            name: "black".to_owned(),
            domain: "style50".to_owned(),
            resolvers: vec!["uv".to_owned()],
            config: ResolverConfig::Uv {
                package: "black".to_owned(),
                version: None,
            },
        }
    }

    #[test]
    fn id_is_uv() {
        assert_eq!(UvResolver.id(), "uv");
    }

    #[test]
    fn supports_uv_configs_only() {
        assert!(UvResolver.supports(&uv_spec()));

        let toolchain_spec = ToolSpec {
            config: ResolverConfig::Toolchain {
                binary: "rustfmt".to_owned(),
            },
            ..uv_spec()
        };
        assert!(!UvResolver.supports(&toolchain_spec));
    }

    #[test]
    fn registers_itself_in_the_resolver_registry() {
        let plugin = resolver::by_id("uv").expect("uv resolver is registered");
        assert_eq!(plugin.id(), "uv");
    }

    #[test]
    fn resolve_has_no_backend_yet() {
        assert!(UvResolver.resolve(&uv_spec()).is_none());
    }

    #[test]
    fn provision_is_not_yet_implemented() {
        assert!(UvResolver.provision(&uv_spec()).is_err());
    }
}
