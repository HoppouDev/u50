//! HTML diff: the character-diff walk with `<ins>`/`<del>` transitions
//! (`Style50.html_diff`).

use super::NEWLINE_MARKER;
use super::TAB_MARKER;
use super::doc_flavor::html_escape;

/// `Style50.html_diff` (`_api.py:221-233`): the same character-diff walk
/// the character renderer ports, with `fmt=html.escape` and transitions
/// emitting `<ins>`/`<del>` tags instead of ANSI colors
/// (`_api.py:225-231`): closing the previous span, then opening the new
/// one — a `?` guide tag opens nothing, and there is no ANSI-reset
/// equivalent. The chunk is wrapped in `<pre>`/`</pre>` and the yielded
/// chunks are joined with newlines (`_api.py:147`:
/// `"\\n".join(self.diff(...))`).
pub(crate) fn render_html_diff(source: &str, formatted: &str) -> String {
    let delta = crate::diff::ndiff_lines(source, formatted);
    let mut chunks: Vec<String> = Vec::new();
    chunks.push("<pre>".to_owned());
    let mut dtype: Option<char> = None;
    let mut line = String::new();
    for &(tag, value) in &delta {
        if Some(tag) != dtype {
            line.push_str(&html_transition(dtype, Some(tag)));
            dtype = Some(tag);
        }
        if value == '\n' {
            if dtype != Some(' ') {
                // Show added/removed newlines (the literal two-char
                // marker, exactly like the character renderer).
                line.push_str(&html_escape(NEWLINE_MARKER));
                line.push_str(&html_transition(dtype, Some(' ')));
            }
            // Don't yield a line if we are removing a newline.
            if dtype != Some('-') {
                chunks.push(std::mem::take(&mut line));
            }
            line.push_str(&html_transition(Some(' '), dtype));
        } else if dtype != Some(' ') && value == '\t' {
            line.push_str(&html_escape(TAB_MARKER));
        } else {
            line.push_str(&html_escape(&value.to_string()));
        }
    }

    // Flush buffer before quitting: the closing transition is part of the
    // last line, which is yielded only when it carries visible content
    // (CPython strips ANSI escapes; the HTML walk never emits any, so the
    // check is plain emptiness — a closing tag makes it non-empty exactly
    // when a span was open).
    line.push_str(&html_transition(dtype, None));
    if !line.is_empty() {
        chunks.push(line);
    }

    chunks.push("</pre>".to_owned());
    chunks.join("\n")
}

/// `html_transition(old_type, new_type)` (`_api.py:225-231`): closing the
/// previous span, then opening the new one. `None` (the sentinel/initial
/// state) and `' '` never open or close anything.
fn html_transition(old: Option<char>, new: Option<char>) -> String {
    let mut tags = String::new();
    for (slash, tag) in [("/", old), ("", new)] {
        let Some(tag) = tag else { continue };
        if tag != '+' && tag != '-' {
            continue;
        }
        tags.push('<');
        if slash == "/" {
            tags.push('/');
        }
        tags.push_str(if tag == '+' { "ins" } else { "del" });
        tags.push('>');
    }
    tags
}
