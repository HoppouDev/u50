#[derive(Debug, Clone, Copy)]
pub struct LanguagePlugin {
    pub id: &'static str,
    pub display_name: &'static str,
    pub extensions: &'static [&'static str],
    /// Id of the `FormatterPlugin` used to format this language
    pub formatter: &'static str,
}

pub mod c;
pub mod cpp;
pub mod java;

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
    all().find(|plugin| {
        plugin
            .extensions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(ext))
    })
}

/// Shared assertions for language plugin test modules
#[cfg(test)]
pub(crate) mod test_support {
    /// Asserts a language plugin is registered with the given identity and formatter link
    pub(crate) fn assert_registered(id: &str, display_name: &str, formatter: &str) {
        let plugin = super::by_id(id).unwrap_or_else(|| panic!("`{id}` is registered"));
        assert_eq!(plugin.display_name, display_name);
        assert_eq!(plugin.formatter, formatter);
    }

    /// Asserts every extension
    pub(crate) fn assert_detects_all(id: &str, extensions: &[&str]) {
        for ext in extensions {
            for candidate in [ext.to_string(), ext.to_ascii_uppercase()] {
                let path = std::path::PathBuf::from(format!("main.{candidate}"));
                let plugin =
                    super::detect(&path).unwrap_or_else(|| panic!("`{}` detected", path.display()));
                assert_eq!(
                    plugin.id, id,
                    "extension `{candidate}` should detect as `{id}`"
                );
            }
        }
    }

    /// Asserts `path` does not detect as language `id`
    pub(crate) fn assert_does_not_detect(id: &str, path: &str) {
        let detected = super::detect(std::path::Path::new(path));
        assert_ne!(detected.map(|plugin| plugin.id), Some(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    inventory::submit! {
        LanguagePlugin {
            id: "test-fixture-language",
            display_name: "Test Fixture Language",
            extensions: &["testfixture"],
            formatter: "test-fixture-formatter",
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
    fn detect_is_case_insensitive() {
        let plugin =
            detect(std::path::Path::new("main.TESTFIXTURE")).expect("detected by extension");
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

    #[test]
    fn every_formatter_reference_resolves_to_a_registered_formatter_plugin() {
        for plugin in all() {
            assert!(
                crate::plugin::formatter::by_id(plugin.formatter).is_some(),
                "language `{}` references unknown formatter `{}`",
                plugin.id,
                plugin.formatter
            );
        }
    }
}
