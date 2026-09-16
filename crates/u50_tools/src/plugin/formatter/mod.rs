#[derive(Debug, Clone, Copy)]
pub struct FormatterPlugin {
    pub id: &'static str,
    pub display_name: &'static str,
}

inventory::collect!(FormatterPlugin);

/// Every registered formatter plugin
pub fn all() -> impl Iterator<Item = &'static FormatterPlugin> {
    inventory::iter::<FormatterPlugin>.into_iter()
}

/// Looks up a registered plugin by its id
#[must_use]
pub fn by_id(id: &str) -> Option<&'static FormatterPlugin> {
    all().find(|plugin| plugin.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    inventory::submit! {
        FormatterPlugin {
            id: "test-fixture-formatter",
            display_name: "Test Fixture Formatter",
        }
    }

    #[test]
    fn registered_plugin_is_discoverable_by_id() {
        let plugin = by_id("test-fixture-formatter").expect("registered in this test binary");
        assert_eq!(plugin.display_name, "Test Fixture Formatter");
    }

    #[test]
    fn registered_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for plugin in all() {
            assert!(ids.insert(plugin.id), "duplicate plugin id `{}`", plugin.id);
        }
    }
}
