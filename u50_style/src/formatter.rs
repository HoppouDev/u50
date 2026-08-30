//! Formatter backend backed by external style tools.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

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

/// Where a tool command came from: an explicit path, or u50's cache
/// (installed by `u50 --setup` or auto-provisioned on first use).
///
/// u50 NEVER resolves its BUILT-IN formatter tools through the system
/// `PATH`: bare tool names are looked up in the cache only, and missing
/// backends are downloaded into it on first use. [`ToolOrigin::Path`]
/// therefore only ever applies to explicit user-provided paths (see
/// [`is_explicit_path`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOrigin {
    /// An explicit path command (see [`is_explicit_path`]), used as-is.
    Path,
    /// Found in the u50 style cache (`~/.cache/u50/style50`).
    Cache,
}

/// The u50 style cache root: the absolute `$XDG_CACHE_HOME` override
/// when set (all platforms), else the platform cache base
/// ([`cache_base`]), then `u50/style50`.
///
/// # Errors
/// Returns an error when no cache base is determinable: no absolute
/// `$XDG_CACHE_HOME`, and no `$HOME` on unix or `%LOCALAPPDATA%` /
/// `%USERPROFILE%` on Windows. u50 never falls back to a relative
/// `.cache`, which would silently scatter the cache across working
/// directories.
pub(crate) fn cache_dir() -> anyhow::Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(xdg.join("u50").join("style50"));
    }
    let base = cache_base().ok_or_else(|| {
        anyhow::anyhow!(if cfg!(windows) {
            "cannot determine the u50 style cache directory: set \
             %LOCALAPPDATA% or %USERPROFILE% (or an absolute \
             $XDG_CACHE_HOME)"
        } else {
            "cannot determine the u50 style cache directory: set \
             $HOME (or an absolute $XDG_CACHE_HOME)"
        })
    })?;
    Ok(base.join("u50").join("style50"))
}

/// The platform cache base (after the `$XDG_CACHE_HOME` override):
/// `$HOME/.cache` on unix, `%LOCALAPPDATA%` (or
/// `%USERPROFILE%\AppData\Local`) on Windows.
#[cfg(unix)]
fn cache_base() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache"))
}

#[cfg(windows)]
fn cache_base() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|profile| profile.join("AppData").join("Local"))
        })
}

/// The `bin` directory of a uv-managed venv: `Scripts` on Windows
/// (where console scripts are installed as `.exe` shims), `bin`
/// elsewhere (POSIX shebang scripts).
#[must_use]
pub(crate) fn venv_bin_dir(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    }
}

/// The file name the console script for `tool` is installed under in
/// the venv bin dir: `tool.exe` on Windows, `tool` elsewhere.
#[must_use]
pub(crate) fn tool_file_name(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_owned()
    }
}

/// The directory holding binaries installed by `u50 --setup` (the
/// uv-managed venv `<cache>/venv` puts console scripts in `bin/` —
/// `Scripts\` with `.exe` shims on Windows; see [`venv_bin_dir`]).
///
/// # Errors
/// Propagates [`cache_dir`] failures.
pub(crate) fn cache_bin_dir() -> anyhow::Result<PathBuf> {
    Ok(venv_bin_dir(&cache_dir()?.join("venv")))
}

/// Whether `path` is an existing regular file usable as a formatter
/// tool: on unix it must carry an execute bit; Windows has no exec-bit
/// model, so any existing regular file qualifies.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Whether `tool` names an explicit path rather than a bare tool name:
/// true when it contains either path separator, or parses as a path
/// with a root or non-empty parent component (drive prefix, `..`, a
/// subdirectory). Bare names (`clang-format`) stay cache-only on all
/// platforms, so a hostile or unrelated same-named binary on `PATH`
/// can never be picked up.
fn is_explicit_path(tool: &str) -> bool {
    if tool.contains('/') || tool.contains('\\') {
        return true;
    }
    let path = Path::new(tool);
    path.has_root()
        || path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
}

