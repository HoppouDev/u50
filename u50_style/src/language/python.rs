//! Python: the tokenize-mirroring comment counter, and the autopep8
//! backend.

use super::Language;
use crate::format::run_tool;

/// Formats Python source with `autopep8` (the original's library options:
/// max-line-length 100, no local config).
///
/// # Errors
/// Returns an error when `autopep8` is missing or fails.
pub(crate) fn format(source: &str, _language: Language) -> anyhow::Result<String> {
    run_tool(
        "autopep8",
        &["-", "--max-line-length=100", "--ignore-local-config"],
        source,
    )
}

/// Token classes the Python mini-lexer needs (a mirror of `tokenize`'s
/// output restricted to what the docstring rule can observe).
#[derive(Clone, Copy, PartialEq, Eq)]
enum PyTok {
    Indent,
    Dedent,
    String,
    FString,
    Newline,
    Nl,
    Other,
}

/// A triple-quoted string still open at end of line.
struct PyStringUnit {
    fstring: bool,
    raw: bool,
    quote: char,
}

/// The Python comment counter: a best-effort mini-lexer mirroring
/// `tokenize` for the token classes above (exotic token streams may
/// diverge from `CPython`). Parity-relevant rules (all probed): `prev`
/// starts as `Indent`, so the module docstring counts; a String token
/// counts exactly when `prev` is `Indent` (the docstring rule), an
/// `FString` token never counts; comment-only lines count one comment and
/// leave `prev` AND the indent stack untouched (so the next content line
/// still emits Indent); blank lines are `Nl`; brackets shift a depth
/// counter and turn newlines into `Nl`; a backslash at end of line is a
/// continuation (no Newline token, `prev` unchanged).
// A linear lexer: splitting it into helpers would obscure the single
// token-stream walk the docstring rule depends on.
#[allow(clippy::too_many_lines)]
pub(crate) fn python_comments(code: &str) -> u32 {
    let mut count = 0;
    let mut prev = PyTok::Indent;
    let mut depth: usize = 0;
    let mut indents: Vec<usize> = vec![0];
    let mut open: Option<PyStringUnit> = None;

    for line in code.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;

        // Continue an open triple-quoted string: find its closing triple
        // on this line, honoring backslash escapes unless raw.
        let mut continued_string = false;
        if let Some(unit) = open.take() {
            continued_string = true;
            if let Some(end) = close_triple(&chars, 0, &unit) {
                let tok = if unit.fstring {
                    PyTok::FString
                } else {
                    PyTok::String
                };
                if tok == PyTok::String && prev == PyTok::Indent {
                    count += 1;
                }
                prev = tok;
                i = end;
            } else {
                open = Some(unit);
                continue; // the whole line is string content
            }
        }

        let rest = &chars[i.min(chars.len())..];
        let Some(first) = rest.iter().position(|c| !c.is_whitespace()) else {
            // Whitespace-only remainder: a genuine blank line is `Nl`;
            // after a string closed mid-line the line ends like a logical
            // line (NEWLINE at depth 0).
            prev = if continued_string && depth == 0 {
                PyTok::Newline
            } else {
                PyTok::Nl
            };
            continue;
        };

        // Comment-only line: one comment; `prev` and the indent stack are
        // both left untouched (probed parity behavior).
        if rest[first] == '#' {
            count += 1;
            continue;
        }

        // Indent stack handling (logical lines at depth 0 only).
        if depth == 0 && !continued_string {
            let indent: usize = rest[..first]
                .iter()
                .map(|c| usize::from(*c == '\t') * 7 + 1)
                .sum();
            let top = *indents.last().expect("indent stack never empty");
            if indent > top {
                indents.push(indent);
                prev = PyTok::Indent;
            } else if indent < top {
                while *indents.last().expect("indent stack never empty") > indent {
                    indents.pop();
                    prev = PyTok::Dedent;
                }
            }
            // Skip the leading whitespace: it is not content, and lexing
            // it would clobber the Indent/Dedent just emitted.
            i = first;
        }

        // Lex the content of the line.
        let mut continuation = false;
        while i < chars.len() {
            let c = chars[i];
            if c == '#' {
                count += 1;
                break; // the rest of the line is comment
            }
            if c == '\\' && i + 1 == chars.len() {
                continuation = true; // backslash-newline: no Newline token
                break;
            }
            if let Some((prefix_len, fstring, raw, quote)) = string_start(&chars, i) {
                let j = i + prefix_len;
                if chars[j..].starts_with(&[quote, quote, quote]) {
                    let unit = PyStringUnit {
                        fstring,
                        raw,
                        quote,
                    };
                    if let Some(end) = close_triple(&chars, j + 3, &unit) {
                        let tok = if fstring {
                            PyTok::FString
                        } else {
                            PyTok::String
                        };
                        if tok == PyTok::String && prev == PyTok::Indent {
                            count += 1;
                        }
                        prev = tok;
                        i = end;
                    } else {
                        open = Some(unit);
                        break; // string continues on the next line
                    }
                    continue;
                }
                // Single-quoted: to the closing quote or end of line.
                let tok = if fstring {
                    PyTok::FString
                } else {
                    PyTok::String
                };
                if tok == PyTok::String && prev == PyTok::Indent {
                    count += 1;
                }
                prev = tok;
                i = single_quote_end(&chars, j + 1, quote, raw).unwrap_or(chars.len());
                continue;
            }
            match c {
                '(' | '[' | '{' => {
                    depth += 1;
                    prev = PyTok::Other;
                }
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    prev = PyTok::Other;
                }
                _ => prev = PyTok::Other,
            }
            i += 1;
        }
        if !continuation && open.is_none() {
            prev = if depth > 0 { PyTok::Nl } else { PyTok::Newline };
        }
    }
    count
}

