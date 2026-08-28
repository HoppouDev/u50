#![warn(clippy::pedantic)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use similar::{ChangeTag, DiffTag, TextDiff};

/// The clang-format style configuration CS50 uses for its style checks
/// (recorded verbatim from the original `style50` source).
const CS50_CLANG_FORMAT_CONFIG: &str = "{ \
AllowShortFunctionsOnASingleLine: Empty, \
BraceWrapping: { AfterCaseLabel: true, AfterControlStatement: true, \
AfterFunction: true, AfterStruct: true, BeforeElse: true, BeforeWhile: true }, \
BreakBeforeBraces: Custom, ColumnLimit: 100, IndentCaseLabels: true, \
IndentWidth: 4, SpaceAfterCStyleCast: true, TabWidth: 4 }";

/// Context radius passed to `TextDiff::grouped_ops` to keep every change in
/// a single group. Must satisfy `n * 2 <= usize::MAX` (see
/// `similar::common::group_diff_ops`); `usize::MAX` itself would overflow.
const ALL_IN_ONE_GROUP: usize = usize::MAX / 2;

const RED: &str = "\u{1b}[31m";
const GREEN: &str = "\u{1b}[32m";
const BOLD: &str = "\u{1b}[1m";
const RESET: &str = "\u{1b}[0m";

/// A language whose style can be checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// C (`.c`, `.h`).
    C,
    /// C++ (`.cpp`, `.hpp`, `.cc`, `.cxx`).
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
    /// Canonical file name used with `--assume-filename` so clang-format
    /// picks the right lexer for the language (only meaningful for the
    /// clang-format-backed languages).
    #[must_use]
    fn file_name(self) -> &'static str {
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
    pub fn required_tool(self) -> &'static str {
        match self {
            Self::C | Self::Cpp | Self::Java => "clang-format",
            Self::Python => "autopep8",
            Self::JavaScript => "js-beautify",
            Self::Html => "djhtml",
            Self::Css => "css-beautify",
            Self::Sql => "sqlformat",
        }
    }
}

/// Detects the language of `path` from its file extension
/// (c/h -> C, cpp/hpp/cc/cxx -> Cpp, java -> Java, py -> Python,
/// js -> JavaScript, html -> Html, css -> Css, sql -> Sql).
#[must_use]
pub fn detect_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "c" | "h" => Some(Language::C),
        "cpp" | "hpp" | "cc" | "cxx" => Some(Language::Cpp),
        "java" => Some(Language::Java),
        "py" => Some(Language::Python),
        "js" => Some(Language::JavaScript),
        "html" => Some(Language::Html),
        "css" => Some(Language::Css),
        "sql" => Some(Language::Sql),
        _ => None,
    }
}

/// Diff output format for `u50 style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// One-character-per-line diff style (style50 default).
    Character,
    /// Side-by-side split diff.
    Split,
    /// Unified diff.
    Unified,
    /// Machine-readable JSON (also present in the original via `-o json`).
    Json,
}

/// Parameters for a `u50 style` invocation.
#[derive(Debug, Clone)]
pub struct Request {
    /// Files to style-check.
    pub files: Vec<PathBuf>,
    /// Diff output format.
    pub output: Output,
    /// Whether ANSI colors are enabled for text output modes (JSON output
    /// is never colored).
    pub color: bool,
}

/// Styles one file's source.
pub trait Formatter {
    /// Formats `source` per CS50 style.
    ///
    /// # Errors
    /// Returns an error when the external formatter fails.
    fn format(&self, source: &str, language: Language) -> anyhow::Result<String>;
}

