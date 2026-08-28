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
}

impl Language {
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
        }
    }

    /// Environment-variable key (after the `U50_STYLE_` prefix) that can
    /// override this language's formatter command line.
    #[must_use]
    pub fn env_var_key(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::Cpp => "CPP",
            Self::Java => "JAVA",
            Self::Python => "PYTHON",
            Self::JavaScript => "JAVASCRIPT",
        }
    }
}

/// Detects the language of `path` from its file extension
/// (c/h -> C, cpp/hpp -> Cpp, java -> Java, py -> Python,
/// js -> JavaScript).
#[must_use]
pub fn detect_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "c" | "h" => Some(Language::C),
        "cpp" | "hpp" => Some(Language::Cpp),
        "java" => Some(Language::Java),
        "py" => Some(Language::Python),
        "js" => Some(Language::JavaScript),
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
        other => format!("`{other}` is required"),
    }
}
