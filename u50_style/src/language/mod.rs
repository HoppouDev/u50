//! Language plugins and the shared comment-hint arithmetic. Each
//! language is one module implementing [`LanguagePlugin`] and
//! registering its `PLUGIN` in [`crate::registry`]; this file holds the
//! trait, the [`Language`] handle, detection, the comment-hint
//! arithmetic, and the shared C-family comment counter.

use std::path::Path;

use crate::registry;
use crate::request::FileResult;

pub(crate) mod c;
pub(crate) mod css;
pub(crate) mod html;
pub(crate) mod javascript;
pub(crate) mod python;
pub(crate) mod rust;
pub(crate) mod sql;

/// One language, fully self-contained: metadata, detection inputs, the
/// comment counter, and the formatting backend. Implemented by a
/// zero-sized struct in the language's own module and registered in
/// `registry::languages()` — the single core file that names plugins
/// (the `gate.AddPlugin` analog). Every method has a sensible default
/// so a minimal plugin implements only the identity methods and
/// [`LanguagePlugin::format`].
pub(crate) trait LanguagePlugin: Sync {
    /// Stable machine id (`"rust"`), also the registry lookup key and
    /// the [`Language`] equality basis.
    fn id(&self) -> &'static str;

    /// Human-readable name for `--status` / listings (`"Rust"`).
    fn display_name(&self) -> &'static str;

    /// File extensions detected for this language.
    fn extensions(&self) -> &'static [&'static str];

    /// The backing binary; every language has exactly one.
    fn required_tool(&self) -> &'static str;

    /// The pip package provisioning
    /// [`required_tool`](Self::required_tool), or `None` when it cannot
    /// be pip-provisioned (rustfmt resolves from the Rust toolchain
    /// instead and is never auto-provisioned).
    fn pip_package(&self) -> Option<&'static str> {
        None
    }

    /// Canonical file name passed to tools that lex by filename
    /// (clang-format's `--assume-filename`); `None` = not applicable.
    fn assume_filename(&self) -> Option<&'static str> {
        None
    }

    /// Comment counter mirroring style50's per-language
    /// `count_comments`; `None` = the language is never comment-hinted
    /// (HTML/CSS/SQL).
    fn count_comments(&self, _code: &str) -> Option<u32> {
        None
    }

    /// The style-check line-count rule used as the comment-hint and
    /// score denominator (default: non-blank lines only).
    fn count_lines(&self, code: &str) -> usize {
        code.lines().filter(|line| !line.trim().is_empty()).count()
    }

    /// Formats normalized source with this language's backend.
    ///
    /// # Errors
    /// Returns an error when the backend is missing or fails.
    fn format(&self, source: &str) -> anyhow::Result<String>;

    /// Optional fallback resolution for tools that cannot be located
    /// cache-only (rustfmt resolves from the Rust toolchain); only ever
    /// called for the plugin owning `tool`. `None` = not resolvable
    /// here.
    fn resolve_tool(&self, _tool: &str) -> Option<std::path::PathBuf> {
        None
    }

    /// Where a missing [`required_tool`](Self::required_tool) is
    /// searched, for the not-found error message.
    fn tool_search_scope(&self) -> &'static str {
        "the u50 style cache"
    }

    /// The actionable message shown when
    /// [`required_tool`](Self::required_tool) is missing.
    fn missing_tool_message(&self) -> String {
        format!("`{}` is required", self.required_tool())
    }
}

