//! HTML: the djhtml backend (lenient runner).

use super::Language;
use crate::format::run_tool_lenient;

/// Formats HTML source with `djhtml -` via the lenient runner.
///
/// # Errors
/// Returns an error when `djhtml` is missing or fails.
pub(crate) fn format(source: &str, _language: Language) -> anyhow::Result<String> {
    run_tool_lenient("djhtml", &["-"], source)
}
