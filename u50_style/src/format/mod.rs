//! The formatter abstraction: the [`Formatter`] trait and the CS50
//! formatter dispatching to the per-language backends under
//! [`crate::language`] (which own their tools' invocations).

use crate::language::{Language, c, css, html, javascript, python, sql};

pub(crate) mod tool;

#[cfg(test)]
pub(crate) use tool::cache_bin_dir;
pub use tool::locate_tool;
pub(crate) use tool::{
    cache_dir, ensure_backend, ensure_backends, run_tool, run_tool_lenient, tool_file_name,
    venv_bin_dir,
};

/// Styles one file's source.
pub trait Formatter: Sync {
    /// Formats `source` per CS50 style.
    ///
    /// # Errors
    /// Returns an error when the external formatter fails.
    fn format(&self, source: &str, language: Language) -> anyhow::Result<String>;
}

/// Where a tool command came from: an explicit path, or u50's cache
/// (installed by `u50 --setup` or auto-provisioned on first use).
///
/// u50 NEVER resolves its BUILT-IN formatter tools through the system
/// `PATH`: bare tool names are looked up in the cache only, and missing
/// backends are downloaded into it on first use. [`ToolOrigin::Path`]
/// therefore only ever applies to explicit user-provided paths (see
/// `is_explicit_path`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOrigin {
    /// An explicit path command (see `is_explicit_path`), used as-is.
    Path,
    /// Found in the u50 style cache (`~/.cache/u50/style50`).
    Cache,
}

/// Formatter backed by the same per-language external formatters the
/// original style50 (3.0.0) uses (`style50/languages.py`): clang-format
/// for C/C++/Java, autopep8 for Python, js-beautify for JavaScript,
/// djhtml for HTML, cssbeautifier for CSS, and sqlparse for SQL. The
/// original calls the Python libraries directly (`autopep8`,
/// `jsbeautifier`, `cssbeautifier`, `sqlparse`); u50 shells out to the
/// corresponding pip-installed CLIs, which apply the same defaults. Exact
/// options passed (flag names verified against the installed CLIs; they
/// mirror the original's library options):
///
/// - Python: `autopep8 - --max-line-length=100 --ignore-local-config`
/// - JavaScript: `js-beautify --end-with-newline --operator-position preserve-newline -w 100 --brace-style collapse,preserve-inline --keep-array-indentation -` — the short `-w 100` form is required because this CLI build declares the long `--wrap-line-length` as taking no argument, and the `-` stdin marker must come last because the CLI stops parsing options at the first positional
/// - HTML: `djhtml -` via the lenient runner (`run_tool_lenient`)
/// - CSS: `css-beautify --indent-size 4 --end-with-newline -` — verified byte-identical to the `cssbeautifier.beautify` call the original makes with `indent_size = 4, end_with_newline = True`
/// - SQL: `sqlformat -k upper -r --indent_width 4 -` with a `\n` appended when missing — verified byte-identical to the original's `sqlparse.format(code, reindent=True, keyword_case="upper", indent_width=4)` plus its trailing-newline fix-up
#[derive(Debug, Clone, Default)]
pub struct Cs50Formatter;

impl Formatter for Cs50Formatter {
    /// # Errors
    /// Returns an error when the language's formatter is missing or exits
    /// unsuccessfully.
    fn format(&self, source: &str, language: Language) -> anyhow::Result<String> {
        // style50 3.0.0 raises "file is empty" for empty/whitespace-only
        // files before ever calling a formatter (engine.rs now implements
        // that), so this short-circuit is only a safety net for direct
        // `Formatter::format` callers; empty input no longer reaches it
        // through the engine.
        if source.trim().is_empty() {
            return Ok(source.to_owned());
        }
        // Lazy auto-provisioning: bare tools resolve cache-only, so a
        // missing backend is downloaded into the cache on first use. A
        // failed attempt is only warned about — the `run_tool` call in
        // the language module then produces the usual per-file
        // missing-tool error.
        if let Some(tool) = language.required_tool()
            && locate_tool(tool).is_none()
        {
            ensure_backend(tool);
        }
        match language {
            Language::C | Language::Cpp | Language::Java => c::format(source, language),
            Language::Python => python::format(source, language),
            Language::JavaScript => javascript::format(source, language),
            Language::Html => html::format(source, language),
            Language::Css => css::format(source, language),
            Language::Sql => sql::format(source, language),
        }
    }
}