/// Resolves `tool` to its location, cache-only: an explicit path (see
/// [`is_explicit_path`]) is used as-is ([`ToolOrigin::Path`]); a bare
/// tool name is looked up ONLY in the u50 style cache bin dir (the
/// `u50 --setup` / lazy auto-provision install location, with the
/// platform console-script file name, see [`tool_file_name`]) — the
/// system `PATH` is never consulted. Returns `None`
/// when the tool is not in the cache (the caller may then auto-provision
/// it; see [`Cs50Formatter::format`]) or when the cache directory
/// cannot be determined ([`cache_dir`]).
#[must_use]
pub fn locate_tool(tool: &str) -> Option<(PathBuf, ToolOrigin)> {
    if is_explicit_path(tool) {
        return Some((PathBuf::from(tool), ToolOrigin::Path));
    }
    let cached = cache_bin_dir().ok()?.join(tool_file_name(tool));
    if is_executable_file(&cached) {
        return Some((cached, ToolOrigin::Cache));
    }
    None
}

/// Resolves `tool` cache-only and spawns it with `args`, feeding `source`
/// on stdin (written from a separate thread so a child that fills its
/// stdout pipe cannot deadlock against us still writing its stdin), and
/// waits for it to exit.
///
/// Cache-only spawn guard for BUILT-IN tools: never let the OS resolve a
/// bare formatter tool through `PATH`. `locate_tool` runs exactly once
/// here and the resolved path is handed straight to the spawn (no second
/// lookup). No env fixup is needed: the venv console scripts installed by
/// `--setup` carry absolute shebangs and are self-contained.
///
/// # Errors
/// Returns an error when the tool is not in the cache (with the standard
/// missing-tool message) and any error while attaching stdin or waiting
/// on the child.
fn spawn_tool(tool: &str, args: &[&str], source: &str) -> anyhow::Result<std::process::Output> {
    let resolved = locate_tool(tool).map(|(path, _)| path).ok_or_else(|| {
        anyhow::anyhow!(
            "{} (not found in the u50 style cache)",
            missing_tool_message(tool)
        )
    })?;
    let mut command = Command::new(&resolved);
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("{}: {e}", missing_tool_message(tool)))?;
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

pub(crate) fn run_tool(tool: &str, args: &[&str], source: &str) -> anyhow::Result<String> {
    let output = spawn_tool(tool, args, source)?;
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
    let output = spawn_tool(tool, args, source)?;
    let reformatted_on_exit_1 = output.status.code() == Some(1) && !output.stdout.is_empty();
    if !output.status.success() && !reformatted_on_exit_1 {
        anyhow::bail!(
            "`{tool}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Tools whose lazy auto-provisioning was already attempted in this
/// process (see [`ensure_backend_once`]). The first missing-tool
/// occurrence per run triggers provisioning; later files in the same run
/// skip straight to the missing-tool error when the first attempt
/// failed. When an attempt succeeded, [`locate_tool`] finds the tool and
/// the dedupe never matters.
static PROVISION_ATTEMPTED: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Attempts to lazily auto-provision `tool` into the u50 style cache
/// exactly once per process: downloads it via the same uv library path
/// `u50 --setup` uses (never through the formatter, so there is no
/// recursion) and lets the caller's subsequent spawn fail naturally when
/// provisioning did not help. Set `U50_STYLE_NO_PROVISION` in the
/// environment to disable (used by hermetic tests).
fn ensure_backend_once(tool: &str) {
    if std::env::var_os("U50_STYLE_NO_PROVISION").is_some() {
        return;
    }
    let mut attempted = PROVISION_ATTEMPTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !attempted.insert(tool.to_owned()) {
        return;
    }
    drop(attempted);
    tracing::info!(
        tool,
        "formatter backend missing from the cache; auto-provisioning"
    );
    if let Err(e) = crate::setup::ensure_backend(tool) {
        tracing::warn!(tool, error = %e, "auto-provisioning failed");
    }
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
        // failed attempt is only warned about — the `run_tool` call below
        // then produces the usual per-file missing-tool error.
        if let Some(tool) = language.required_tool()
            && locate_tool(tool).is_none()
        {
            ensure_backend_once(tool);
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
