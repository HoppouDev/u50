//! Split mode: side-by-side columns of width 50 separated by ` | `.

use similar::ChangeTag;
use similar::DiffTag;
use unicode_width::UnicodeWidthChar;

use super::line_diff::{ALL_IN_ONE_GROUP, line_diff, trim_line};
use super::palette::{green, red, reset};

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

/// The column width of one side of a split row.
const SPLIT_WIDTH: usize = 50;
/// Tab stops for split-column expansion.
const TAB_STOP: usize = 4;

/// Expands tabs to the next [`TAB_STOP`] boundary and pads/truncates to
/// exactly [`SPLIT_WIDTH`] **display columns** (unicode width — CJK and
/// emoji occupy 2), so both sides of a split row stay aligned even for
/// wide characters and tabs.
fn fit_column(text: &str) -> String {
    let mut out = String::with_capacity(SPLIT_WIDTH);
    let mut column = 0usize;
    for ch in text.chars() {
        if ch == '\t' {
            let stop = TAB_STOP - (column % TAB_STOP);
            for _ in 0..stop.min(SPLIT_WIDTH - column) {
                out.push(' ');
                column += 1;
            }
        } else {
            let width = ch.width().unwrap_or(0);
            if column + width > SPLIT_WIDTH {
                break;
            }
            out.push(ch);
            column += width;
        }
    }
    while column < SPLIT_WIDTH {
        out.push(' ');
        column += 1;
    }
    out
}

fn split_row(left: &str, deleted: bool, right: &str, inserted: bool, color: bool) -> String {
    let l = fit_column(left);
    let r = fit_column(right);
    let l = if color && deleted {
        format!("{}{l}{}", red(), reset())
    } else {
        l
    };
    let r = if color && inserted {
        format!("{}{r}{}", green(), reset())
    } else {
        r
    };
    format!("{l} | {r}\n")
}
