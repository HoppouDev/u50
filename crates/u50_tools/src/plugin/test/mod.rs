#[derive(Debug, Clone, Copy)]
pub struct TestPlugin {
    pub id: &'static str,
    pub display_name: &'static str,
}

inventory::collect!(TestPlugin);

/// Every registered test plugin
pub fn all() -> impl Iterator<Item = &'static TestPlugin> {
    inventory::iter::<TestPlugin>.into_iter()
}

/// Looks up a registered plugin by its id
#[must_use]
pub fn by_id(id: &str) -> Option<&'static TestPlugin> {
    all().find(|plugin| plugin.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    inventory::submit! {
        TestPlugin {
            id: "test-fixture-check-set",
            display_name: "Test Fixture Check Set",
        }
    }

    #[test]
    fn registered_plugin_is_discoverable_by_id() {
        let plugin = by_id("test-fixture-check-set").expect("registered in this test binary");
        assert_eq!(plugin.display_name, "Test Fixture Check Set");
    }

    #[test]
    fn registered_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for plugin in all() {
            assert!(ids.insert(plugin.id), "duplicate plugin id `{}`", plugin.id);
        }
    }
}
