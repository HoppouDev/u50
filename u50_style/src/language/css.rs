//! CSS: the css-beautify backend.

use super::LanguagePlugin;
use crate::format::run_tool;

/// Formats CSS source with `css-beautify` — verified byte-identical to
/// the `cssbeautifier.beautify` call the original makes with
/// `indent_size = 4, end_with_newline = True`.
///
/// # Errors
/// Returns an error when `css-beautify` is missing or fails.
fn format_css_beautify(source: &str) -> anyhow::Result<String> {
    run_tool(
        "css-beautify",
        &["--indent-size", "4", "--end-with-newline", "-"],
        source,
    )
}

/// The CSS language plugin: the css-beautify backend (no comment
/// counter — CSS files are never comment-hinted). Registered in
/// `crate::registry::languages()`.
pub(crate) struct CssPlugin;
pub(crate) static PLUGIN: CssPlugin = CssPlugin;

impl LanguagePlugin for CssPlugin {
    fn id(&self) -> &'static str {
        "css"
    }

    fn display_name(&self) -> &'static str {
        "CSS"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["css"]
    }

    fn required_tool(&self) -> &'static str {
        "css-beautify"
    }

    fn pip_package(&self) -> Option<&'static str> {
        Some("cssbeautifier")
    }

    fn missing_tool_message(&self) -> String {
        "`css-beautify` is required to check CSS style (pip install cssbeautifier)".to_owned()
    }

    fn format(&self, source: &str) -> anyhow::Result<String> {
        format_css_beautify(source)
    }
}
