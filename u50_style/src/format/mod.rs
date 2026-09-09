//! The formatter abstraction: the [`Formatter`] trait and the CS50
//! formatter, which resolves the language's registered plugin from
//! [`crate::registry`] and delegates to it (the plugin owns its tools'
//! invocations).

use crate::language::Language;

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
    /// Found by the owning plugin's toolchain resolver (deterministic
    /// install locations, never `PATH`).
    Toolchain,
}

/// Formatter backed by the registered per-language plugins
/// (`crate::registry::languages()`), reproducing the original style50
/// (3.0.0) backend behavior per `style50/languages.py`: where the
/// original calls Python libraries directly, u50 shells out to the
/// corresponding pip-installed CLIs with the same option values (and
/// resolves non-pip tools by other documented means). The exact
/// invocation for each backend — its flags, the CLI quirks they work
/// around, and the byte-parity verification against the original's
/// library calls — is documented on the plugin module itself (see
/// [`crate::language`]).
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
        let tool = language.required_tool();
        if locate_tool(tool).is_none() {
            ensure_backend(tool);
        }
        language.plugin().format(source)
    }
}