/// Index of the closing single `quote` at/after `from` (escapes honored
/// unless `raw`), or `None` when the line ends first.
fn single_quote_end(chars: &[char], from: usize, quote: char, raw: bool) -> Option<usize> {
    let mut k = from;
    while k < chars.len() {
        if !raw && chars[k] == '\\' {
            k += 2;
            continue;
        }
        if chars[k] == quote {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// Finds the closing triple quote of `unit` in `chars` starting at
/// `from`; returns the index just past the closing triple, or `None`
/// when the string is still open at end of line (multi-line string).
fn close_triple(chars: &[char], from: usize, unit: &PyStringUnit) -> Option<usize> {
    let mut k = from;
    while k < chars.len() {
        if !unit.raw && chars[k] == '\\' {
            k += 2; // escape: skips the next char (even past end of line)
            continue;
        }
        if chars[k] == unit.quote
            && chars.get(k + 1) == Some(&unit.quote)
            && chars.get(k + 2) == Some(&unit.quote)
        {
            return Some(k + 3);
        }
        k += 1;
    }
    None
}

/// Recognizes a string literal starting at `chars[i]`: an optional prefix
/// of letters from {r, b, u, f} (any case, at most one of each) followed
/// by a quote. Returns the prefix length, whether the prefix contains
/// f/F (making the token an `FString`), whether it contains r/R (raw), and
/// the quote character.
fn string_start(chars: &[char], i: usize) -> Option<(usize, bool, bool, char)> {
    let mut j = i;
    let (mut saw_raw, mut saw_bytes, mut saw_unicode, mut saw_fstring) =
        (false, false, false, false);
    while j - i < 2 && j < chars.len() {
        match chars[j].to_ascii_lowercase() {
            'r' if !saw_raw => saw_raw = true,
            'b' if !saw_bytes => saw_bytes = true,
            'u' if !saw_unicode => saw_unicode = true,
            'f' if !saw_fstring => saw_fstring = true,
            _ => break,
        }
        j += 1;
    }
    let quote = *chars.get(j)?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    Some((j - i, saw_fstring, saw_raw, quote))
}
