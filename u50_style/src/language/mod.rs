//! Language detection, per-language metadata, and the per-language
//! comment counters. Each language's tokenizer and formatter live in the
//! language's own module (`c.rs`, `python.rs`, ...); this file holds the
//! shared enum, detection, the comment-hint arithmetic, the shared
//! C-family comment counter, and the dispatch to the per-language
//! counters.

use std::path::Path;

use crate::request::FileResult;

pub(crate) mod c;
pub(crate) mod css;
pub(crate) mod html;
pub(crate) mod javascript;
pub(crate) mod python;
pub(crate) mod sql;

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
    /// style50 invokes per `languages.py`. Every supported language has
    /// one, so this is total.
    #[must_use]
    pub(crate) fn required_tool(self) -> &'static str {
        match self {
            Self::C | Self::Cpp | Self::Java => "clang-format",
            Self::Python => "autopep8",
            Self::JavaScript => "js-beautify",
            Self::Html => "djhtml",
            Self::Css => "css-beautify",
            Self::Sql => "sqlformat",
        }
    }

    /// The pip package that provides this language's formatter backend
    /// (all backends are pip-installable: `clang-format` ships a standalone
    /// binary wheel, the rest are pure-Python packages with console
    /// scripts).
    #[must_use]
    pub(crate) fn pip_package(self) -> &'static str {
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
/// Shared by [`ScoreRenderer`](crate::rendering::renderer::ScoreRenderer) and
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
            Some(count_c_comments(&c::c_strip_strings(code)))
        }
        Language::JavaScript => Some(count_c_comments(&javascript::js_strip_strings(code))),
        Language::Python => Some(python::python_comments(code)),
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

/// Whether style50's comments hint fires for a processed file: the
/// [`comment_hint`] ratio rule applied to the file's normalized original.
/// Shared by the console and HTML renderers so both use the identical
/// rule.
pub(crate) fn comment_hinted(result: &FileResult) -> bool {
    result
        .source
        .as_deref()
        .zip(detect_language(&result.path))
        .is_some_and(|(source, language)| comment_hint(source, language))
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
