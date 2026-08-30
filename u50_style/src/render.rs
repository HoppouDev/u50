//! Diff rendering for text and JSON output modes.

use std::path::Path;

use similar::algorithms::Algorithm;
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

/// Below this line count the overlap probe is not worth its hashing cost
/// (rule: see `select_algorithm`):
/// Myers handles such inputs in single-digit milliseconds (see
/// `examples/bench_diff.rs` for the measurements behind these choices).
const ADAPTIVE_MIN_LINES: usize = 1024;

/// Measured (release, `examples/bench_diff.rs`; wall time for the unified
/// render):
///
/// | input                                    | Myers   | Lcs     |
/// |------------------------------------------|---------|---------|
/// | golden 2.5k real dirty→expected, 26 common | 11.5ms | 0.57ms |
/// | 7.5k wholly-dirty (0 common)             | 509.6ms | 205.7ms |
/// | 60k wholly-dirty (0 common)              | 32.21s  | 13.14s  |
/// | 60k, 28 common (earlier build)           | 32.5s   | 12.9s   |
/// | 7.5k, 8 common (earlier build)           | ~1s     | ~3s     |
///
/// Myers degrades quadratically on large low-overlap pairs while Lcs stays
/// linear-ish — but Lcs collapses once the inputs share a real number of
/// lines (7.5k with 8 common: 3s vs Myers' 1s). Lcs is therefore engaged
/// only when the larger side has at least [`ADAPTIVE_MIN_LINES`] lines AND
/// the distinct shared lines are fewer than a thousandth of it. The 1000x
/// multiplier is a measured heuristic: the crossover between the
/// 8-common@7.5k collapse and the 28-common@60k win lies between those
/// points, and `examples/bench_diff.rs` records the matrix behind it.
///
/// Display-only concern: only diff rendering consults this; the formatter
/// results (and thus clean/dirty decisions) are unaffected.
pub(crate) fn select_algorithm(source: &str, formatted: &str) -> Algorithm {
    let max_lines = source.lines().count().max(formatted.lines().count());
    if max_lines < ADAPTIVE_MIN_LINES {
        return Algorithm::Myers;
    }
    let src: std::collections::HashSet<&str> = source.lines().collect();
    let common = formatted
        .lines()
        .collect::<std::collections::HashSet<&str>>()
        .intersection(&src)
        .count();
    if common.saturating_mul(1000) < max_lines {
        Algorithm::Lcs
    } else {
        Algorithm::Myers
    }
}

/// Diffs the two texts line-wise with the measured algorithm strategy.
fn line_diff<'a>(source: &'a str, formatted: &'a str) -> TextDiff<'a, 'a, 'a, str> {
    TextDiff::configure()
        .algorithm(select_algorithm(source, formatted))
        .diff_lines(source, formatted)
}

/// Character mode: per-line diff with inline (character-level) emphasis on
/// changed spans.
pub(crate) fn render_character(source: &str, formatted: &str, color: bool) -> String {
    let diff = line_diff(source, formatted);
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
    let diff = line_diff(source, formatted);
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
    line_diff(source, formatted)
        .unified_diff()
        .context_radius(3)
        .header(&name, &name)
        .to_string()
}

/// The `patch` field for one file in [`json_document`]: `null` for clean
/// files (legacy schema), otherwise the unified diff of the normalized
/// source against the styled content.
fn patch(result: &crate::request::FileResult) -> Option<String> {
    if result.clean {
        return None;
    }
    result
        .source
        .as_ref()
        .zip(result.formatted.as_ref())
        .map(|(source, formatted)| render_unified(source, formatted, &result.path))
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
                    // Clean files carry no patch (null), matching the legacy
                    // schema; dirty files render the unified diff of source
                    // against formatted.
                    "patch": patch(r),
                })
            })
            .collect::<Vec<serde_json::Value>>(),
    })
}
