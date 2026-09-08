//! Unified mode: `git diff`-style output, plus the JSON `patch` field.

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

/// The `patch` field for one file in [`json_document`]: `null` for clean
/// files (legacy schema), otherwise the unified diff of the normalized
/// source against the styled content.
pub(crate) fn patch(result: &crate::request::FileResult) -> Option<String> {
    if result.clean {
        return None;
    }
    result
        .source
        .as_ref()
        .zip(result.formatted.as_ref())
        .map(|(source, formatted)| render_unified(source, formatted, &result.path))
}
