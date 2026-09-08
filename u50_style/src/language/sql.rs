//! SQL: the sqlformat backend with the trailing-newline fix-up.

use super::Language;
use crate::format::run_tool;

/// Formats SQL source with `sqlformat` — verified byte-identical to the
/// original's `sqlparse.format(code, reindent=True, keyword_case="upper",
/// indent_width=4)` plus its trailing-newline fix-up.
///
/// # Errors
/// Returns an error when `sqlformat` is missing or fails.
pub(crate) fn format(source: &str, _language: Language) -> anyhow::Result<String> {
    let mut formatted = run_tool(
        "sqlformat",
        &["-k", "upper", "-r", "--indent_width", "4", "-"],
        source,
    )?;
    if !formatted.ends_with('\n') {
        formatted.push('\n');
    }
    Ok(formatted)
}