/// A cheap handle to a registered language plugin: `Copy`, compared by
/// plugin id, and the type [`detect_language`] returns. The plugin
/// dispatch lives behind the handle — core code never matches on
/// specific languages.
#[derive(Clone, Copy)]
pub struct Language(pub(crate) &'static dyn LanguagePlugin);

impl Language {
    /// The registered plugin behind this handle.
    #[must_use]
    pub(crate) fn plugin(self) -> &'static dyn LanguagePlugin {
        self.0
    }

    /// The backing binary (see [`LanguagePlugin::required_tool`]).
    #[must_use]
    pub(crate) fn required_tool(self) -> &'static str {
        self.0.required_tool()
    }

    /// The pip package (see [`LanguagePlugin::pip_package`]).
    #[must_use]
    pub(crate) fn pip_package(self) -> Option<&'static str> {
        self.0.pip_package()
    }

    /// The human-readable name (see [`LanguagePlugin::display_name`]).
    #[must_use]
    pub(crate) fn display_name(self) -> &'static str {
        self.0.display_name()
    }

    /// The detected extensions (see [`LanguagePlugin::extensions`]).
    #[must_use]
    pub(crate) fn extensions(self) -> &'static [&'static str] {
        self.0.extensions()
    }
}

impl Language {
    /// Looks up a registered language plugin by its machine id — the
    /// plugin-registry analog of gate's plugin lookup by name. `None`
    /// when no plugin with that id is registered.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Language> {
        crate::registry::languages()
            .iter()
            .copied()
            .find(|&plugin| plugin.id() == id)
            .map(Language)
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.0.id() == other.0.id()
    }
}

impl Eq for Language {}

impl std::fmt::Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Language({})", self.0.id())
    }
}

/// Detects the language of `path` from its file extension, searching
/// the registered plugins in registry order (c/h -> C, cpp/hpp -> Cpp,
/// java -> Java, py -> Python, js -> JavaScript, html -> Html,
/// css -> Css, sql -> Sql, rs -> Rust).
#[must_use]
pub fn detect_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    registry::languages()
        .iter()
        .copied()
        .find(|plugin| plugin.extensions().contains(&ext))
        .map(Language)
}

/// style50 3.0.0's `COMMENT_MIN`: a file is comment-hinted when its
/// comment ratio is *strictly* below 0.10.
const COMMENT_MIN: f64 = 0.10;

/// Counts the lines style50's hint arithmetic uses for `language`:
/// the plugin's [`LanguagePlugin::count_lines`] rule (ALL lines for
/// Python — the original counts blank lines there, per PEP 8 —
/// non-blank lines only for every other language). Shared by
/// [`ScoreRenderer`](crate::rendering::renderer::ScoreRenderer) and
/// [`comment_hint`] so both use the identical denominator.
pub(crate) fn style50_count_lines(code: &str, language: Language) -> usize {
    language.plugin().count_lines(code)
}

/// Counts the comments of `code` the way style50 3.0.0's per-language
/// `count_comments` does; `None` for languages without a counter
/// (HTML, CSS, SQL — those files are never comment-hinted).
pub(crate) fn count_comments(code: &str, language: Language) -> Option<u32> {
    language.plugin().count_comments(code)
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

/// The C/C++/Java/Rust string-strip pass (comment-unaware, linear): removes
/// every *closed* double-quoted string literal including its quotes.
/// Escapes (`\x`) skip the next character and literals may span newlines;
/// an unterminated quote removes nothing and the scan continues right
/// after the quote character (matching `re.sub` restart semantics: only a
/// closed `"(?:\\.|[^"\\])*"` match is removed). Single-quoted char
/// literals are deliberately NOT stripped (style50 quirk, probed:
/// `char c = '//'` counts one comment) — Rust lifetimes (`'a`) are
/// therefore safe too.
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

/// Counts the comments of string-stripped C-family text: strip with
/// [`c_strip_strings`], count with [`count_c_comments`]. One helper so
/// the C/C++/Java and Rust plugins share the identical pipeline.
pub(crate) fn count_c_family_comments(code: &str) -> u32 {
    count_c_comments(&c_strip_strings(code))
}

/// The shared C-family comment-count pass over string-stripped text
/// (linear): at `/*` the first `*/` is searched starting *after* the two
/// opening characters (`/*/` never closes), a found pair resumes scanning
/// after it and counts one comment, an unclosed `/*` counts nothing and
/// resumes just after the `/*`; at `//` one comment is counted and the
/// scan skips to the next newline (or EOF); anything else advances one
/// character. Shared by the C/C++/Java and Rust plugins.
pub(crate) fn count_c_comments(code: &str) -> u32 {
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
