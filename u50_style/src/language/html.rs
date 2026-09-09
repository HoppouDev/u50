//! HTML: the djhtml backend (lenient runner).

use super::LanguagePlugin;
use crate::format::run_tool_lenient;

/// Formats HTML source with `djhtml -` via the lenient runner.
///
/// # Errors
/// Returns an error when `djhtml` is missing or fails.
fn format_djhtml(source: &str) -> anyhow::Result<String> {
    run_tool_lenient("djhtml", &["-"], source)
}

/// The HTML language plugin: the djhtml backend (no comment counter —
/// HTML files are never comment-hinted).
/// Registered in `crate::registry::languages()`.
pub(crate) struct HtmlPlugin;
pub(crate) static PLUGIN: HtmlPlugin = HtmlPlugin;

impl LanguagePlugin for HtmlPlugin {
    fn id(&self) -> &'static str {
        "html"
    }

    fn display_name(&self) -> &'static str {
        "HTML"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["html"]
    }

    fn required_tool(&self) -> &'static str {
        "djhtml"
    }

    fn pip_package(&self) -> Option<&'static str> {
        Some("djhtml")
    }

    fn missing_tool_message(&self) -> String {
        "`djhtml` is required to check HTML style (pip install djhtml)".to_owned()
    }

    fn format(&self, source: &str) -> anyhow::Result<String> {
        format_djhtml(source)
    }
}
