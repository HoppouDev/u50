//! Language detection and per-language metadata for style checking.

use std::path::Path;

/// A language whose style can be checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// C (`.c`, `.h`).
    C,
    /// C++ (`.cpp`, `.hpp`).
    Cpp,
    /// Java (`.java`).
    Java,
    /// Python (`.py`).
    Python,
    /// JavaScript (`.js`).
    JavaScript,
    /// HTML (`.html`).
    Html,
    /// CSS (`.css`).
    Css,
    /// SQL (`.sql`).
    Sql,
}

impl Language {
    /// Every supported language, in listing order (C, C++, Java, Python,
    /// JavaScript, HTML, CSS, SQL — the style50 3.0.0 set).
    pub(crate) const ALL: [Language; 8] = [
        Self::C,
        Self::Cpp,
        Self::Java,
        Self::Python,
        Self::JavaScript,
        Self::Html,
        Self::Css,
        Self::Sql,
    ];

    /// Canonical file name used with `--assume-filename` so clang-format
    /// picks the right lexer for the language (only meaningful for the
    /// clang-format-backed languages).
    #[must_use]
    pub(crate) fn file_name(self) -> &'static str {
        match self {
            Self::C => "foo.c",
            Self::Cpp => "foo.cpp",
            Self::Java => "foo.java",
            _ => unreachable!("clang-format backend only handles C, C++, and Java"),
        }
    }

    /// The external formatter binary this language's style check depends
    /// on — the same tools (or their CLI counterparts) the original
    /// style50 invokes per `languages.py`.
    #[must_use]
    pub fn required_tool(self) -> Option<&'static str> {
        match self {
            Self::C | Self::Cpp | Self::Java => Some("clang-format"),
            Self::Python => Some("autopep8"),
            Self::JavaScript => Some("js-beautify"),
            Self::Html => Some("djhtml"),
            Self::Css => Some("css-beautify"),
            Self::Sql => Some("sqlformat"),
        }
    }

    /// The pip package that provides this language's formatter backend
    /// (all backends are pip-installable: `clang-format` ships a standalone
    /// binary wheel, the rest are pure-Python packages with console
    /// scripts).
    #[must_use]
    pub fn pip_package(self) -> &'static str {
        match self {
            Self::C | Self::Cpp | Self::Java => "clang-format",
            Self::Python => "autopep8",
            Self::JavaScript => "jsbeautifier",
            Self::Html => "djhtml",
            Self::Css => "cssbeautifier",
            Self::Sql => "sqlparse",
        }
    }

    /// Human-readable name used in listings.
    #[must_use]
    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Java => "Java",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Sql => "SQL",
        }
    }

    /// File extensions this language is detected from.
    #[must_use]
    pub(crate) fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::C => &["c", "h"],
            Self::Cpp => &["cpp", "hpp"],
            Self::Java => &["java"],
            Self::Python => &["py"],
            Self::JavaScript => &["js"],
            Self::Html => &["html"],
            Self::Css => &["css"],
            Self::Sql => &["sql"],
        }
    }
}

/// style50 3.0.0's `COMMENT_MIN`: a file is comment-hinted when its
/// comment ratio is *strictly* below 0.10.
const COMMENT_MIN: f64 = 0.10;

/// Counts the lines style50's hint arithmetic uses for `language`: ALL
/// lines for Python (the original's `Python.count_lines` counts blank
/// lines too, per PEP 8), non-blank lines only for every other language.
/// Shared by [`ScoreRenderer`](crate::renderer::ScoreRenderer) and
/// [`comment_hint`] so both use the identical denominator.
pub(crate) fn style50_count_lines(code: &str, language: Language) -> usize {
    if language == Language::Python {
        code.lines().count()
    } else {
        code.lines().filter(|line| !line.trim().is_empty()).count()
    }
}

/// Counts the comments of `code` the way style50 3.0.0's per-language
/// `count_comments` does; `None` for languages whose base class has no
/// counter (HTML, CSS, SQL — those files are never comment-hinted).
pub(crate) fn count_comments(code: &str, language: Language) -> Option<u32> {
    match language {
        Language::C | Language::Cpp | Language::Java => {
            Some(count_c_comments(&c_strip_strings(code)))
        }
        Language::JavaScript => Some(count_c_comments(&js_strip_strings(code))),
        Language::Python => Some(python_comments(code)),
        Language::Html | Language::Css | Language::Sql => None,
    }
}

