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
