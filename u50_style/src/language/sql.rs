//! SQL: the sqlformat backend with the trailing-newline fix-up.

use super::LanguagePlugin;
use crate::format::run_tool;

/// Formats SQL source with `sqlformat` — verified byte-identical to the
/// original's `sqlparse.format(code, reindent=True, keyword_case="upper",
/// indent_width=4)` plus its trailing-newline fix-up.
///
/// # Errors
/// Returns an error when `sqlformat` is missing or fails.
fn format_sqlformat(source: &str) -> anyhow::Result<String> {
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

/// The SQL language plugin: the sqlformat backend with its
/// trailing-newline fix-up (no comment counter — SQL files are never
/// comment-hinted).
/// Registered in `crate::registry::languages()`.
pub(crate) struct SqlPlugin;
pub(crate) static PLUGIN: SqlPlugin = SqlPlugin;

impl LanguagePlugin for SqlPlugin {
    fn id(&self) -> &'static str {
        "sql"
    }

    fn display_name(&self) -> &'static str {
        "SQL"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["sql"]
    }

    fn required_tool(&self) -> &'static str {
        "sqlformat"
    }

    fn pip_package(&self) -> Option<&'static str> {
        Some("sqlparse")
    }

    fn missing_tool_message(&self) -> String {
        "`sqlformat` is required to check SQL style (pip install sqlparse)".to_owned()
    }

    fn format(&self, source: &str) -> anyhow::Result<String> {
        format_sqlformat(source)
    }
}