/// Whether style50's comments hint fires for `code`: the ratio of comments
/// to [`style50_count_lines`] on the *normalized original* must be
/// strictly below [`COMMENT_MIN`] (and the line count non-zero, which the
/// engine's `file is empty` error already guarantees for processed files).
pub(crate) fn comment_hint(code: &str, language: Language) -> bool {
    match count_comments(code, language) {
        Some(comments) => {
            let lines = style50_count_lines(code, language);
            if lines == 0 {
                return false;
            }
            #[allow(clippy::cast_precision_loss)]
            let ratio = f64::from(comments) / lines as f64;
            ratio < COMMENT_MIN
        }
        None => false,
    }
}

/// The C/C++/Java string-strip pass (comment-unaware, linear): removes
/// every *closed* double-quoted string literal including its quotes.
/// Escapes (`\x`) skip the next character and literals may span newlines;
/// an unterminated quote removes nothing and the scan continues right
/// after the quote character (matching `re.sub` restart semantics: only a
/// closed `"(?:\\.|[^"\\])*"` match is removed). Single-quoted char
/// literals are deliberately NOT stripped (style50 quirk, probed:
/// `char c = '//'` counts one comment).
fn c_strip_strings(code: &str) -> String {
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

/// The JavaScript string-strip pass (`Js.match_literals`): double/single
/// quoted strings are same-line only (no DOTALL) and close at the first
/// closing quote whose preceding character is not a backslash; when the
/// line ends first the literal is abandoned (the rest of the line stays,
/// so a `//` on a later line of a multi-line string still counts). A `/`
/// whose previous character is not `*` or `/` and whose next character is
/// not `/` or `*` opens a regex literal, consumed to the next same-line
/// `/` whose preceding character is not a backslash (so a regex
/// containing `//` is never counted as a comment).
fn js_strip_strings(code: &str) -> String {
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

/// The shared C-family comment-count pass over string-stripped text
/// (linear): at `/*` the first `*/` is searched starting *after* the two
/// opening characters (`/*/` never closes), a found pair resumes scanning
/// after it and counts one comment, an unclosed `/*` counts nothing and
/// resumes just after the `/*`; at `//` one comment is counted and the
/// scan skips to the next newline (or EOF); anything else advances one
/// character.
fn count_c_comments(code: &str) -> u32 {
    let chars: Vec<char> = code.chars().collect();
    let mut count = 0;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            match find_star_slash(&chars[i + 2..]) {
                Some(end) => {
                    count += 1;
                    i += 2 + end + 2;
                }
                None => i += 2,
            }
        } else if chars[i] == '/' && chars.get(i + 1) == Some(&'/') {
            count += 1;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    count
}

/// Index of the first `*/` in `haystack`.
fn find_star_slash(haystack: &[char]) -> Option<usize> {
    (0..haystack.len().saturating_sub(1)).find(|&k| haystack[k] == '*' && haystack[k + 1] == '/')
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
fn python_comments(code: &str) -> u32 {
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

/// Detects the language of `path` from its file extension
/// (c/h -> C, cpp/hpp -> Cpp, java -> Java, py -> Python,
/// js -> JavaScript, html -> Html, css -> Css, sql -> Sql).
#[must_use]
pub fn detect_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "c" | "h" => Some(Language::C),
        "cpp" | "hpp" => Some(Language::Cpp),
        "java" => Some(Language::Java),
        "py" => Some(Language::Python),
        "js" => Some(Language::JavaScript),
        "html" => Some(Language::Html),
        "css" => Some(Language::Css),
        "sql" => Some(Language::Sql),
        _ => None,
    }
}

/// The actionable message shown when the formatter binary `tool` is
/// missing (per language, with an install hint).
pub(crate) fn missing_tool_message(tool: &str) -> String {
    match tool {
        "clang-format" => "clang-format is required (>= 14) to check C/C++/Java style".to_owned(),
        "autopep8" => {
            "`autopep8` is required to check Python style (pip install autopep8)".to_owned()
        }
        "js-beautify" => {
            "`js-beautify` is required to check JavaScript style (pip install jsbeautifier)"
                .to_owned()
        }
        "djhtml" => "`djhtml` is required to check HTML style (pip install djhtml)".to_owned(),
        "css-beautify" => {
            "`css-beautify` is required to check CSS style (pip install cssbeautifier)".to_owned()
        }
        "sqlformat" => {
            "`sqlformat` is required to check SQL style (pip install sqlparse)".to_owned()
        }
        other => format!("`{other}` is required"),
    }
}
