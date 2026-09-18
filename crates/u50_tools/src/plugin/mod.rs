pub mod formatter;
pub mod language;
pub mod logging;
pub mod resolver;
pub mod test;

pub use formatter::FormatterPlugin;
pub use language::LanguagePlugin;
pub use resolver::ResolverPlugin;
pub use test::TestPlugin;

/// The registered plugin kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
	Language,
	Formatter,
	Test,
	Resolver,
}

impl std::fmt::Display for PluginKind {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(match self {
			Self::Language => "language",
			Self::Formatter => "formatter",
			Self::Test => "test",
			Self::Resolver => "resolver",
		})
	}
}

/// One registered plugin's identity, regardless of kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginInfo {
	pub kind: PluginKind,
	pub id: &'static str,
	pub display_name: &'static str,
}

/// Every registered plugin across all kinds
#[must_use]
pub fn all() -> Vec<PluginInfo> {
	let languages = language::all().map(|plugin| PluginInfo {
		kind: PluginKind::Language,
		id: plugin.id,
		display_name: plugin.display_name,
	});
	let formatters = formatter::all().map(|plugin| PluginInfo {
		kind: PluginKind::Formatter,
		id: plugin.id,
		display_name: plugin.display_name,
	});
	let tests = test::all().map(|plugin| PluginInfo {
		kind: PluginKind::Test,
		id: plugin.id,
		display_name: plugin.display_name,
	});
	let resolvers = resolver::all().map(|plugin| PluginInfo {
		kind: PluginKind::Resolver,
		id: plugin.id(),
		display_name: plugin.display_name(),
	});
	languages
		.chain(formatters)
		.chain(tests)
		.chain(resolvers)
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn all_merges_every_registered_kind() {
		let plugins = all();
		assert!(plugins.iter().any(|p| p.kind == PluginKind::Language));
		assert!(plugins.iter().any(|p| p.kind == PluginKind::Formatter));
		assert!(plugins.iter().any(|p| p.kind == PluginKind::Test));
		// Pin to a known resolver id, not just the kind, so a regression that
		// aliases Resolver entries onto another registry's plugins (as once
		// happened by copy-pasting test::all()) fails this test.
		assert!(
			plugins
				.iter()
				.any(|p| p.kind == PluginKind::Resolver && p.id == "uv")
		);
	}
}
