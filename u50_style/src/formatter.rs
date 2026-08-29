//! Formatter backend backed by external style tools.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::language::{Language, missing_tool_message};

/// The clang-format style configuration CS50 uses for its style checks
/// (recorded verbatim from the original `style50` source).
const CS50_CLANG_FORMAT_CONFIG: &str = "{ \
AllowShortFunctionsOnASingleLine: Empty, \
BraceWrapping: { AfterCaseLabel: true, AfterControlStatement: true, \
AfterFunction: true, AfterStruct: true, BeforeElse: true, BeforeWhile: true }, \
BreakBeforeBraces: Custom, ColumnLimit: 100, IndentCaseLabels: true, \
IndentWidth: 4, SpaceAfterCStyleCast: true, TabWidth: 4 }";

/// Styles one file's source.
pub trait Formatter {
    /// Formats `source` per CS50 style.
    ///
    /// # Errors
    /// Returns an error when the external formatter fails.
    fn format(&self, source: &str, language: Language) -> anyhow::Result<String>;
}

/// Where a tool binary was found: the system `PATH`, or u50's cache
/// (installed by `u50 style --setup`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOrigin {
    /// Found on `PATH`.
    Path,
    /// Found in the u50 style cache (`~/.cache/u50/style50`).
    Cache,
}

/// The u50 style cache root: `$XDG_CACHE_HOME` or `~/.cache`, then
/// `u50/style50`.
#[must_use]
pub(crate) fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|h| h.join(".cache"))
        })
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("u50").join("style50")
}

/// The directory holding binaries installed by `u50 style --setup`
/// (the uv-managed venv `<cache>/venv` puts console scripts in `bin/`).
#[must_use]
pub(crate) fn cache_bin_dir() -> PathBuf {
    cache_dir().join("venv").join("bin")
}

/// Whether `path` is an existing regular file with an execute bit.
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

/// Resolves `tool` to its location: a path containing `/` is used as-is;
/// otherwise `PATH` is searched first, then the u50 style cache bin dir
/// (`u50 style --setup`'s install location). Returns `None` when the tool
/// is nowhere to be found.
#[must_use]
pub fn locate_tool(tool: &str) -> Option<(PathBuf, ToolOrigin)> {
    if tool.contains('/') {
        return Some((PathBuf::from(tool), ToolOrigin::Path));
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(tool);
            if is_executable_file(&candidate) {
                return Some((candidate, ToolOrigin::Path));
            }
        }
    }
    let cached = cache_bin_dir().join(tool);
    if is_executable_file(&cached) {
        return Some((cached, ToolOrigin::Cache));
    }
    None
}

/// The path of [`locate_tool`], when found. Part of the crate's tool
/// management API (exercised by tests; `run_process` uses the richer
/// [`locate_tool`]).
#[must_use]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn resolve_tool(tool: &str) -> Option<PathBuf> {
    locate_tool(tool).map(|(path, _)| path)
}

/// Spawns `tool` with `args`, feeds `source` on stdin (written from a
/// separate thread so a child that fills its stdout pipe cannot deadlock
/// against us still writing its stdin), and waits for it to exit. Spawn
/// errors are mapped by `on_spawn` so callers can phrase the failure for
/// their context (built-in install hint vs. override env var).
///
/// # Errors
/// Returns any error while attaching stdin or waiting on the child;
/// spawn failures are mapped by `on_spawn` instead.
fn run_process(
    tool: &str,
    args: &[&str],
    source: &str,
    on_spawn: impl Fn(std::io::Error) -> anyhow::Error,
) -> anyhow::Result<std::process::Output> {
    // Cache-aware resolution: a bare tool name is looked up on PATH
    // first, then in the u50 style cache. Only cache hits get their spawn
    // rewritten to the resolved path; PATH hits and unresolved tools spawn
    // by name exactly as before, preserving the missing-tool error
    // messages, and paths containing `/` are used as-is (overrides). No
    // env fixup is needed: the venv console scripts installed by `--setup`
    // carry absolute shebangs and are self-contained.
    let mut command = Command::new(tool);
    if !tool.contains('/')
        && let Some((path, ToolOrigin::Cache)) = locate_tool(tool)
    {
        command = Command::new(path);
    }
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(on_spawn)?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("could not attach stdin to `{tool}`"))?;
    let source = source.to_owned();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(source.as_bytes());
    });
    let output = child.wait_with_output()?;
    let _ = writer.join();
    Ok(output)
}

