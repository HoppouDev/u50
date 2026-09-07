//! Diff rendering for text and JSON output modes.
//!
//! The style50 palette is emitted through **crossterm**: every escape the
//! renderers produce comes from a crossterm command's ANSI writer (see
//! [`pinned`]), never from a hand-rolled byte string. Colored output is
//! visually identical to the original's `termcolor` rendering; the bytes
//! follow crossterm's SGR emission (8-bit `38;5;N`/`48;5;N` colors) rather
//! than termcolor's 3-bit `30-37`/`40-47` codes.

use std::path::Path;
use std::sync::LazyLock;

use crossterm::Command;
use crossterm::style::{Attribute, Color, SetAttribute, SetBackgroundColor, SetForegroundColor};
use similar::algorithms::Algorithm;
use similar::{ChangeTag, DiffTag, TextDiff};
use unicode_width::UnicodeWidthChar;

use crate::request::Report;

/// Renders a crossterm command to its ANSI form once and pins it for the
/// process lifetime: the style50 palette is a fixed set of sequences, so
/// the pinned strings are bounded, and handing back `&'static str` keeps
/// the character-mode hot path allocation-free.
fn pinned(command: impl Command) -> &'static str {
    let mut buf = String::new();
    command
        .write_ansi(&mut buf)
        .expect("writing to a String cannot fail");
    Box::leak(buf.into_boxed_str())
}

/// Declares one palette accessor whose sequence is emitted through
/// crossterm's ANSI writer ([`pinned`]) on first use.
macro_rules! palette {
    ($($(#[$doc:meta])* $name:ident = $command:expr;)*) => {
        $(
            $(#[$doc])*
            #[must_use]
            pub(crate) fn $name() -> &'static str {
                static SEQUENCE: LazyLock<&'static str> = LazyLock::new(|| pinned($command));
                *SEQUENCE
            }
        )*
    };
}

palette! {
    /// Red foreground — termcolor's "red" (SGR 31): split-mode deletion
    /// columns.
    red = SetForegroundColor(Color::DarkRed);
    /// Green foreground — termcolor's "green" (SGR 32): split-mode
    /// insertion columns and character mode's `Looks good!`.
    green = SetForegroundColor(Color::DarkGreen);
    /// Yellow foreground — termcolor's "yellow" (SGR 33): the comments
    /// hints and score-mode error lines.
    yellow = SetForegroundColor(Color::DarkYellow);
    /// Cyan foreground — termcolor's "cyan" (SGR 36): the per-file
    /// `::::::::::::::` header of character mode.
    cyan = SetForegroundColor(Color::DarkCyan);
    /// Bright white foreground — termcolor's "white" (SGR 97): the
    /// character-mode banner (which also carries [`bold`]).
    bright_white = SetForegroundColor(Color::White);
    /// Green background — termcolor's "on green" (SGR 42):
    /// character-mode insertions.
    on_green = SetBackgroundColor(Color::DarkGreen);
    /// Red background — termcolor's "on red" (SGR 41): character-mode
    /// deletions.
    on_red = SetBackgroundColor(Color::DarkRed);
    /// Reset all attributes (SGR 0).
    reset = SetAttribute(Attribute::Reset);
    /// Bold (SGR 1).
    bold = SetAttribute(Attribute::Bold);
}
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
pub(crate) fn line_diff<'a>(source: &'a str, formatted: &'a str) -> TextDiff<'a, 'a, 'a, str> {
    TextDiff::configure()
        .algorithm(select_algorithm(source, formatted))
        .diff_lines(source, formatted)
}

/// Line diff for the score renderer: Myers **unconditionally**. The
/// adaptive Lcs switch is display-only — the score sums the change count
/// and must match style50's ndiff-based value on every input.
pub(crate) fn line_diff_score<'a>(
    source: &'a str,
    formatted: &'a str,
) -> TextDiff<'a, 'a, 'a, str> {
    TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .diff_lines(source, formatted)
}

/// The visible marker text for a warned character (style50 renders the
/// literal two-character sequences `\n` / `\t` instead of the raw control
/// characters).
const NEWLINE_MARKER: &str = "\\n";
const TAB_MARKER: &str = "\\t";

/// Enters `tag`'s delta state onto `line`, style50's
/// `color_transition(old_type, new_type)`: a reset closing any previous
/// background, then the new background for `'-'`/`'+'`. For `' '` and the
/// `'?'` guide tag `termcolor.colored("")` is empty, so only the reset
/// remains.
fn push_transition(line: &mut String, tag: char) {
    line.push_str(reset());
    match tag {
        '-' => line.push_str(on_red()),
        '+' => line.push_str(on_green()),
        _ => {}
    }
}

/// Removes ANSI escape sequences the way style50's EOF-flush check does
/// (`re.sub(r"\x1b[^m]*m", "")`): every `ESC ... m` run disappears.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('\u{1b}') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        rest = if let Some(end) = after.find('m') {
            &after[end + 1..]
        } else {
            // Unterminated escape: style50's regex only strips
            // terminated `ESC…m` runs and leaves a bare ESC in
            // place, so keep it and continue scanning after it.
            out.push('\u{1b}');
            after
        };
    }
    out.push_str(rest);
    out
}

