//! The compiled-in plugin registry: the single core file that names
//! check sets (the same model as `u50_style`'s registry).

use crate::checks::hello::HelloPlugin;
use crate::plugin::CheckSetPlugin;

/// The compiled-in check-set plugins.
pub(crate) fn builtin_plugins() -> Vec<Box<dyn CheckSetPlugin>> {
    vec![Box::new(HelloPlugin)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtin_plugin_ids_and_check_names_are_unique() {
        let plugins = builtin_plugins();
        assert!(!plugins.is_empty(), "the registry must not be empty");
        let mut ids = HashSet::new();
        for plugin in &plugins {
            assert!(
                ids.insert(plugin.id().to_owned()),
                "duplicate plugin id `{}`",
                plugin.id()
            );
            let mut names = HashSet::new();
            for check in plugin.checks() {
                assert!(
                    names.insert(check.name.clone()),
                    "duplicate check name `{}` in plugin `{}`",
                    check.name,
                    plugin.id()
                );
                assert!(
                    !check.name.is_empty(),
                    "empty check name in plugin `{}`",
                    plugin.id()
                );
            }
        }
    }
}