/// The actionable message shown when the formatter binary `tool` is
/// missing (per language, with an install hint).
fn missing_tool_message(tool: &str) -> String {
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

/// Runs `tool` with `args`, feeding `source` on stdin, and returns its
/// stdout. Writes stdin from a separate thread so a child that fills its
/// stdout pipe cannot deadlock against us still writing its stdin.
///
/// # Errors
/// Returns an error when the binary is missing (with the per-tool install
/// hint from [`missing_tool_message`]) or exits unsuccessfully.
fn run_tool(tool: &str, args: &[&str], source: &str) -> anyhow::Result<String> {
    let mut child = Command::new(tool)
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
    if !output.status.success() {
        anyhow::bail!(
            "`{tool}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Like [`run_tool`], but tolerant of exit code 1: `djhtml` follows the
/// `diff`/`black` convention and exits 1 when it reformats the input.
/// Success = exit 0 (any stdout), or exit 1 with non-empty stdout.
/// Anything else — including exit 1 with empty stdout — is an error.
///
/// # Errors
/// Returns an error when the binary is missing (with the per-tool install
/// hint from [`missing_tool_message`]) or fails the convention above.
fn run_tool_lenient(tool: &str, args: &[&str], source: &str) -> anyhow::Result<String> {
    let mut child = Command::new(tool)
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
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let ok = match output.status.code() {
        Some(0) => true,
        Some(1) => !stdout.is_empty(),
        _ => false,
    };
    if ok {
        Ok(stdout)
    } else {
        anyhow::bail!(
            "`{tool}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}

/// Formatter backed by the same per-language external formatters the
/// original style50 uses (`style50/languages.py`): clang-format for
/// C/C++/Java, autopep8 for Python, js-beautify for JavaScript, djhtml for
/// HTML, css-beautify for CSS, and sqlformat for SQL. The original calls
/// the Python libraries directly (`autopep8`, `jsbeautifier`,
/// `cssbeautifier`, `sqlparse`; `djhtml` as a process); u50 shells out to
/// the corresponding pip-installed CLIs, which apply the same defaults.
/// Exact options passed (flag names verified against the installed CLIs;
/// they mirror the original's library options):
///
/// - Python: `autopep8 - --max-line-length=100 --ignore-local-config`
/// - JavaScript: `js-beautify --end-with-newline --operator-position preserve-newline -w 100 --brace-style collapse,preserve-inline --keep-array-indentation -` — the short `-w 100` form is required because this CLI build declares the long `--wrap-line-length` as taking no argument, and the `-` stdin marker must come last because the CLI stops parsing options at the first positional
/// - HTML: `djhtml -` (exit 1 on successful reformat is treated as
///   success, per the diff/black convention; only exit > 1, or exit 1
///   with empty stdout, is an error)
/// - CSS: `css-beautify --indent-size 4 --end-with-newline -` (the `-`
///   stdin marker must come last, as with js-beautify)
/// - SQL: `sqlformat --reindent --keywords upper --indent_width 4 -` (the
///   CLI takes a single FILE positional, `-` = stdin, and writes to
///   stdout; unlike the `sqlparse` library it does not end output with a
///   newline, so a trailing newline is appended when missing — matching
///   the original's SQL class)
#[derive(Debug, Clone, Copy, Default)]
pub struct Cs50Formatter;

impl Formatter for Cs50Formatter {
    /// # Errors
    /// Returns an error when the language's formatter is missing or exits
    /// unsuccessfully.
    fn format(&self, source: &str, language: Language) -> anyhow::Result<String> {
        // The original style50 library calls leave empty and whitespace-only
        // files untouched (e.g. `autopep8.format_code("") == ""`), so no
        // external formatter is invoked and the file is reported clean.
        if source.trim().is_empty() {
            return Ok(source.to_owned());
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
                    &[
                        "--reindent",
                        "--keywords",
                        "upper",
                        "--indent_width",
                        "4",
                        "-",
                    ],
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

/// The style-check outcome for a single file.
#[derive(Debug, Clone)]
pub struct FileResult {
    /// The checked file.
    pub path: PathBuf,
    /// Whether the file already conforms to CS50 style.
    pub clean: bool,
    /// Human-readable per-file output for text modes (or the unified patch
    /// in JSON mode); `None` when the file is clean.
    pub rendered: Option<String>,
}

/// The aggregated style-check outcome for a request.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// One entry per requested file.
    pub results: Vec<FileResult>,
}

impl Report {
    /// Whether every requested file is clean (true for an empty request).
    #[must_use]
    pub fn clean(&self) -> bool {
        self.results.iter().all(|r| r.clean)
    }
}

/// Runs the style check for `req` using the CS50 formatter stack
/// printing results (the only place this crate prints) and returning the
/// report so the caller can decide the exit code.
///
/// # Errors
/// Returns an error for unreadable files, unsupported file types, or
/// formatter failures.
pub fn run(req: &Request) -> anyhow::Result<Report> {
    tracing::debug!(?req, "u50_style::run");
    let report = run_with(req, &Cs50Formatter)?;
    if req.output == Output::Json {
        println!("{}", json_document(&report));
    } else {
        for result in &report.results {
            if let Some(rendered) = &result.rendered {
                print!("{rendered}");
            }
        }
    }
    Ok(report)
}

/// Like [`run`], but injects the formatter so tests can run without the
/// external formatter binaries installed. Builds no output for the caller; rendering lives
/// on the [`FileResult`]s.
///
/// # Errors
/// Returns an error for unreadable files, unsupported file types, or
/// formatter failures.
pub fn run_with(req: &Request, formatter: &dyn Formatter) -> anyhow::Result<Report> {
    let mut results = Vec::with_capacity(req.files.len());
    for path in &req.files {
        let source = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("could not read `{}`: {e}", path.display()))?;
        let Some(language) = detect_language(path) else {
            anyhow::bail!(
                "unsupported file type `{}`; supported extensions: \
                 c, h, cpp, hpp, cc, cxx, java, py, js, html, css, sql",
                path.display()
            );
        };
        let expected = formatter.format(&source, language)?;
        let clean = source == expected;
        let rendered = if clean {
            None
        } else {
            Some(match req.output {
                Output::Character => render_character(&source, &expected, req.color),
                Output::Split => render_split(&source, &expected, req.color),
                Output::Unified | Output::Json => render_unified(&source, &expected, path),
            })
        };
        results.push(FileResult {
            path: path.clone(),
            clean,
            rendered,
        });
    }
    Ok(Report { results })
}

/// Builds the single JSON document printed in JSON mode.
fn json_document(report: &Report) -> serde_json::Value {
    serde_json::json!({
        "clean": report.clean(),
        "files": report
            .results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "path": r.path.display().to_string(),
                    "clean": r.clean,
                    "patch": r.rendered,
                })
            })
            .collect::<Vec<serde_json::Value>>(),
    })
}

fn trim_line(value: &str) -> String {
    value.trim_end_matches(['\r', '\n']).to_owned()
}

/// Character mode: per-line diff with inline (character-level) emphasis on
/// changed spans.
fn render_character(source: &str, formatted: &str, color: bool) -> String {
    let diff = TextDiff::from_lines(source, formatted);
    let mut out = String::new();
    for group in &diff.grouped_ops(ALL_IN_ONE_GROUP) {
        for op in group {
            for change in diff.iter_inline_changes(op) {
                let code = match change.tag() {
                    ChangeTag::Equal => None,
                    ChangeTag::Delete => Some(RED),
                    ChangeTag::Insert => Some(GREEN),
                };
                let colored = color && code.is_some();
                if colored {
                    out.push_str(code.unwrap_or(""));
                }
                out.push(match change.tag() {
                    ChangeTag::Equal => ' ',
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                });
                for (emphasized, value) in change.values() {
                    if *emphasized && colored {
                        out.push_str(BOLD);
                    }
                    out.push_str(value.trim_end_matches(['\r', '\n']));
                    if *emphasized && colored {
                        out.push_str(RESET);
                    }
                }
                if colored {
                    out.push_str(RESET);
                }
                out.push('\n');
            }
        }
    }
    out
}

/// Split mode: side-by-side columns of width 50 separated by ` | `.
fn render_split(source: &str, formatted: &str, color: bool) -> String {
    let diff = TextDiff::from_lines(source, formatted);
    let mut out = String::new();
    let mut dels: Vec<String> = Vec::new();
    let mut adds: Vec<String> = Vec::new();
    for group in &diff.grouped_ops(ALL_IN_ONE_GROUP) {
        for op in group {
            if op.tag() == DiffTag::Equal {
                flush_split_rows(&mut out, &mut dels, &mut adds, color);
                for change in diff.iter_changes(op) {
                    let line = trim_line(change.value());
                    out.push_str(&split_row(&line, false, &line, false, color));
                }
            } else {
                for change in diff.iter_changes(op) {
                    match change.tag() {
                        ChangeTag::Delete => dels.push(trim_line(change.value())),
                        ChangeTag::Insert => adds.push(trim_line(change.value())),
                        ChangeTag::Equal => {}
                    }
                }
            }
        }
        flush_split_rows(&mut out, &mut dels, &mut adds, color);
    }
    out
}

/// Pairs buffered deletions with insertions, padding the shorter side.
fn flush_split_rows(out: &mut String, dels: &mut Vec<String>, adds: &mut Vec<String>, color: bool) {
    for i in 0..dels.len().max(adds.len()) {
        let left = dels.get(i).map_or(String::new(), Clone::clone);
        let right = adds.get(i).map_or(String::new(), Clone::clone);
        out.push_str(&split_row(
            &left,
            i < dels.len(),
            &right,
            i < adds.len(),
            color,
        ));
    }
    dels.clear();
    adds.clear();
}

fn split_row(left: &str, deleted: bool, right: &str, inserted: bool, color: bool) -> String {
    const WIDTH: usize = 50;
    let mut l = left.chars().take(WIDTH).collect::<String>();
    let mut r = right.chars().take(WIDTH).collect::<String>();
    for _ in l.chars().count()..WIDTH {
        l.push(' ');
    }
    for _ in r.chars().count()..WIDTH {
        r.push(' ');
    }
    if color && deleted {
        l = format!("{RED}{l}{RESET}");
    }
    if color && inserted {
        r = format!("{GREEN}{r}{RESET}");
    }
    format!("{l} | {r}\n")
}

/// Unified mode: `git diff`-style output.
fn render_unified(source: &str, formatted: &str, path: &Path) -> String {
    let name = path.display().to_string();
    TextDiff::from_lines(source, formatted)
        .unified_diff()
        .context_radius(3)
        .header(&name, &name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Formatter that leaves the source untouched (models a clean file).
    struct Identity;

    impl Formatter for Identity {
        fn format(&self, source: &str, _language: Language) -> anyhow::Result<String> {
            Ok(source.to_owned())
        }
    }

    /// Formatter that re-indents every non-empty line (models a dirty file).
    struct Reindent;

    impl Formatter for Reindent {
        fn format(&self, source: &str, _language: Language) -> anyhow::Result<String> {
            let mut out = String::new();
            for line in source.lines() {
                if !line.is_empty() {
                    out.push_str("    ");
                }
                out.push_str(line);
                out.push('\n');
            }
            Ok(out)
        }
    }

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("u50_style_test_{}_{name}", std::process::id()));
        std::fs::write(&path, contents).expect("write temp file");
        path
    }

    #[test]
    fn detect_language_maps_extensions() {
        let cases = [
            ("a.c", Some(Language::C)),
            ("a.h", Some(Language::C)),
            ("a.cpp", Some(Language::Cpp)),
            ("a.hpp", Some(Language::Cpp)),
            ("a.cc", Some(Language::Cpp)),
            ("a.cxx", Some(Language::Cpp)),
            ("a.java", Some(Language::Java)),
            ("a.py", Some(Language::Python)),
            ("a.js", Some(Language::JavaScript)),
            ("a.html", Some(Language::Html)),
            ("a.css", Some(Language::Css)),
            ("a.sql", Some(Language::Sql)),
            ("a", None),
        ];
        for (name, expected) in cases {
            assert_eq!(detect_language(Path::new(name)), expected, "for {name}");
        }
    }

    #[test]
    fn required_tool_maps_every_language() {
        let cases = [
            (Language::C, "clang-format"),
            (Language::Cpp, "clang-format"),
            (Language::Java, "clang-format"),
            (Language::Python, "autopep8"),
            (Language::JavaScript, "js-beautify"),
            (Language::Html, "djhtml"),
            (Language::Css, "css-beautify"),
            (Language::Sql, "sqlformat"),
        ];
        for (language, tool) in cases {
            assert_eq!(language.required_tool(), tool, "for {language:?}");
        }
    }

    #[test]
    fn run_tool_missing_binary_names_the_tool() {
        let err = run_tool("definitely-not-a-real-u50-tool", &[], "x").expect_err("errors");
        assert!(err.to_string().contains("definitely-not-a-real-u50-tool"));
        assert!(err.to_string().contains("is required"));
    }

    #[test]
    fn clean_file_is_reported_clean() {
        let path = temp_file("clean.c", "int main(void)\n{\n    return 0;\n}\n");
        let req = Request {
            files: vec![path.clone()],
            output: Output::Unified,
            color: false,
        };
        let report = run_with(&req, &Identity).expect("run succeeds");
        assert!(report.clean());
        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].clean);
        assert!(report.results[0].rendered.is_none());
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn dirty_file_unified_has_plus_and_minus_lines() {
        let path = temp_file("dirty.c", "int main(void)\n{\nreturn 0;\n}\n");
        let req = Request {
            files: vec![path.clone()],
            output: Output::Unified,
            color: false,
        };
        let report = run_with(&req, &Reindent).expect("run succeeds");
        assert!(!report.clean());
        let rendered = report.results[0].rendered.clone().expect("rendered");
        assert!(rendered.lines().any(|l| l.starts_with('-')));
        assert!(rendered.lines().any(|l| l.starts_with('+')));
        assert!(rendered.contains(path.to_str().expect("utf8 path")));
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn character_output_has_plus_and_minus_lines() {
        let path = temp_file("char.c", "int main(void)\n{\nreturn 0;\n}\n");
        let req = Request {
            files: vec![path.clone()],
            output: Output::Character,
            color: false,
        };
        let report = run_with(&req, &Reindent).expect("run succeeds");
        let rendered = report.results[0].rendered.clone().expect("rendered");
        assert!(rendered.lines().any(|l| l.starts_with('-')));
        assert!(rendered.lines().any(|l| l.starts_with('+')));
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn split_output_has_column_separator() {
        let path = temp_file("split.c", "int main(void)\n{\nreturn 0;\n}\n");
        let req = Request {
            files: vec![path.clone()],
            output: Output::Split,
            color: false,
        };
        let report = run_with(&req, &Reindent).expect("run succeeds");
        let rendered = report.results[0].rendered.clone().expect("rendered");
        assert!(rendered.contains(" | "));
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn json_document_parses_with_expected_fields() {
        let path = temp_file("json.c", "int main(void)\n{\nreturn 0;\n}\n");
        let req = Request {
            files: vec![path.clone()],
            output: Output::Json,
            color: false,
        };
        let report = run_with(&req, &Reindent).expect("run succeeds");
        let doc = json_document(&report);
        assert_eq!(doc["clean"], serde_json::Value::Bool(false));
        assert_eq!(doc["files"][0]["path"], path.display().to_string());
        assert_eq!(doc["files"][0]["clean"], serde_json::Value::Bool(false));
        assert!(doc["files"][0]["patch"].is_string());
        let text = serde_json::to_string(&doc).expect("serialize");
        serde_json::from_str::<serde_json::Value>(&text).expect("valid json");
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn json_document_clean_file_has_null_patch() {
        let path = temp_file("jsonclean.c", "int main(void)\n{\n    return 0;\n}\n");
        let req = Request {
            files: vec![path.clone()],
            output: Output::Json,
            color: false,
        };
        let report = run_with(&req, &Identity).expect("run succeeds");
        let doc = json_document(&report);
        assert_eq!(doc["clean"], serde_json::Value::Bool(true));
        assert!(doc["files"][0]["patch"].is_null());
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn formatter_short_circuits_on_empty_and_whitespace_only_source() {
        for language in [Language::JavaScript, Language::Css] {
            assert_eq!(Cs50Formatter.format("", language).expect("ok"), "");
            assert_eq!(Cs50Formatter.format("\n  ", language).expect("ok"), "\n  ");
        }
    }

    #[test]
    fn run_with_empty_js_file_is_clean() {
        let path = temp_file("empty.js", "");
        let req = Request {
            files: vec![path.clone()],
            output: Output::Unified,
            color: false,
        };
        let report = run_with(&req, &Cs50Formatter).expect("run succeeds");
        assert!(report.clean());
        assert!(report.results[0].rendered.is_none());
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn empty_request_is_clean() {
        let req = Request {
            files: vec![],
            output: Output::Unified,
            color: false,
        };
        let report = run_with(&req, &Identity).expect("run succeeds");
        assert!(report.clean());
        assert!(report.results.is_empty());
        assert_eq!(json_document(&report)["files"], serde_json::json!([]));
    }

    #[test]
    fn unsupported_extension_errors_with_path() {
        let path = temp_file("bad.rb", "puts 1\n");
        let req = Request {
            files: vec![path.clone()],
            output: Output::Unified,
            color: false,
        };
        let err = run_with(&req, &Identity).expect_err("errors");
        assert!(err.to_string().contains("unsupported file type"));
        assert!(err.to_string().contains(&path.display().to_string()));
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn missing_file_errors_with_path() {
        let path =
            std::env::temp_dir().join(format!("u50_style_test_{}_missing.c", std::process::id()));
        let req = Request {
            files: vec![path.clone()],
            output: Output::Unified,
            color: false,
        };
        let err = run_with(&req, &Identity).expect_err("errors");
        assert!(err.to_string().contains("could not read"));
        assert!(err.to_string().contains(&path.display().to_string()));
    }
}
