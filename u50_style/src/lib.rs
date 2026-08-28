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
}

impl Language {
    /// Canonical file name used with `--assume-filename` so clang-format
    /// picks the right lexer for the language.
    #[must_use]
    fn file_name(self) -> &'static str {
        match self {
            Self::C => "foo.c",
            Self::Cpp => "foo.cpp",
            Self::Java => "foo.java",
        }
    }
}

/// Detects the language of `path` from its file extension
/// (c/h -> C, cpp/hpp/cc/cxx -> Cpp, java -> Java).
#[must_use]
pub fn detect_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "c" | "h" => Some(Language::C),
        "cpp" | "hpp" | "cc" | "cxx" => Some(Language::Cpp),
        "java" => Some(Language::Java),
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

/// Formatter backed by the external `clang-format` binary with CS50's style
/// configuration (the same approach the original style50 uses).
#[derive(Debug, Clone, Copy, Default)]
pub struct ClangFormat;

impl Formatter for ClangFormat {
    /// # Errors
    /// Returns an error when clang-format is missing or exits unsuccessfully.
    fn format(&self, source: &str, language: Language) -> anyhow::Result<String> {
        let mut child = Command::new("clang-format")
            .arg(format!("--assume-filename={}", language.file_name()))
            .arg(format!("-style={CS50_CLANG_FORMAT_CONFIG}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!("clang-format is required (>= 14) to check C/C++/Java style: {e}")
            })?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            anyhow::anyhow!("clang-format is required (>= 14) to check C/C++/Java style")
        })?;
        let source = source.to_owned();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(source.as_bytes());
        });
        let output = child.wait_with_output()?;
        let _ = writer.join();
        if !output.status.success() {
            anyhow::bail!(
                "clang-format failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

/// Runs the style check for `req` using the clang-format-backed formatter,
/// printing results (the only place this crate prints) and returning the
/// report so the caller can decide the exit code.
///
/// # Errors
/// Returns an error for unreadable files, unsupported file types, or
/// formatter failures.
pub fn run(req: &Request) -> anyhow::Result<Report> {
    tracing::debug!(?req, "u50_style::run");
    let report = run_with(req, &ClangFormat)?;
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

/// Like [`run`], but injects the formatter so tests can run without
/// clang-format installed. Builds no output for the caller; rendering lives
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
                 c, h, cpp, hpp, cc, cxx, java",
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
            ("a.py", None),
            ("a", None),
        ];
        for (name, expected) in cases {
            assert_eq!(detect_language(Path::new(name)), expected, "for {name}");
        }
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
        let path = temp_file("bad.py", "print(1)\n");
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
