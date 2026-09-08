//! Unified mode: `git diff`-style output.

use std::path::Path;

use super::line_diff::line_diff;

/// Unified mode: `git diff`-style output.
pub(crate) fn render_unified(source: &str, formatted: &str, path: &Path) -> String {
    let name = path.display().to_string();
    line_diff(source, formatted)
        .unified_diff()
        .context_radius(3)
        .header(&name, &name)
        .to_string()
}