/// Character mode, style50 3.0.0 parity (`_api.py::char_diff` +
/// `renderer/_renderers.py::to_ansi`): the full normalized source and its
/// styled content are diffed **character by character** (newlines are diff
/// units too) and the *original text* is re-rendered with
/// character-level background highlighting — inserted characters on green,
/// deleted on red, common text unstyled. Added or removed newlines are
/// shown as a literal `\n` in the active background (an added newline ends
/// the visible line; a deleted one merges the two lines it joined), added
/// or removed tabs as a literal `\t`; each such marker also contributes a
/// legend line `<marker> means that you should insert|delete a
/// newline|tab.`
///
/// The returned per-file dirty block is: a blank line, the highlighted
/// diff, a blank line, one legend line per unique (state, marker) in
/// first-seen order, then — when `hint_comments` is set (style50's
/// `file["comments"]`: the comment ratio of the normalized original is
/// under `COMMENT_MIN`) — a yellow `And consider adding more comments!`
/// line, and a trailing blank line when any legend line was emitted or
/// the hint fired. With `color == false` no ANSI escape is emitted
/// anywhere (markers and layout are identical).
///
/// The character diff consumes [`crate::difflib::ndiff_lines`] — a faithful
/// port of `CPython`'s `difflib.ndiff` (autojunk included), the exact
/// algorithm style50 feeds its `_char_diff` walk — so the delta alignment
/// is identical and character mode is byte-compatible with style50.
///
/// Like style50's own `ndiff`-based walk this is quadratic in the edit
/// distance; on pathological large inputs character mode is the slowest
/// renderer by design.
// style50-parity character mode: the ndiff-style walk, the dtype
// transition bookkeeping and the newline handling form one interleaved
// pass — extracting helpers would scatter the state machine.
#[allow(clippy::too_many_lines)]
pub(crate) fn render_character(
    source: &str,
    formatted: &str,
    color: bool,
    hint_comments: bool,
) -> String {
    // style50 feeds `difflib.ndiff(old, new)` — the raw character
    // sequences — to its walk and reads each delta unit as (d[0], d[2]).
    let delta = crate::difflib::ndiff_lines(source, formatted);

    // The visible diff state — kept exactly like style50's `dtype`: the
    // raw delta tag char (' ', '-', '+', '?'), only reassigned when a
    // delta unit's tag differs from it, and NOT touched by the newline
    // handling (the newline branches emit their own transition pairs
    // without changing `dtype`), so consecutive same-type newlines emit
    // no spurious transitions.
    let mut dtype: Option<char> = None;
    let mut line = String::new();
    let mut body = String::new();
    let mut legend: Vec<(char, &'static str)> = Vec::new();

    // `dtype` (Some(tag) of the current unit) drives transitions; `warn`
    // records a legend entry unless the (state, marker) pair is already
    // there (style50 keeps a set; insertion order is preserved here).
    macro_rules! warn {
        ($state:expr, $marker:expr) => {
            if !legend.contains(&($state, $marker)) {
                legend.push(($state, $marker));
            }
        };
    }

    for &(tag, value) in &delta {
        if dtype != Some(tag) {
            if color {
                push_transition(&mut line, tag);
            }
            dtype = Some(tag);
        }
        let state = dtype.expect("dtype is Some from the first unit on");
        if value == '\n' {
            if state != ' ' {
                warn!(state, NEWLINE_MARKER);
                line.push_str(NEWLINE_MARKER);
                if color {
                    line.push_str(reset());
                }
            }
            // An inserted (or common) newline ends the visible line; a
            // deleted one merges the two lines (no yield).
            if state != '-' {
                body.push_str(&line);
                body.push('\n');
                line.clear();
            }
            if color {
                // Re-open the background for the text that follows
                // (style50's unconditional `transition(" ", dtype)`).
                push_transition(&mut line, state);
            }
        } else if state != ' ' && value == '\t' {
            warn!(state, TAB_MARKER);
            line.push_str(TAB_MARKER);
        } else {
            line.push(value);
        }
    }
    // Close any open background, then flush the pending line only if it
    // carries visible (non-ANSI) content — style50's EOF behavior.
    if color {
        line.push_str(reset());
    }
    if !strip_ansi(&line).is_empty() {
        body.push_str(&line);
        body.push('\n');
    }

    let mut out = String::new();
    out.push('\n');
    out.push_str(&body);
    out.push('\n');
    for (state, marker) in &legend {
        let (background, verb) = match *state {
            '+' => (on_green(), "insert"),
            '-' => (on_red(), "delete"),
            // Equal never warns; the '?' guide tag is unreachable in
            // character mode (a synch pair only passes the similarity
            // cutoff when the characters are equal).
            _ => continue,
        };
        let noun = if *marker == NEWLINE_MARKER {
            "newline"
        } else {
            "tab"
        };
        if color {
            out.push_str(background);
            out.push_str(marker);
            out.push_str(reset());
            out.push_str(yellow());
            out.push_str(" means that you should ");
            out.push_str(verb);
            out.push_str(" a ");
            out.push_str(noun);
            out.push('.');
            out.push_str(reset());
        } else {
            out.push_str(marker);
            out.push_str(" means that you should ");
            out.push_str(verb);
            out.push_str(" a ");
            out.push_str(noun);
            out.push('.');
        }
        out.push('\n');
    }
    if hint_comments {
        if color {
            out.push_str(yellow());
        }
        out.push_str("And consider adding more comments!");
        if color {
            out.push_str(reset());
        }
        out.push('\n');
    }
    if hint_comments || !legend.is_empty() {
        out.push('\n');
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
