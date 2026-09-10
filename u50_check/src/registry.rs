//! The compiled-in plugin registry: the single core file that names
//! check sets (the same model as `u50_style`'s registry).

use crate::checks::hello::HelloPlugin;
use crate::plugin::CheckSetPlugin;

/// The compiled-in check-set plugins.
pub(crate) fn builtin_plugins() -> Vec<Box<dyn CheckSetPlugin>> {
    vec![Box::new(HelloPlugin)]
}
