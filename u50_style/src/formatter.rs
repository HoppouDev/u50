//! Formatter backend backed by external style tools.

use std::collections::HashMap;
use std::io::Write;
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

/// Runs `tool` with `args`, feeding `source` on stdin, and returns its
/// stdout. Writes stdin from a separate thread so a child that fills its
/// stdout pipe cannot deadlock against us still writing its stdin.
///
/// # Errors
/// Returns an error when the binary is missing (with the per-tool install
/// hint from [`missing_tool_message`]) or exits unsuccessfully.
/// Spawns `tool` with `args`, feeds `source` on stdin (written from a
/// separate thread so a child that fills its stdout pipe cannot deadlock
/// against us still writing its stdin), and waits for it to exit. Spawn
/// errors are mapped by `on_spawn` so callers can phrase the failure for
/// their context (built-in install hint vs. override env var).
fn run_process(
    tool: &str,
    args: &[&str],
    source: &str,
    on_spawn: impl Fn(std::io::Error) -> anyhow::Error,
) -> anyhow::Result<std::process::Output> {
    let mut child = Command::new(tool)
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

/// Formatter backed by the same per-language external formatters the
/// original style50 uses (`style50/languages.py`): clang-format for
/// C/C++/Java, autopep8 for Python, and js-beautify for JavaScript. The
/// original calls the Python libraries directly (`autopep8`,
/// `jsbeautifier`); u50 shells out to the corresponding pip-installed
/// CLIs, which apply the same defaults. Exact options passed (flag names
/// verified against the installed CLIs; they mirror the original's
/// library options):
///
/// - Python: `autopep8 - --max-line-length=100 --ignore-local-config`
/// - JavaScript: `js-beautify --end-with-newline --operator-position preserve-newline -w 100 --brace-style collapse,preserve-inline --keep-array-indentation -` — the short `-w 100` form is required because this CLI build declares the long `--wrap-line-length` as taking no argument, and the `-` stdin marker must come last because the CLI stops parsing options at the first positional
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
        // The original style50 library calls leave empty and whitespace-only
        // files untouched (e.g. `autopep8.format_code("") == ""`), so no
        // formatter — built-in or override — is invoked and the file is
        // reported clean.
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
        }
    }
}
