//! Character mode: the style50 3.0.0 parity char-diff renderer.

use super::NEWLINE_MARKER;
use super::TAB_MARKER;
use super::palette::{on_green, on_red, reset, yellow};

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
/// The character diff consumes [`crate::diff::ndiff_lines`] — a faithful
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
    let delta = crate::diff::ndiff_lines(source, formatted);

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