/// Runs a user-provided formatter override (`U50_STYLE_<LANG>`) strictly:
/// exit 0 is the only success.
fn run_override(var: &str, command: &[String], source: &str) -> anyhow::Result<String> {
    let (binary, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty override command in {var}"))?;
    let joined = command.join(" ");
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_process(binary, &arg_refs, source, |e| {
        anyhow::anyhow!("could not run `{binary}` (set via {var}): {e}")
    })?;
    if !output.status.success() {
        anyhow::bail!(
            "formatter `{joined}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn run_tool(tool: &str, args: &[&str], source: &str) -> anyhow::Result<String> {
    let output = run_process(tool, args, source, |e| {
        anyhow::anyhow!("{}: {e}", missing_tool_message(tool))
    })?;
    if !output.status.success() {
        anyhow::bail!(
            "`{tool}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Runs `tool` with `args`, feeding `source` on stdin, tolerating the
/// "exit 1 means reformatted" diff/black exit-code convention followed
/// by older `djhtml` releases (the convention `style50/languages.py`
/// documents for it): exit 0 is success, and exit 1 with non-empty
/// stdout is also treated as success; anything else is an error. The
/// installed djhtml (3.0.11; like 3.0.6 before it) always exits 0 (the
/// source comment is stale for it), so in practice the strict path is
/// what runs — the leniency keeps u50 compatible with older djhtml
/// versions too.
fn run_tool_lenient(tool: &str, args: &[&str], source: &str) -> anyhow::Result<String> {
    let output = run_process(tool, args, source, |e| {
        anyhow::anyhow!("{}: {e}", missing_tool_message(tool))
    })?;
    let reformatted_on_exit_1 = output.status.code() == Some(1) && !output.stdout.is_empty();
    if !output.status.success() && !reformatted_on_exit_1 {
        anyhow::bail!(
            "`{tool}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
/// - HTML: `djhtml -` via the lenient runner ([`run_tool_lenient`])
/// - CSS: `css-beautify --indent-size 4 --end-with-newline -` — verified byte-identical to the `cssbeautifier.beautify` call the original makes with `indent_size = 4, end_with_newline = True`
/// - SQL: `sqlformat -k upper -r --indent_width 4 -` with a `\n` appended when missing — verified byte-identical to the original's `sqlparse.format(code, reindent=True, keyword_case="upper", indent_width=4)` plus its trailing-newline fix-up
///
/// Any language can additionally be redirected to a custom command line
/// via the `U50_STYLE_<LANG>` environment variable (see
/// [`overrides_from_env`]); the source is piped to the custom tool via
/// stdin and its exit code is treated strictly (exit 0 = success).
#[derive(Debug, Clone, Default)]
pub struct Cs50Formatter {
    /// Per-language override command lines (`U50_STYLE_<LANG>`), private —
    /// populated via [`Cs50Formatter::with_overrides`] or
    /// [`Cs50Formatter::from_env`].
    overrides: HashMap<Language, Vec<String>>,
}

/// Pure parser for the `U50_STYLE_<LANG>` override contract: for each
/// known language key, a non-empty (after trimming) variable value is
/// split on whitespace into an argv (no quoting support); empty and
/// unknown variables are ignored. `vars` is a lookup closure so tests can
/// pass a fake map instead of the process environment.
pub(crate) fn overrides_from_env(
    vars: impl Fn(&str) -> Option<String>,
) -> HashMap<Language, Vec<String>> {
    let languages = [
        Language::C,
        Language::Cpp,
        Language::Java,
        Language::Python,
        Language::JavaScript,
        Language::Html,
        Language::Css,
        Language::Sql,
    ];
    let mut overrides = HashMap::new();
    for language in languages {
        let Some(value) = vars(&format!("U50_STYLE_{}", language.env_var_key())) else {
            continue;
        };
        let argv: Vec<String> = value.split_whitespace().map(str::to_owned).collect();
        if argv.is_empty() {
            continue;
        }
        overrides.insert(language, argv);
    }
    overrides
}

impl Cs50Formatter {
    /// Builds a formatter with per-language override command lines; each
    /// command's source is piped via stdin.
    #[must_use]
    pub fn with_overrides(overrides: HashMap<Language, Vec<String>>) -> Self {
        Self { overrides }
    }

    /// Builds a formatter honoring the `U50_STYLE_<LANG>` environment
    /// variables.
    #[must_use]
    pub fn from_env() -> Self {
        Self::with_overrides(overrides_from_env(|var| std::env::var(var).ok()))
    }
}

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
        if let Some(argv) = self.overrides.get(&language) {
            return run_override(
                &format!("U50_STYLE_{}", language.env_var_key()),
                argv,
                source,
            );
        }
        match language {
            Language::C | Language::Cpp | Language::Java => {
                let assume = format!("--assume-filename={}", language.file_name());
                let style = format!("-style={CS50_CLANG_FORMAT_CONFIG}");
                run_tool("clang-format", &[assume.as_str(), style.as_str()], source)
            }
            Language::Python => run_tool(
                "autopep8",
                &["-", "--max-line-length=100", "--ignore-local-config"],
                source,
            ),
            Language::JavaScript => run_tool(
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
            ),
            Language::Html => run_tool_lenient("djhtml", &["-"], source),
            Language::Css => run_tool(
                "css-beautify",
                &["--indent-size", "4", "--end-with-newline", "-"],
                source,
            ),
            Language::Sql => {
                let mut formatted = run_tool(
                    "sqlformat",
                    &["-k", "upper", "-r", "--indent_width", "4", "-"],
                    source,
                )?;
                if !formatted.ends_with('\n') {
                    formatted.push('\n');
                }
                Ok(formatted)
            }
        }
    }
}
