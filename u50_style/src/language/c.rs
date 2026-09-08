//! C/C++/Java: string stripping, and the clang-format backend.

use super::Language;
use crate::format::run_tool;

/// The C/C++/Java string-strip pass (comment-unaware, linear): removes
/// every *closed* double-quoted string literal including its quotes.
/// Escapes (`\x`) skip the next character and literals may span newlines;
/// an unterminated quote removes nothing and the scan continues right
/// after the quote character (matching `re.sub` restart semantics: only a
/// closed `"(?:\\.|[^"\\])*"` match is removed). Single-quoted char
/// literals are deliberately NOT stripped (style50 quirk, probed:
/// `char c = '//'` counts one comment).
pub(crate) fn c_strip_strings(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            // Try to consume `"(?:\\.|[^"\\])*"` starting here.
            let mut j = i + 1;
            let mut closed = false;
            while j < chars.len() {
                if chars[j] == '\\' {
                    if j + 1 >= chars.len() {
                        break; // dangling escape: the literal cannot close
                    }
                    j += 2;
                } else if chars[j] == '"' {
                    closed = true;
                    break;
                } else {
                    j += 1;
                }
            }
            if closed {
                i = j + 1; // remove the literal including both quotes
            } else {
                out.push('"');
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

const CS50_CLANG_FORMAT_CONFIG: &str = "{ \
AllowShortFunctionsOnASingleLine: Empty, \
BraceWrapping: { AfterCaseLabel: true, AfterControlStatement: true, \
AfterFunction: true, AfterStruct: true, BeforeElse: true, BeforeWhile: true }, \
BreakBeforeBraces: Custom, ColumnLimit: 100, IndentCaseLabels: true, \
IndentWidth: 4, SpaceAfterCStyleCast: true, TabWidth: 4 }";

/// Formats C/C++/Java source with `clang-format`, passing the canonical
/// file name for `language` (`--assume-filename`) so the right lexer is
/// picked.
///
/// # Errors
/// Returns an error when `clang-format` is missing or fails.
pub(crate) fn format(source: &str, language: Language) -> anyhow::Result<String> {
    let assume = format!("--assume-filename={}", language.file_name());
    let style = format!("-style={CS50_CLANG_FORMAT_CONFIG}");
    run_tool("clang-format", &[assume.as_str(), style.as_str()], source)
}
