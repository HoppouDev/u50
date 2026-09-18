use std::collections::HashMap;
use std::sync::OnceLock;

use crate::plugin::resolver::ToolSpec;

pub mod clang_format;

/// One registered code formatter
#[derive(Debug, Clone, Copy)]
pub struct FormatterPlugin {
	pub id: &'static str,
	pub display_name: &'static str,
	/// PyPI package providing this formatter's binary
	pub uv_package: &'static str,
	/// Pinned `uv_package` version, or `None` to resolve the latest release
	pub version: Option<&'static str>,
}

impl FormatterPlugin {
	/// Builds the `uv` resolver spec that provisions this formatter's binary
	#[must_use]
	pub fn tool_spec(&self, domain: &str) -> ToolSpec {
		ToolSpec::for_uv_package(self.id, domain, self.uv_package, self.version)
	}
}

inventory::collect!(FormatterPlugin);

/// Every registered formatter plugin
pub fn all() -> impl Iterator<Item = &'static FormatterPlugin> {
	inventory::iter::<FormatterPlugin>.into_iter()
}

/// Id-indexed view of every registered formatter, built once on first use
fn registry() -> &'static HashMap<&'static str, &'static FormatterPlugin> {
	static REGISTRY: OnceLock<HashMap<&'static str, &'static FormatterPlugin>> = OnceLock::new();
	REGISTRY.get_or_init(|| all().map(|plugin| (plugin.id, plugin)).collect())
}

/// Looks up a registered plugin by its id
#[must_use]
pub fn by_id(id: &str) -> Option<&'static FormatterPlugin> {
	registry().get(id).copied()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::plugin::resolver::ResolverConfig;

	inventory::submit! {
		FormatterPlugin {
			id: "test-fixture-formatter",
			display_name: "Test Fixture Formatter",
			uv_package: "test-fixture-formatter-package",
			version: None,
		}
	}

	inventory::submit! {
		FormatterPlugin {
			id: "test-fixture-formatter-versioned",
			display_name: "Test Fixture Formatter (versioned)",
			uv_package: "test-fixture-formatter-versioned-package",
			version: Some("1.2.3"),
		}
	}

	#[test]
	fn registered_plugin_is_discoverable_by_id() {
		let plugin = by_id("test-fixture-formatter").expect("registered in this test binary");
		assert_eq!(plugin.display_name, "Test Fixture Formatter");
	}

	#[test]
	fn by_id_returns_none_for_an_unregistered_id() {
		assert!(by_id("does-not-exist-formatter").is_none());
	}

	#[test]
	fn registered_ids_are_unique() {
		let mut ids = std::collections::HashSet::new();
		for plugin in all() {
			assert!(ids.insert(plugin.id), "duplicate plugin id `{}`", plugin.id);
		}
	}

	#[test]
	fn tool_spec_targets_the_uv_resolver_with_the_declared_package() {
		let plugin = by_id("test-fixture-formatter").expect("registered in this test binary");
		let spec = plugin.tool_spec("style50");
		assert_eq!(spec.name, "test-fixture-formatter");
		assert_eq!(spec.domain, "style50");
		assert_eq!(spec.resolvers, vec!["uv".to_owned()]);
		assert!(
			matches!(spec.config, ResolverConfig::Uv { ref package, version: None } if package == "test-fixture-formatter-package")
		);
	}

	#[test]
	fn tool_spec_carries_a_pinned_version_when_the_plugin_declares_one() {
		let plugin =
			by_id("test-fixture-formatter-versioned").expect("registered in this test binary");
		let spec = plugin.tool_spec("style50");
		assert!(
			matches!(spec.config, ResolverConfig::Uv { ref version, .. } if version.as_deref() == Some("1.2.3"))
		);
	}
}
