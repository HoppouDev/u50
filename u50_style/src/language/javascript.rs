//! JavaScript: the string-strip pass, and the js-beautify backend.

use super::LanguagePlugin;
use super::count_c_comments;
use crate::format::run_tool;

/// Formats JavaScript source with `js-beautify` (the short `-w 100` form
/// is required because this CLI build declares the long
/// `--wrap-line-length` as taking no argument, and the `-` stdin marker
/// must come last because the CLI stops parsing options at the first
/// positional).
///
/// # Errors
/// Returns an error when `js-beautify` is missing or fails.
fn format_js_beautify(source: &str) -> anyhow::Result<String> {
    run_tool(
        "js-beautify",
        &[
            "--end-with-newline",
            "--operator-position",
            "preserve-newline",
            "-w",
            "100",
            "--brace-style",
            "collapse,preserve-inline",
            "--keep-array-indentation",
            "-",
        ],
        source,
    )
}

/// The JavaScript string-strip pass (`Js.match_literals`): double/single
/// quoted strings are same-line only (no DOTALL) and close at the first
/// closing quote whose preceding character is not a backslash; when the
/// line ends first the literal is abandoned (the rest of the line stays,
/// so a `//` on a later line of a multi-line string still counts). A `/`
/// whose previous character is not `*` or `/` and whose next character is
/// not `/` or `*` opens a regex literal, consumed to the next same-line
/// `/` whose preceding character is not a backslash (so a regex
/// containing `//` is never counted as a comment).
pub(crate) fn js_strip_strings(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    let mut prev: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1);
        // End index (exclusive) of the literal starting at `i`, when it
        // closes on this line; `None` leaves the character in place.
        let literal_end = match c {
            '"' | '\'' => same_line_close(&chars, i + 1, |ch, prev_ch| {
                ch == c && prev_ch != Some('\\')
            }),
            '/' if prev != Some('*')
                && prev != Some('/')
                && next != Some(&'/')
                && next != Some(&'*') =>
            {
                same_line_close(&chars, i + 1, |ch, prev_ch| {
                    ch == '/' && prev_ch != Some('\\')
                })
            }
            _ => None,
        };
        if let Some(end) = literal_end {
            i = end; // remove the literal including its quotes
        } else {
            out.push(c);
            i += 1;
        }
        prev = Some(c);
    }
    out
}

/// Scans `chars` from `from` to the first same-line character satisfying
/// `closes` (given the character and its predecessor); returns the index
/// just past it, or `None` at end of line (the literal is abandoned).
fn same_line_close(
    chars: &[char],
    from: usize,
    closes: impl Fn(char, Option<char>) -> bool,
) -> Option<usize> {
    let mut j = from;
    while j < chars.len() && chars[j] != '\n' {
        if closes(
            chars[j],
            j.checked_sub(1).and_then(|k| chars.get(k).copied()),
        ) {
            return Some(j + 1);
        }
        j += 1;
    }
    None
}

/// The JavaScript language plugin: the string-strip pass (this module),
/// the shared C-family comment counter, and the js-beautify backend.
pub(crate) struct JavaScriptPlugin;
pub(crate) static PLUGIN: JavaScriptPlugin = JavaScriptPlugin;

impl LanguagePlugin for JavaScriptPlugin {
    fn id(&self) -> &'static str {
        "javascript"
    }

    fn display_name(&self) -> &'static str {
        "JavaScript"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["js"]
    }

    fn required_tool(&self) -> &'static str {
        "js-beautify"
    }

    fn pip_package(&self) -> Option<&'static str> {
        Some("jsbeautifier")
    }

    fn count_comments(&self, code: &str) -> Option<u32> {
        Some(count_c_comments(&js_strip_strings(code)))
    }

    fn missing_tool_message(&self) -> String {
        "`js-beautify` is required to check JavaScript style (pip install jsbeautifier)".to_owned()
    }

    fn format(&self, source: &str) -> anyhow::Result<String> {
        format_js_beautify(source)
    }
}
