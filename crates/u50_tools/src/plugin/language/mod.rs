#[derive(Debug, Clone, Copy)]
pub struct LanguagePlugin {
    pub id: &'static str,
    pub display_name: &'static str,
    pub extensions: &'static [&'static str],
}

inventory::collect!(LanguagePlugin);

/// Every registered language plugin
pub fn all() -> impl Iterator<Item = &'static LanguagePlugin> {
    inventory::iter::<LanguagePlugin>.into_iter()
}

/// Looks up a registered plugin by its id
#[must_use]
pub fn by_id(id: &str) -> Option<&'static LanguagePlugin> {
    all().find(|plugin| plugin.id == id)
}

/// Detects the language of `path` from its file extension
#[must_use]
pub fn detect(path: &std::path::Path) -> Option<&'static LanguagePlugin> {
    let ext = path.extension()?.to_str()?;
    all().find(|plugin| plugin.extensions.contains(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    inventory::submit! {
        LanguagePlugin {
            id: "test-fixture-language",
            display_name: "Test Fixture Language",
            extensions: &["testfixture"],
        }
    }

    #[test]
    fn registered_plugin_is_discoverable_by_id() {
        let plugin = by_id("test-fixture-language").expect("registered in this test binary");
        assert_eq!(plugin.display_name, "Test Fixture Language");
    }

    #[test]
    fn detect_finds_plugin_by_extension() {
        let plugin =
            detect(std::path::Path::new("main.testfixture")).expect("detected by extension");
        assert_eq!(plugin.id, "test-fixture-language");
    }

    #[test]
    fn detect_returns_none_for_unknown_extension() {
        assert!(detect(std::path::Path::new("main.unknown-ext")).is_none());
    }

    #[test]
    fn registered_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for plugin in all() {
            assert!(ids.insert(plugin.id), "duplicate plugin id `{}`", plugin.id);
        }
    }
}
