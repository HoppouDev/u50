//! Diff rendering for text and JSON output modes.

use std::path::Path;

use similar::{ChangeTag, DiffTag, TextDiff};

use crate::request::Report;

pub(crate) const RED: &str = "\u{1b}[31m";
pub(crate) const GREEN: &str = "\u{1b}[32m";
pub(crate) const BOLD: &str = "\u{1b}[1m";
pub(crate) const RESET: &str = "\u{1b}[0m";

/// Context radius passed to `TextDiff::grouped_ops` to keep every change in
/// a single group. Must satisfy `n * 2 <= usize::MAX` (see
/// `similar::common::group_diff_ops`); `usize::MAX` itself would overflow.
const ALL_IN_ONE_GROUP: usize = usize::MAX / 2;

fn trim_line(value: &str) -> String {
    value.trim_end_matches(['\r', '\n']).to_owned()
}

/// Character mode: per-line diff with inline (character-level) emphasis on
/// changed spans.
pub(crate) fn render_character(source: &str, formatted: &str, color: bool) -> String {
    let diff = TextDiff::from_lines(source, formatted);
    let mut out = String::new();
    for group in &diff.grouped_ops(ALL_IN_ONE_GROUP) {
        for op in group {
            for change in diff.iter_inline_changes(op) {
                let code = match change.tag() {
                    ChangeTag::Equal => None,
                    ChangeTag::Delete => Some(RED),
                    ChangeTag::Insert => Some(GREEN),
                };
                let colored = color && code.is_some();
                if colored {
                    out.push_str(code.unwrap_or(""));
                }
                out.push(match change.tag() {
                    ChangeTag::Equal => ' ',
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                });
                for (emphasized, value) in change.values() {
                    if *emphasized && colored {
                        out.push_str(BOLD);
                    }
                    out.push_str(value.trim_end_matches(['\r', '\n']));
                    if *emphasized && colored {
                        out.push_str(RESET);
                        // A bare RESET would cancel the enclosing line's
                        // color for the rest of the line, so re-establish
                        // it (Equal lines are never colored).
                        if let Some(line) = code {
                            out.push_str(line);
                        }
                    }
                }
                if colored {
                    out.push_str(RESET);
                }
                out.push('\n');
            }
        }
    }
    out
}

/// Split mode: side-by-side columns of width 50 separated by ` | `.
pub(crate) fn render_split(source: &str, formatted: &str, color: bool) -> String {
    let diff = TextDiff::from_lines(source, formatted);
    let mut out = String::new();
    let mut dels: Vec<String> = Vec::new();
    let mut adds: Vec<String> = Vec::new();
    for group in &diff.grouped_ops(ALL_IN_ONE_GROUP) {
        for op in group {
            if op.tag() == DiffTag::Equal {
                flush_split_rows(&mut out, &mut dels, &mut adds, color);
                for change in diff.iter_changes(op) {
                    let line = trim_line(change.value());
                    out.push_str(&split_row(&line, false, &line, false, color));
                }
            } else {
                for change in diff.iter_changes(op) {
                    match change.tag() {
                        ChangeTag::Delete => dels.push(trim_line(change.value())),
                        ChangeTag::Insert => adds.push(trim_line(change.value())),
                        ChangeTag::Equal => {}
                    }
                }
            }
        }
        flush_split_rows(&mut out, &mut dels, &mut adds, color);
    }
    out
}

/// Pairs buffered deletions with insertions, padding the shorter side.
fn flush_split_rows(out: &mut String, dels: &mut Vec<String>, adds: &mut Vec<String>, color: bool) {
    for i in 0..dels.len().max(adds.len()) {
        let left = dels.get(i).map_or(String::new(), Clone::clone);
        let right = adds.get(i).map_or(String::new(), Clone::clone);
        out.push_str(&split_row(
            &left,
            i < dels.len(),
            &right,
            i < adds.len(),
            color,
        ));
    }
    dels.clear();
    adds.clear();
}

fn split_row(left: &str, deleted: bool, right: &str, inserted: bool, color: bool) -> String {
    const WIDTH: usize = 50;
    let mut l = left.chars().take(WIDTH).collect::<String>();
    let mut r = right.chars().take(WIDTH).collect::<String>();
    for _ in l.chars().count()..WIDTH {
        l.push(' ');
    }
    for _ in r.chars().count()..WIDTH {
        r.push(' ');
    }
    if color && deleted {
        l = format!("{RED}{l}{RESET}");
    }
    if color && inserted {
        r = format!("{GREEN}{r}{RESET}");
    }
    format!("{l} | {r}\n")
}

/// Unified mode: `git diff`-style output.
pub(crate) fn render_unified(source: &str, formatted: &str, path: &Path) -> String {
    let name = path.display().to_string();
    TextDiff::from_lines(source, formatted)
        .unified_diff()
        .context_radius(3)
        .header(&name, &name)
        .to_string()
}

/// Builds the single JSON document printed in JSON mode.
pub(crate) fn json_document(report: &Report) -> serde_json::Value {
    serde_json::json!({
        "clean": report.clean(),
        "files": report
            .results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "path": r.path.display().to_string(),
                    "clean": r.clean,
                    "patch": r.rendered,
                })
            })
            .collect::<Vec<serde_json::Value>>(),
    })
}
