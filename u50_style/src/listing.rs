//! `--list` support: a table of supported languages, their backing
//! binaries, and where each binary was found (PATH, cache, or nowhere).

use std::fmt::Write as _;

use crate::formatter::{ToolOrigin, locate_tool};
use crate::language::Language;

/// Prints the language/binary/status table to stdout:
///
/// ```text
/// Language    Extensions         Binary         Status
/// ----------  -----------------  -------------  ----------
/// C           c, h               clang-format   found (PATH)
/// ```
///
/// # Panics
/// Panics only if a supported language lacks a backing tool — impossible
/// for the fixed language set.
pub fn list_languages() {
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    for &language in &Language::ALL {
        let tool = language
            .required_tool()
            .expect("every supported language has a backing tool");
        let status = match locate_tool(tool) {
            Some((_, ToolOrigin::Path)) => "found (PATH)",
            Some((_, ToolOrigin::Cache)) => "found (cache)",
            None => "missing",
        };
        rows.push((
            language.display_name().to_owned(),
            language.extensions().join(", "),
            tool.to_owned(),
            status.to_owned(),
        ));
    }

    let header = ("Language", "Extensions", "Binary", "Status");
    let widths = [
        header
            .0
            .len()
            .max(rows.iter().map(|r| r.0.len()).max().unwrap_or(0)),
        header
            .1
            .len()
            .max(rows.iter().map(|r| r.1.len()).max().unwrap_or(0)),
        header
            .2
            .len()
            .max(rows.iter().map(|r| r.2.len()).max().unwrap_or(0)),
        header.3.len(),
    ];

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<w0$}  {:<w1$}  {:<w2$}  {}",
        header.0,
        header.1,
        header.2,
        header.3,
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2]
    );
    let _ = writeln!(
        out,
        "{:-<w0$}  {:-<w1$}  {:-<w2$}  {:-<w3$}",
        "",
        "",
        "",
        "",
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3]
    );
    for row in &rows {
        let _ = writeln!(
            out,
            "{:<w0$}  {:<w1$}  {:<w2$}  {}",
            row.0,
            row.1,
            row.2,
            row.3,
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2]
        );
    }
    print!("{out}");
}
