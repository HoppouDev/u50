//! CSS: the css-beautify backend.

use super::Language;
use crate::format::run_tool;

/// Formats CSS source with `css-beautify` — verified byte-identical to
/// the `cssbeautifier.beautify` call the original makes with
/// `indent_size = 4, end_with_newline = True`.
///
/// # Errors
/// Returns an error when `css-beautify` is missing or fails.
pub(crate) fn format(source: &str, _language: Language) -> anyhow::Result<String> {
    run_tool(
        "css-beautify",
        &["--indent-size", "4", "--end-with-newline", "-"],
        source,
    )
}
