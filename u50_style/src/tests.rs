use std::path::{Path, PathBuf};

use super::*;
use crate::engine::expand_paths;

/// Builds `n` distinct lines `prefix 0..n`, one per line (test input helper).
fn numbered_lines(prefix: &str, n: usize) -> String {
    let mut out = String::with_capacity(n * 12);
    for i in 0..n {
        out.push_str(prefix);
        out.push_str(&i.to_string());
        out.push('\n');
    }
    out
}
use crate::formatter::{cache_bin_dir, cache_dir, locate_tool, run_tool, venv_bin_dir};
use crate::language::{Language, comment_hint, count_comments};
use crate::render::{
    bold, bright_white, cyan, green, json_document, on_green, on_red, red, render_character,
    render_split, render_unified, reset, select_algorithm, yellow,
};
use crate::renderer::HEADER_RULE;
use similar::algorithms::Algorithm;

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

/// Formatter that rstrips every line and ensures a trailing newline
/// (models a formatter whose output equals style50 3.0.0's normalized
/// input, e.g. a tool that only strips trailing whitespace).
struct Rstrip;

/// A dirty file whose in-place write fails (read-only permissions) must
/// be reported as `could not write` â€” and the ORIGINAL must survive
/// byte-for-byte: the styled content is written to a sibling temp file
/// and renamed, so a failing write never truncates the target.
#[test]
#[cfg(unix)]
fn fix_with_records_write_failures_and_keeps_the_original() {
    use std::os::unix::fs::PermissionsExt;
    let root = temp_dir("fixwrite");
    let c = write_in(&root, "dirty.c", DIRTY_C);
    std::fs::set_permissions(&c, std::fs::Permissions::from_mode(0o444)).expect("chmod fixture");
    let report = fix_with(&fix_request(vec![c.clone()]), &Reindent, false);
    assert_eq!(report.errors.len(), 1, "write failure recorded: {report:?}");
    assert!(
        report.errors[0]
            .1
            .starts_with(&format!("could not write `{}`", c.display())),
        "unexpected error: {:?}",
        report.errors[0]
    );
    // The original is byte-for-byte intact â€” never truncated.
    assert_eq!(std::fs::read_to_string(&c).expect("read"), DIRTY_C);
    assert!(report.results.is_empty());
    // No temp sibling is left behind.
    assert!(!root.join("dirty.c.u50-tmp").exists());
    std::fs::set_permissions(&c, std::fs::Permissions::from_mode(0o644)).expect("restore perms");
    std::fs::remove_dir_all(&root).expect("cleanup");
}

impl Formatter for Rstrip {
    fn format(&self, source: &str, _language: Language) -> anyhow::Result<String> {
        let mut out = source
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        if !out.ends_with('\n') {
            out.push('\n');
        }
        Ok(out)
    }
}

/// Formatter that fixes only the `retrun` typo (models a tool whose
/// output differs from its input on exactly one line).
struct FixTy;

impl Formatter for FixTy {
    fn format(&self, source: &str, _language: Language) -> anyhow::Result<String> {
        Ok(source.replace("retrun", "return"))
    }
}

/// Formatter that always fails (models a broken external tool).
struct Failing;

impl Formatter for Failing {
    fn format(&self, _source: &str, _language: Language) -> anyhow::Result<String> {
        anyhow::bail!("boom: formatter exploded")
    }
}

/// In-memory `Write` sink shared with a renderer (`Rc` clone, then read
/// back after the renderer is dropped; `Box<dyn Write>` requires
/// `'static`).
#[derive(Default, Clone)]
struct SharedBuf(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

impl std::io::Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Renders a single result through the built-in renderer into a string
/// (test helper; replaces the removed per-result `rendered` field).
fn render_result(result: &FileResult, output: Output, color: bool) -> String {
    let sink = SharedBuf::default();
    let mut renderer = builtin_renderer(output, color, Box::new(sink.clone()));
    renderer.file(result);
    String::from_utf8(sink.0.borrow().clone()).expect("utf8 rendered output")
}

/// Drives `builtin_renderer(Output::Score, ...)` over the given results
/// and errors and returns the rendered bytes (score-mode test helper).
fn score_output(results: Vec<FileResult>, errors: Vec<(PathBuf, String)>, color: bool) -> String {
    let sink = SharedBuf::default();
    {
        let mut renderer = builtin_renderer(Output::Score, color, Box::new(sink.clone()));
        for result in &results {
            renderer.file(result);
        }
        for (path, message) in &errors {
            renderer.file_error(path, message);
        }
        renderer.finish(&Report { results, errors });
    }
    String::from_utf8(sink.0.borrow().clone()).expect("utf8")
}

fn fix_request(files: Vec<PathBuf>) -> Request {
    Request {
        files,
        output: Output::Unified,
        color: false,
    }
}

fn temp_file(name: &str, contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("u50_style_test_{}_{name}", std::process::id()));
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
        ("a.cc", None),
        ("a.cxx", None),
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
        (Language::C, Some("clang-format")),
        (Language::Cpp, Some("clang-format")),
        (Language::Java, Some("clang-format")),
        (Language::Python, Some("autopep8")),
        (Language::JavaScript, Some("js-beautify")),
        (Language::Html, Some("djhtml")),
        (Language::Css, Some("css-beautify")),
        (Language::Sql, Some("sqlformat")),
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
    let report = run_with(&req, &Identity);
    assert!(report.clean());
    assert!(!report.has_errors());
    assert_eq!(report.results.len(), 1);
    assert!(report.results[0].clean);
    let result = &report.results[0];
    assert_eq!(result.source, result.formatted, "clean: styled == source");
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
    let report = run_with(&req, &Reindent);
    assert!(!report.clean());
    let rendered = render_result(&report.results[0], Output::Unified, false);
    assert!(rendered.lines().any(|l| l.starts_with('-')));
    assert!(rendered.lines().any(|l| l.starts_with('+')));
    assert!(rendered.contains(path.to_str().expect("utf8 path")));
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn character_output_renders_the_original_text_without_plus_minus_lines() {
    let path = temp_file("char.c", "int main(void)\n{\nreturn 0;\n}\n");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Character,
        color: false,
    };
    let report = run_with(&req, &Reindent);
    let rendered = render_result(&report.results[0], Output::Character, false);
    // style50 parity: a leading blank line, then the original text
    // re-rendered with the styled spans inlined (here the changes are pure
    // 4-space insertions, so the body IS the styled text), then a trailing
    // blank line, then the comments hint (a 0-comment file has ratio
    // 0 < 0.10, oracle-verified) and its trailing blank line. Character
    // mode has no +/- rows.
    assert_eq!(
        rendered,
        "\n    int main(void)\n    {\n    return 0;\n    }\n\nAnd consider adding more comments!\n\n"
    );
    assert!(
        !rendered
            .lines()
            .any(|l| l.starts_with('+') || l.starts_with('-'))
    );
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn character_output_escapes_newline_and_tab_markers_with_legend() {
    // A deleted newline merges the two lines it joined and contributes a
    // legend line (marker escaped, style50's wording).
    assert_eq!(
        render_character("a\nb\n", "ab\n", false, false),
        "\na\\nb\n\n\\n means that you should delete a newline.\n\n"
    );
    // An inserted newline ends the visible line where it is inserted.
    assert_eq!(
        render_character("ab\n", "a\nb\n", false, false),
        "\na\\n\nb\n\n\\n means that you should insert a newline.\n\n"
    );
    // An inserted tab renders as the escaped marker plus its legend line.
    assert_eq!(
        render_character("x\n", "\tx\n", false, false),
        "\n\\tx\n\n\\t means that you should insert a tab.\n\n"
    );
}

#[test]
fn split_output_has_column_separator() {
    let path = temp_file("split.c", "int main(void)\n{\nreturn 0;\n}\n");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Split,
        color: false,
    };
    let report = run_with(&req, &Reindent);
    let rendered = render_result(&report.results[0], Output::Split, false);
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
    let report = run_with(&req, &Reindent);
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
    let report = run_with(&req, &Identity);
    let doc = json_document(&report);
    assert_eq!(doc["clean"], serde_json::Value::Bool(true));
    assert!(doc["files"][0]["patch"].is_null());
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn json_document_multi_file_mixed_clean_and_dirty() {
    let dirty = FileResult {
        path: PathBuf::from("dirty.c"),
        clean: false,
        source: Some("return 0;\n".to_owned()),
        formatted: Some("    return 0;\n".to_owned()),
    };
    let clean = FileResult {
        path: PathBuf::from("clean.c"),
        clean: true,
        source: None,
        formatted: None,
    };
    let report = Report {
        results: vec![dirty, clean],
        errors: Vec::new(),
    };
    let doc = json_document(&report);
    assert_eq!(doc["clean"], serde_json::Value::Bool(false));
    assert!(doc["files"][0]["patch"].is_string());
    assert!(doc["files"][1]["patch"].is_null());
}

#[test]
fn formatter_short_circuits_on_empty_and_whitespace_only_source() {
    for language in [Language::JavaScript, Language::Python] {
        assert_eq!(Cs50Formatter.format("", language).expect("ok"), "");
        assert_eq!(Cs50Formatter.format("\n  ", language).expect("ok"), "\n  ");
    }
}

#[test]
fn empty_file_is_a_per_file_error() {
    let path = temp_file("empty.js", "");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Identity);
    assert!(report.has_errors());
    assert!(report.results.is_empty());
    assert_eq!(report.errors.len(), 1);
    assert_eq!(&report.errors[0].0, &path);
    assert_eq!(report.errors[0].1, "file is empty");
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn whitespace_only_file_is_a_per_file_error() {
    let path = temp_file("blank.js", " \n\t\n");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Identity);
    assert!(report.has_errors());
    assert!(report.results.is_empty());
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].1, "file is empty");
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn normalization_trailing_whitespace_is_not_flagged() {
    // style50 3.0.0 rstrips every line before formatting, so trailing
    // whitespace never makes a file dirty (Rstrip's output equals the
    // normalized input).
    let path = temp_file("trailing.js", "x = 1   \ny = 2\t\n");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Rstrip);
    assert!(!report.has_errors());
    assert_eq!(report.results.len(), 1);
    assert!(report.results[0].clean);
    let result = &report.results[0];
    assert_eq!(result.source, result.formatted, "clean: styled == source");
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn normalization_appends_missing_trailing_newline() {
    let path = temp_file("nonewline.js", "x = 1");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Identity);
    assert!(!report.has_errors());
    assert!(report.results[0].clean);
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn normalization_converts_crlf_to_lf() {
    let path = temp_file("crlf.js", "x = 1\r\ny = 2\r\n");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Identity);
    assert!(!report.has_errors());
    assert!(report.results[0].clean);
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn empty_request_is_clean() {
    let req = Request {
        files: vec![],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Identity);
    assert!(report.clean());
    assert!(!report.has_errors());
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
    let report = run_with(&req, &Identity);
    assert!(report.has_errors());
    assert!(report.results.is_empty());
    assert_eq!(report.errors.len(), 1);
    assert_eq!(&report.errors[0].0, &path);
    assert!(report.errors[0].1.contains("unsupported file type"));
    assert!(report.errors[0].1.contains(&path.display().to_string()));
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
    let report = run_with(&req, &Identity);
    assert!(report.has_errors());
    assert!(report.results.is_empty());
    assert_eq!(report.errors.len(), 1);
    assert_eq!(&report.errors[0].0, &path);
    assert!(report.errors[0].1.contains("could not read"));
    assert!(report.errors[0].1.contains(&path.display().to_string()));
}

#[test]
fn formatter_failure_is_recorded_per_file() {
    let path = temp_file("failing.c", "int main(void)\n{\n    return 0;\n}\n");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Failing);
    assert!(report.has_errors());
    assert!(report.results.is_empty());
    assert_eq!(report.errors.len(), 1);
    assert_eq!(&report.errors[0].0, &path);
    assert!(report.errors[0].1.contains("boom"));
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn error_in_later_file_preserves_earlier_results() {
    let dirty = temp_file("stream.c", "int main(void)\n{\nreturn 0;\n}\n");
    let missing =
        std::env::temp_dir().join(format!("u50_style_test_{}_gone.c", std::process::id()));
    let req = Request {
        files: vec![dirty.clone(), missing.clone()],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Reindent);
    assert!(report.has_errors());
    assert_eq!(report.results.len(), 1);
    assert!(!report.results[0].clean);
    let rendered = render_result(&report.results[0], Output::Unified, false);
    assert!(rendered.lines().any(|l| l.starts_with('+')));
    assert_eq!(report.errors.len(), 1);
    assert_eq!(&report.errors[0].0, &missing);
    assert!(report.errors[0].1.contains("could not read"));
    std::fs::remove_file(&dirty).expect("cleanup");
}

#[test]
fn character_render_colored_uses_background_transitions_and_plain_has_no_ansi() {
    // A deleted newline: the deletion transition is reset+on-red, and the
    // legend line is an on-red marker followed by the yellow message.
    let deleted = render_character("a\nb\n", "ab\n", true, false);
    assert!(
        deleted.contains(&format!("{}{}", reset(), on_red())),
        "red deletion transition missing: {deleted:?}"
    );
    assert!(
        deleted.contains(&format!(
            "{}\\n{}{} means that you should delete a newline.{}",
            on_red(),
            reset(),
            yellow(),
            reset()
        )),
        "colored deletion legend missing: {deleted:?}"
    );
    assert!(
        !deleted.contains(&format!("{}{}", reset(), on_green())),
        "no insertion background in a deletion-only diff: {deleted:?}"
    );
    // An inserted newline: reset+on-green transition, on-green legend.
    let inserted = render_character("ab\n", "a\nb\n", true, false);
    assert!(
        inserted.contains(&format!("{}{}", reset(), on_green())),
        "green insertion transition missing: {inserted:?}"
    );
    assert!(
        inserted.contains(&format!(
            "{}\\n{}{} means that you should insert a newline.{}",
            on_green(),
            reset(),
            yellow(),
            reset()
        )),
        "colored insertion legend missing: {inserted:?}"
    );
    // Every background span is closed by a reset.
    assert!(inserted.contains("\\n\u{1b}[0m"));
    // The non-colored renderings of the same diffs must not contain ANSI.
    for (source, formatted) in [("a\nb\n", "ab\n"), ("ab\n", "a\nb\n")] {
        let plain = render_character(source, formatted, false, false);
        assert!(!plain.contains('\u{1b}'), "unexpected ANSI: {plain:?}");
    }
}

#[test]
fn split_render_colored_wraps_cells_in_red_and_green() {
    let out = render_split("return 0;\n", "    return 0;\n", true);
    let line = out.lines().next().expect("one row");
    assert!(
        line.starts_with(red()),
        "left cell not red-wrapped: {line:?}"
    );
    assert!(
        line.contains(&format!("{} | {}", reset(), green())),
        "right cell not green-wrapped: {line:?}"
    );
    assert!(
        line.ends_with(reset()),
        "row not reset-terminated: {line:?}"
    );
    let plain = render_split("return 0;\n", "    return 0;\n", false);
    assert!(!plain.contains('\u{1b}'), "unexpected ANSI: {plain:?}");
}

#[test]
fn split_render_truncates_columns_at_50_chars() {
    // Differing 60-char lines so the diff has an actual change to render.
    let long_x = "x".repeat(60);
    let long_y = "y".repeat(60);
    let out = render_split(&format!("{long_x}\n"), &format!("{long_y}\n"), false);
    let line = out.lines().next().expect("one row");
    assert_eq!(line.chars().count(), 50 + 3 + 50);
    assert_eq!(line.matches('x').count(), 50, "not truncated: {line:?}");
    assert_eq!(line.matches('y').count(), 50, "not truncated: {line:?}");
}

#[test]
fn fix_writes_styled_content_for_dirty_file() {
    let path = temp_file("fixdirty.c", "int main(void)\n{\nreturn 0;\n}\n");
    let report = fix_with(&fix_request(vec![path.clone()]), &Reindent, false);
    assert!(!report.has_errors());
    assert_eq!(report.results.len(), 1);
    assert!(!report.results[0].clean);
    let expected = "    int main(void)\n    {\n    return 0;\n    }\n";
    assert_eq!(report.results[0].formatted.as_deref(), Some(expected));
    assert_eq!(std::fs::read_to_string(&path).expect("read back"), expected);
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn fix_dry_run_leaves_file_unchanged() {
    let original = "int main(void)\n{\nreturn 0;\n}\n";
    let path = temp_file("fixdry.c", original);
    let report = fix_with(&fix_request(vec![path.clone()]), &Reindent, true);
    assert!(!report.has_errors());
    // A dry run with a would-fix reports dirty (drives the exit-1 contract).
    assert!(!report.clean());
    assert!(!report.results[0].clean);
    assert!(report.results[0].formatted.is_some());
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        original,
        "dry run must not write"
    );
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn fix_clean_file_is_not_rewritten() {
    let original = "int main(void)\n{\n    return 0;\n}\n";
    let path = temp_file("fixclean.c", original);
    let report = fix_with(&fix_request(vec![path.clone()]), &Identity, false);
    assert!(!report.has_errors());
    assert!(report.clean());
    assert!(report.results[0].clean);
    assert_eq!(report.results[0].formatted.as_deref(), Some(original));
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        original,
        "clean file must not be rewritten"
    );
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn fix_already_styled_file_is_not_rewritten() {
    let original = "x = 1\ny = 2\n";
    let path = temp_file("fixfixed.py", original);
    // Rstrip's output equals the normalized input, so the file is already
    // styled for this formatter: nothing to write.
    let report = fix_with(&fix_request(vec![path.clone()]), &Rstrip, false);
    assert!(!report.has_errors());
    assert!(report.results[0].clean);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        original,
        "already-styled file must not be rewritten"
    );
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn fix_missing_file_errors_and_still_fixes_others() {
    let dirty = temp_file("fixmix.c", "int main(void)\n{\nreturn 0;\n}\n");
    let missing =
        std::env::temp_dir().join(format!("u50_style_test_{}_nofix.c", std::process::id()));
    let report = fix_with(
        &fix_request(vec![dirty.clone(), missing.clone()]),
        &Reindent,
        false,
    );
    assert!(report.has_errors());
    assert_eq!(report.errors.len(), 1);
    assert_eq!(&report.errors[0].0, &missing);
    assert!(report.errors[0].1.contains("could not read"));
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].path, dirty);
    assert!(!report.results[0].clean);
    assert_eq!(
        std::fs::read_to_string(&dirty).expect("read back"),
        "    int main(void)\n    {\n    return 0;\n    }\n"
    );
    std::fs::remove_file(&dirty).expect("cleanup");
}

#[test]
fn split_render_pairs_deletions_with_insertions_and_pads_blanks() {
    // Two deletions, one insertion: the unpaired deletion gets a blank
    // right cell.
    let out = render_split("a\nb\n", "c\n", false);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    let row0 = lines[0];
    let row1 = lines[1];
    assert!(row0.starts_with('a'), "row 0: {row0:?}");
    assert!(row0.contains(" | c"), "row 0: {row0:?}");
    assert!(row1.starts_with('b'), "row 1: {row1:?}");
    let sep = row1.find(" | ").expect("separator");
    assert!(
        row1[sep + 3..].chars().all(|ch| ch == ' '),
        "right cell not blank-padded: {row1:?}"
    );

    // One deletion, two insertions: the unpaired insertion gets a blank
    // left cell.
    let out = render_split("c\n", "a\nb\n", false);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    let row0 = lines[0];
    let row1 = lines[1];
    assert!(row0.starts_with('c'), "row 0: {row0:?}");
    assert!(row0.contains(" | a"), "row 0: {row0:?}");
    let sep = row1.find(" | ").expect("separator");
    assert!(
        row1[..sep].chars().all(|ch| ch == ' '),
        "left cell not blank-padded: {row1:?}"
    );
    assert!(row1[sep + 3..].starts_with('b'), "row 1: {row1:?}");
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "u50_style_test_{}_{}_dir",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn write_in(dir: &Path, rel: &str, contents: &str) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create subdir");
    std::fs::write(&path, contents).expect("write file");
    path
}

const DIRTY_C: &str = "int main(void)\n{\nreturn 0;\n}\n";

#[test]
fn expand_paths_walks_directories_filters_and_sorts() {
    let root = temp_dir("walk");
    let c = write_in(&root, "dirty.c", DIRTY_C);
    let py = write_in(&root, "sub/dirty.py", "x = 1\n");
    let js = write_in(&root, "sub/deep/dirty.js", "x = 1;\n");
    let unsupported = write_in(&root, "unsupported.rb", "puts 1\n");
    let hidden = write_in(&root, ".hiddendir/dirty2.c", DIRTY_C);
    let (expanded, skipped) = expand_paths(std::slice::from_ref(&root));
    // Hidden dirs are included (style50 parity: --ignore is the filter);
    // unsupported regular files are reported as skipped (in walk order);
    // the kept list is sorted and unique.
    let mut expected = vec![c, hidden, js, py];
    expected.sort();
    assert_eq!(expanded, expected);
    assert_eq!(skipped, vec![unsupported]);
    std::fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn expand_paths_keeps_explicit_unsupported_file() {
    let root = temp_dir("keepunsup");
    let rb = write_in(&root, "bad.rb", "puts 1\n");
    let (files, skipped) = expand_paths(std::slice::from_ref(&rb));
    assert_eq!(files, vec![rb]);
    assert!(skipped.is_empty());
    std::fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn expand_paths_keeps_missing_path() {
    let missing = std::env::temp_dir().join(format!(
        "u50_style_test_{}_gone_dir_missing.c",
        std::process::id()
    ));
    let (files, skipped) = expand_paths(std::slice::from_ref(&missing));
    assert_eq!(files, vec![missing]);
    assert!(skipped.is_empty());
}

#[test]
fn expand_paths_dedupes_dir_and_file_inside() {
    let root = temp_dir("dedupe");
    let c = write_in(&root, "dirty.c", DIRTY_C);
    let py = write_in(&root, "sub/dirty.py", "x = 1\n");
    let (expanded, skipped) = expand_paths(&[root.clone(), c.clone(), root]);
    assert_eq!(expanded, vec![c, py]);
    assert!(skipped.is_empty());
}

#[test]
fn expand_paths_empty_input_is_empty() {
    let (files, skipped) = expand_paths(&[]);
    assert!(files.is_empty());
    assert!(skipped.is_empty());
}

#[test]
fn expand_paths_symlink_parity_follows_operands_not_subdirs() {
    let root = temp_dir("symlink");
    let other = temp_dir("symlink_target");
    write_in(&other, "other.js", "x = 1;\n");
    write_in(&root, "dirty.c", DIRTY_C);
    #[cfg(unix)]
    {
        // Inside a walked tree: a symlinked directory is never descended
        // into (os.walk followlinks=false) â€” but a symlinked regular file
        // IS collected like any file (style50 filters by name).
        let subdir_link = root.join("sublink");
        std::os::unix::fs::symlink(&other, &subdir_link).expect("create symlink");
        let file_link = root.join("filelink.js");
        std::os::unix::fs::symlink(other.join("other.js"), &file_link)
            .expect("create file symlink");
        let (files, skipped) = expand_paths(std::slice::from_ref(&root));
        assert_eq!(files, vec![root.join("dirty.c"), file_link.clone()]);
        assert!(skipped.is_empty());
        // A symlinked directory passed directly IS walked (os.walk's
        // top-level behavior resolves the operand).
        let (files, _) = expand_paths(std::slice::from_ref(&subdir_link));
        assert_eq!(files, vec![subdir_link.join("other.js")]);
        // A symlinked regular file passed directly stays unchanged
        // (per-file processing reads through the link).
        let (files, _) = expand_paths(std::slice::from_ref(&file_link));
        assert_eq!(files, vec![file_link.clone()]);
    }
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
#[cfg(unix)]
fn expand_paths_dedupes_by_canonical_path() {
    let root = temp_dir("dedupe-canonical");
    let c = write_in(&root, "a.c", DIRTY_C);
    // Equivalent spellings of one file, plus a repeated directory
    // operand: the file must be processed exactly once.
    let awkward = root.join("./a.c");
    let (files, skipped) = expand_paths(&[root.clone(), awkward.clone(), root.clone()]);
    assert_eq!(files, vec![c.clone()]);
    assert!(skipped.is_empty(), "walk warnings deduped too: {skipped:?}");
    // Explicit unsupported operands still dedupe (they stay in `files`
    // â€” explicit arguments keep their per-file error semantics).
    let txt = write_in(&root, "note.txt", "hi\n");
    let (files, skipped) = expand_paths(&[txt.clone(), root.join("note.txt")]);
    assert_eq!(files, vec![txt.clone()]);
    assert!(skipped.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn expand_paths_empty_directory_contributes_nothing() {
    let root = temp_dir("emptydir");
    let (files, skipped) = expand_paths(std::slice::from_ref(&root));
    assert!(files.is_empty());
    assert!(skipped.is_empty());
    std::fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn run_with_expands_directory_argument() {
    let root = temp_dir("rundir");
    let c = write_in(&root, "dirty.c", DIRTY_C);
    let py = write_in(&root, "sub/dirty.py", "x = 1\n");
    let report = run_with(&fix_request(vec![root]), &Reindent);
    assert!(!report.has_errors());
    assert_eq!(report.results.len(), 2);
    let mut paths: Vec<PathBuf> = report.results.iter().map(|r| r.path.clone()).collect();
    paths.sort();
    assert_eq!(paths, vec![c, py]);
    assert!(report.results.iter().all(|r| !r.clean));
}

#[test]
fn fix_with_fixes_every_file_in_directory() {
    let root = temp_dir("fixdir");
    let c = write_in(&root, "dirty.c", DIRTY_C);
    let js = write_in(&root, "sub/dirty.js", "x = 1;\n");
    write_in(&root, "unsupported.rb", "puts 1\n");
    let report = fix_with(&fix_request(vec![root]), &Reindent, false);
    assert!(!report.has_errors());
    assert_eq!(report.results.len(), 2);
    assert_eq!(
        std::fs::read_to_string(&c).expect("read back c"),
        "    int main(void)\n    {\n    return 0;\n    }\n"
    );
    assert_eq!(
        std::fs::read_to_string(&js).expect("read back js"),
        "    x = 1;\n"
    );
}

#[test]
fn select_algorithm_small_input_uses_myers() {
    // Below ADAPTIVE_MIN_LINES the probe is skipped entirely.
    let source = "a\nb\nc\n";
    assert_eq!(select_algorithm(source, "x\ny\n"), Algorithm::Myers);
    // Identical large texts with many distinct lines: always Myers.
    let big = numbered_lines("same ", 2000);
    assert_eq!(select_algorithm(&big, &big), Algorithm::Myers);
}

#[test]
fn select_algorithm_large_zero_overlap_uses_lcs() {
    let source = numbered_lines("old ", 2000);
    let formatted = numbered_lines("new ", 2000);
    assert_eq!(select_algorithm(&source, &formatted), Algorithm::Lcs);
}

#[test]
fn select_algorithm_large_with_many_common_lines_uses_myers() {
    // 3 distinct common lines on a 2000-line pair: 3 * 1000 >= 2000, so the
    // heuristic keeps Myers (mirrors the measured 8-common@7.5k collapse).
    let source = numbered_lines("old ", 2000);
    let mut formatted = numbered_lines("new ", 2000);
    formatted.push_str("old 0\nold 1\nold 2\n");
    assert_eq!(select_algorithm(&source, &formatted), Algorithm::Myers);
    // Identical large texts share every line: always Myers.
    let big = numbered_lines("same ", 3000);
    assert_eq!(select_algorithm(&big, &big), Algorithm::Myers);
}

#[test]
fn large_wholly_changed_input_renders_completely() {
    // 5000 wholly-changed lines: exercises the adaptive Lcs path and guards
    // against any rendering mode dropping change rows on large inputs.
    let source = numbered_lines("old ", 5000);
    let formatted = numbered_lines("new ", 5000);

    let unified = render_unified(&source, &formatted, Path::new("x.c"));
    let dels = unified
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .count();
    let adds = unified
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .count();
    assert_eq!(dels, 5000, "unified deletions incomplete");
    assert_eq!(adds, 5000, "unified insertions incomplete");

    // Character mode re-renders the original text with the styled text
    // inlined at the diff positions (no +/- rows): every one of the 5000
    // lines must carry both the deleted (red background) and inserted
    // (green background) span.
    let character = render_character(&source, &formatted, true, false);
    let char_lines = character.lines().filter(|l| !l.is_empty()).count();
    assert_eq!(char_lines, 5000, "character body incomplete");
    assert_eq!(
        character.lines().filter(|l| l.contains(on_red())).count(),
        5000,
        "character deletions incomplete"
    );
    assert_eq!(
        character.lines().filter(|l| l.contains(on_green())).count(),
        5000,
        "character insertions incomplete"
    );
    let plain = render_character(&source, &formatted, false, false);
    assert_eq!(
        plain
            .lines()
            .filter(|l| l.contains("old") && l.contains("new"))
            .count(),
        5000,
        "plain character body incomplete"
    );
    assert!(!plain.contains('\u{1b}'), "unexpected ANSI: {plain:?}");

    let split = render_split(&source, &formatted, false);
    let rows: Vec<&str> = split.lines().collect();
    assert_eq!(rows.len(), 5000, "split must pair every changed row");
    for row in &rows {
        let (left, right) = row.split_once(" | ").expect("split row separator");
        assert!(!left.trim().is_empty(), "unpaired deletion row: {row:?}");
        assert!(!right.trim().is_empty(), "unpaired insertion row: {row:?}");
    }
}

#[test]
fn pip_package_maps_every_language_to_its_backend() {
    let cases = [
        (Language::C, "clang-format"),
        (Language::Cpp, "clang-format"),
        (Language::Java, "clang-format"),
        (Language::Python, "autopep8"),
        (Language::JavaScript, "jsbeautifier"),
        (Language::Html, "djhtml"),
        (Language::Css, "cssbeautifier"),
        (Language::Sql, "sqlparse"),
    ];
    for (language, package) in cases {
        assert_eq!(language.pip_package(), package);
    }
    // ALL covers every variant exactly once (8 entries, no duplicates).
    assert_eq!(Language::ALL.len(), 8);
    let mut seen: Vec<Language> = Vec::new();
    for &language in &Language::ALL {
        assert!(!seen.contains(&language), "duplicate in ALL: {language:?}");
        seen.push(language);
    }
}

#[test]
fn cache_dirs_are_nested_under_the_cache_root() {
    // Path construction only: the exact base depends on the environment
    // (XDG_CACHE_HOME vs HOME/.cache vs %LOCALAPPDATA%), so the test
    // asserts the suffix and the platform venv layout.
    let root = cache_dir().expect("cache dir is determinable in tests");
    assert!(root.ends_with(std::path::Path::new("u50").join("style50")));
    let bin = cache_bin_dir().expect("cache bin dir is determinable in tests");
    assert_eq!(bin, venv_bin_dir(&root.join("venv")));
}

#[test]
fn locate_tool_passes_through_explicit_paths() {
    // A tool name containing '/' is used as-is (explicit-path semantics).
    assert_eq!(
        locate_tool("/bin/sh").map(|(path, _)| path).as_deref(),
        Some(std::path::Path::new("/bin/sh"))
    );
    // Absolute paths are reported even when the file does not exist â€”
    // the exec failure surfaces through the normal spawn error path.
    assert_eq!(
        locate_tool("/nonexistent/u50-probe-xyz")
            .map(|(path, _)| path)
            .as_deref(),
        Some(std::path::Path::new("/nonexistent/u50-probe-xyz"))
    );
    // Bare names resolve only from the u50 style cache (never from PATH);
    // on machines without u50-installed tools this branch is simply not
    // taken. A name that exists nowhere must resolve to None.
    if let Some((path, _)) = locate_tool("sh") {
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("sh"));
    }
    assert_eq!(locate_tool("u50-definitely-not-installed-xyz"), None);
}

/// Renderer that records the event sequence (test helper).
struct Recorder {
    events: Vec<String>,
    total_files: Option<usize>,
}

impl Renderer for Recorder {
    fn begin(&mut self, _req: &Request) {
        self.events.push("begin".into());
    }

    fn total_files(&mut self, count: usize) {
        self.total_files = Some(count);
        self.events.push(format!("total:{count}"));
    }

    fn file(&mut self, result: &FileResult) {
        self.events.push(format!("file:{}", result.path.display()));
    }

    fn file_error(&mut self, path: &Path, message: &str) {
        self.events
            .push(format!("error:{}:{message}", path.display()));
    }

    fn skipped(&mut self, path: &Path) {
        self.events.push(format!("skipped:{}", path.display()));
    }

    fn finish(&mut self, _report: &Report) {
        self.events.push("finish".into());
    }
}

#[test]
fn run_with_renderer_emits_events_in_order() {
    let dirty = temp_file("evdirty.c", DIRTY_C);
    let missing =
        std::env::temp_dir().join(format!("u50_style_test_{}_evmissing.c", std::process::id()));
    let req = fix_request(vec![dirty.clone(), missing.clone()]);
    let mut recorder = Recorder {
        events: Vec::new(),
        total_files: None,
    };
    let report = run_with_renderer(&req, &Reindent, &mut recorder);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.errors.len(), 1);
    // The total-files event (results + errors) precedes every per-file
    // event: character mode needs the count for its header decision.
    assert_eq!(recorder.total_files, Some(2));
    assert_eq!(recorder.events.len(), 5);
    assert_eq!(recorder.events[0], "begin");
    assert_eq!(recorder.events[1], "total:2");
    assert_eq!(recorder.events[2], format!("file:{}", dirty.display()));
    assert!(
        recorder.events[3].starts_with(&format!("error:{}:could not read", missing.display())),
        "unexpected file_error event: {:?}",
        recorder.events[3]
    );
    assert_eq!(recorder.events[4], "finish");
    std::fs::remove_file(&dirty).expect("cleanup");
}

#[test]
fn run_with_renderer_emits_skipped_for_walk_warnings() {
    // A directory operand with an unsupported regular file: the walk
    // warning surfaces as a `skipped` event between `total_files` and
    // the per-file events â€” and never for explicit file arguments (the
    // other tests pass explicit operands and record no `skipped`).
    let root = temp_dir("renderer-skipped");
    let dirty = write_in(&root, "dirty.c", DIRTY_C);
    write_in(&root, "note.txt", "not code\n");
    let req = fix_request(vec![root.clone()]);
    let mut recorder = Recorder {
        events: Vec::new(),
        total_files: None,
    };
    let report = run_with_renderer(&req, &Reindent, &mut recorder);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.errors.len(), 0);
    assert_eq!(
        recorder.events,
        vec![
            "begin".to_owned(),
            "total:1".to_owned(),
            format!("skipped:{}", root.join("note.txt").display()),
            format!("file:{}", dirty.display()),
            "finish".to_owned(),
        ]
    );
    std::fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn run_with_renderer_empty_request_emits_begin_total_and_finish() {
    let req = fix_request(vec![]);
    let mut recorder = Recorder {
        events: Vec::new(),
        total_files: None,
    };
    run_with_renderer(&req, &Identity, &mut recorder);
    assert_eq!(recorder.total_files, Some(0));
    assert_eq!(
        recorder.events,
        vec![
            "begin".to_owned(),
            "total:0".to_owned(),
            "finish".to_owned()
        ]
    );
}

#[test]
fn console_renderer_matches_direct_rendering() {
    let source = "int main(void)\n{\nreturn 0;\n}\n";
    let formatted = "    int main(void)\n    {\n    return 0;\n    }\n";
    let result = FileResult {
        path: PathBuf::from("x.c"),
        clean: false,
        source: Some(source.to_owned()),
        formatted: Some(formatted.to_owned()),
    };
    for color in [false, true] {
        assert_eq!(
            render_result(&result, Output::Character, color),
            render_character(source, formatted, color, comment_hint(source, Language::C)),
            "character mode, color={color}"
        );
        assert_eq!(
            render_result(&result, Output::Split, color),
            render_split(source, formatted, color),
            "split mode, color={color}"
        );
        assert_eq!(
            render_result(&result, Output::Unified, color),
            render_unified(source, formatted, Path::new("x.c")),
            "unified mode, color={color}"
        );
    }
}

#[test]
fn console_renderer_clean_file_is_looks_good_in_character_mode_only() {
    let result = FileResult {
        path: PathBuf::from("x.c"),
        clean: true,
        source: Some("return 0;\n".to_owned()),
        formatted: Some("return 0;\n".to_owned()),
    };
    // Character mode (style50 parity): clean files get `Looks good!`,
    // followed by the comments hint (this source has no comments, ratio
    // 0 < 0.10, oracle-verified).
    assert_eq!(
        render_result(&result, Output::Character, false),
        "Looks good!\nBut consider adding more comments!\n"
    );
    assert_eq!(
        render_result(&result, Output::Character, true),
        format!(
            "{}Looks good!{}\n{}But consider adding more comments!{}\n",
            green(),
            reset(),
            yellow(),
            reset()
        )
    );
    // The other text modes stay silent for clean files.
    for output in [Output::Split, Output::Unified] {
        assert!(
            render_result(&result, output, false).is_empty(),
            "clean file must render nothing in {output:?}"
        );
    }
}

#[test]
fn console_renderer_character_mode_banner_headers_and_looks_good() {
    let req = Request {
        files: vec![],
        output: Output::Character,
        color: false,
    };
    let clean = FileResult {
        path: PathBuf::from("a.c"),
        clean: true,
        source: Some("return 0;\n".to_owned()),
        formatted: Some("return 0;\n".to_owned()),
    };
    let dirty = FileResult {
        path: PathBuf::from("b.c"),
        clean: false,
        source: Some("return 0;\n".to_owned()),
        formatted: Some("    return 0;\n".to_owned()),
    };
    let sink = SharedBuf::default();
    {
        let mut renderer = builtin_renderer(Output::Character, false, Box::new(sink.clone()));
        renderer.begin(&req);
        renderer.total_files(2);
        renderer.file(&clean);
        renderer.file(&dirty);
    }
    let text = String::from_utf8(sink.0.borrow().clone()).expect("utf8");
    let banner = format!("Results generated by u50 v{}", env!("CARGO_PKG_VERSION"));
    assert!(
        text.starts_with(&format!("{banner}\n")),
        "banner first, followed directly by the next element (style50 parity): {text:?}"
    );
    // Headers apply only when the run covers more than one file: one rule
    // pair per file around the file name (clean files included).
    assert_eq!(text.matches(HEADER_RULE).count(), 4);
    assert!(text.contains(&format!("{HEADER_RULE}\nb.c\n{HEADER_RULE}\n")));
    assert_eq!(text.matches("Looks good!").count(), 1);
    assert!(
        text.contains("\n    return 0;\n\n"),
        "dirty file block after its header: {text:?}"
    );
}

#[test]
fn console_renderer_single_file_prints_no_header() {
    // The negative branch of the header rule: a single-file run (the most
    // common invocation) prints no ::::::::::::::  header at all.
    let req = Request {
        files: vec![],
        output: Output::Character,
        color: false,
    };
    let dirty = FileResult {
        path: PathBuf::from("only.c"),
        clean: false,
        source: Some("return 0;\n".to_owned()),
        formatted: Some("    return 0;\n".to_owned()),
    };
    let sink = SharedBuf::default();
    {
        let mut renderer = builtin_renderer(Output::Character, false, Box::new(sink.clone()));
        renderer.begin(&req);
        renderer.total_files(1);
        renderer.file(&dirty);
    }
    let text = String::from_utf8(sink.0.borrow().clone()).expect("utf8");
    assert!(
        !text.contains(HEADER_RULE),
        "single-file run must not print per-file headers: {text:?}"
    );
    assert!(
        text.contains("    return 0;\n"),
        "diff block still rendered"
    );
}

#[test]
fn console_renderer_character_mode_colors_banner_and_headers() {
    let req = Request {
        files: vec![],
        output: Output::Character,
        color: true,
    };
    let clean = FileResult {
        path: PathBuf::from("a.c"),
        clean: true,
        source: Some("return 0;\n".to_owned()),
        formatted: Some("return 0;\n".to_owned()),
    };
    let sink = SharedBuf::default();
    {
        let mut renderer = builtin_renderer(Output::Character, true, Box::new(sink.clone()));
        renderer.begin(&req);
        // Two files so the cyan header path (total_files > 1) is exercised.
        renderer.total_files(2);
        renderer.file(&clean);
    }
    let text = String::from_utf8(sink.0.borrow().clone()).expect("utf8");
    let banner = format!("Results generated by u50 v{}", env!("CARGO_PKG_VERSION"));
    // Bold bright-white banner, cyan header pair, green Looks good!.
    assert!(
        text.contains(&format!("{}{banner}{}", bold(), bright_white())),
        "banner not bold-bright-white: {text:?}"
    );
    assert!(text.contains(cyan()), "header not cyan: {text:?}");
    assert!(
        text.contains(&format!("{}Looks good!{}", green(), reset())),
        "Looks good! not green: {text:?}"
    );
}

#[test]
fn json_renderer_output_matches_json_document_plus_newline() {
    let dirty = temp_file("jsonrender.c", DIRTY_C);
    let req = Request {
        files: vec![dirty.clone()],
        output: Output::Json,
        color: false,
    };
    let report = run_with(&req, &Reindent);
    let sink = SharedBuf::default();
    let mut renderer = builtin_renderer(Output::Json, false, Box::new(sink.clone()));
    renderer.begin(&req);
    for result in &report.results {
        renderer.file(result);
    }
    for (path, message) in &report.errors {
        renderer.file_error(path, message);
    }
    renderer.finish(&report);
    // Byte-identical to pretty-printing the document with a 4-space
    // indent (style50's JSON formatting) plus a trailing newline.
    let mut expected = crate::renderer::json_pretty(&json_document(&report));
    expected.push(b'\n');
    assert_eq!(
        String::from_utf8(sink.0.borrow().clone()).expect("utf8"),
        String::from_utf8(expected).expect("utf8")
    );
    std::fs::remove_file(&dirty).expect("cleanup");
}

#[test]
fn json_renderer_output_is_pretty_printed_with_four_space_indent() {
    // style50 pretty-prints its JSON with indent 4; u50 must match the
    // formatting (the schema itself stays u50's own â€” documented as a
    // by-design divergence in STYLE50_V3_CROSSCHECK.md).
    let dirty = temp_file("jsonpretty.c", DIRTY_C);
    let req = Request {
        files: vec![dirty.clone()],
        output: Output::Json,
        color: false,
    };
    let report = run_with(&req, &Reindent);
    let sink = SharedBuf::default();
    let mut renderer = builtin_renderer(Output::Json, false, Box::new(sink.clone()));
    renderer.finish(&report);
    let text = String::from_utf8(sink.0.borrow().clone()).expect("utf8");

    // Pretty-printed: newlines and 4-space indentation, not one line.
    assert!(text.contains('\n'), "json output must be multi-line");
    assert!(
        text.contains("\n    \"clean\"") && text.contains("\n            \"path\""),
        "json output must use 4-space indentation:\n{text}"
    );
    let compact = serde_json::to_string(&json_document(&report)).expect("compact");
    assert_ne!(text.trim_end(), compact, "json output must not be compact");
    // Still parses to the same document.
    let doc: serde_json::Value =
        serde_json::from_str(text.trim_end()).expect("renderer wrote valid json");
    assert_eq!(doc, json_document(&report));
    std::fs::remove_file(&dirty).expect("cleanup");
}

#[test]
fn json_renderer_write_target_is_used_not_stdout() {
    // The renderer must write to the injected sink, not stdout: feed it an
    // in-memory buffer via builtin_renderer and check the bytes land there.
    let report = Report {
        results: vec![FileResult {
            path: PathBuf::from("c.c"),
            clean: true,
            source: Some("int main(void)\n{\n    return 0;\n}\n".to_owned()),
            formatted: Some("int main(void)\n{\n    return 0;\n}\n".to_owned()),
        }],
        errors: Vec::new(),
    };
    let sink = SharedBuf::default();
    {
        let mut renderer = builtin_renderer(Output::Json, false, Box::new(sink.clone()));
        renderer.finish(&report);
    }
    let text = String::from_utf8(sink.0.borrow().clone()).expect("utf8");
    assert!(text.ends_with('\n'));
    let doc: serde_json::Value =
        serde_json::from_str(text.trim_end()).expect("renderer wrote valid json");
    assert_eq!(doc["files"][0]["patch"], serde_json::Value::Null);
}

#[test]
fn py_str_f64_matches_python_str() {
    use crate::renderer::py_str_f64;

    assert_eq!(py_str_f64(1.0), "1.0");
    assert_eq!(py_str_f64(0.0), "0.0");
    assert_eq!(py_str_f64(0.5), "0.5");
    // Python: str(1 - 3/26) == '0.8846153846153846'.
    assert_eq!(py_str_f64(1.0 - 3.0 / 26.0), "0.8846153846153846");
}

#[test]
fn score_renderer_clean_file_is_1() {
    let source = "int main(void)\n{\n    return 0;\n}\n";
    assert_eq!(
        score_output(
            vec![FileResult {
                path: PathBuf::from("a.c"),
                clean: true,
                source: Some(source.to_owned()),
                formatted: Some(source.to_owned()),
            }],
            Vec::new(),
            false
        ),
        "1.0\n"
    );
}

#[test]
fn score_renderer_formula_one_changed_line() {
    // Mirrors the original's formula: one changed line -> one '-' and one
    // '+' ndiff line -> diffs = 2/2 = 1.0; the styled text has 3 non-blank
    // lines; score = 1 - 1/3 = 0.6666666666666667 (Python str). Blank
    // lines in the styled text never count toward `lines`.
    assert_eq!(
        score_output(
            vec![FileResult {
                path: PathBuf::from("a.c"),
                clean: false,
                source: Some("int a;\nint b;\nint c;\n".to_owned()),
                formatted: Some("int a;\nint b;\nint d;\n".to_owned()),
            }],
            Vec::new(),
            false
        ),
        "0.6666666666666667\n"
    );
}

#[test]
fn score_renderer_half_insert_and_blank_lines() {
    // A single inserted blank line: diffs = 1/2 = 0.5; the styled text has
    // 2 non-blank lines; score = 1 - 0.5/2 = 0.75.
    assert_eq!(
        score_output(
            vec![FileResult {
                path: PathBuf::from("a.c"),
                clean: false,
                source: Some("a\nb\n".to_owned()),
                formatted: Some("a\nb\n\n".to_owned()),
            }],
            Vec::new(),
            false
        ),
        "0.75\n"
    );
}

#[test]
fn score_renderer_python_counts_all_lines() {
    // style50's `Python.count_lines` counts ALL lines (blank lines matter
    // per PEP 8), unlike every other language: 3 non-blank + 2 blank = 5
    // styled lines; diffs = 1.0; score = 1 - 1/5 = 0.8.
    assert_eq!(
        score_output(
            vec![FileResult {
                path: PathBuf::from("a.py"),
                clean: false,
                source: Some("a\nb\nc\n".to_owned()),
                formatted: Some("a\nb\nc\n\n\n".to_owned()),
            }],
            Vec::new(),
            false
        ),
        "0.8\n"
    );
}

#[test]
fn score_renderer_non_python_still_counts_non_blank() {
    // The same styled text under a .c path: the 2 blank lines do NOT
    // count (C and all other languages keep non-blank counting, and their
    // parity with style50 is byte-exact): score = 1 - 1/3.
    assert_eq!(
        score_output(
            vec![FileResult {
                path: PathBuf::from("a.c"),
                clean: false,
                source: Some("a\nb\nc\n".to_owned()),
                formatted: Some("a\nb\nc\n\n\n".to_owned()),
            }],
            Vec::new(),
            false
        ),
        "0.6666666666666667\n"
    );
}

#[test]
fn html_renderer_matches_style50_structure() {
    // The html renderer drives the style50 `results.html` template: the
    // branch-resolved pre elements, the markupsafe-escaped names, and the
    // raw diff HTML (verified byte-identical against `style50 -o html` for
    // every golden fixture; this test pins the structure at the unit
    // level, including the two distinct escape flavors: markupsafe for
    // names/errors, stdlib html.escape for the diff).
    let req = Request {
        files: vec![],
        output: Output::Html,
        color: false,
    };
    let clean = FileResult {
        path: PathBuf::from("a.c"),
        clean: true,
        source: Some("return 0;\n".to_owned()),
        formatted: Some("return 0;\n".to_owned()),
    };
    let dirty = FileResult {
        path: PathBuf::from("b.c"),
        clean: false,
        source: Some("return 0;\n".to_owned()),
        formatted: Some("    return 0;\n".to_owned()),
    };
    let sink = SharedBuf::default();
    {
        let mut renderer = builtin_renderer(Output::Html, false, Box::new(sink.clone()));
        renderer.begin(&req);
        renderer.file(&clean);
        renderer.file(&dirty);
        renderer.file_error(Path::new("e'x\"y.c"), "unsupported file type");
        renderer.finish(&Report::default());
    }
    let text = String::from_utf8(sink.0.borrow().clone()).expect("utf8");
    assert!(text.starts_with("<!DOCTYPE html>\n<html>"));
    assert!(text.contains("<h3> a.c </h3>"), "clean entry: {text:?}");
    assert!(
        text.contains("<pre style=\"color: #32cf55\">Looks good!</pre>"),
        "clean branch: {text:?}"
    );
    assert!(
        text.contains("<pre><pre>\n<ins>    </ins>return 0;"),
        "dirty diff inserted as raw HTML: {text:?}"
    );
    // markupsafe flavor for the escaped error-path name (NOT &quot;).
    assert!(
        text.contains("<h3> e&#39;x&#34;y.c </h3>"),
        "name escaped with markupsafe: {text:?}"
    );
    assert!(
        text.contains("<pre style=\"color: yellow\">unsupported file type</pre>"),
        "error branch: {text:?}"
    );
    assert!(text.ends_with("</body>\n\n</html>"));
}

#[test]
fn score_renderer_errors_only_is_zero_and_yellow() {
    assert_eq!(
        score_output(
            Vec::new(),
            vec![(PathBuf::from("gone.c"), "file is empty".to_owned())],
            true
        ),
        "\u{1b}[38;5;3mfile is empty\u{1b}[0m\n0.0\n"
    );
}

#[test]
fn score_renderer_mixed_error_then_score_plain() {
    // Errors print first (file order, uncolored when color is off), then
    // the score over the successful files only.
    assert_eq!(
        score_output(
            vec![FileResult {
                path: PathBuf::from("a.c"),
                clean: true,
                source: Some("ok\n".to_owned()),
                formatted: Some("ok\n".to_owned()),
            }],
            vec![(
                PathBuf::from("b.c"),
                "could not read `b.c`: nope".to_owned()
            )],
            false
        ),
        "could not read `b.c`: nope\n1.0\n"
    );
}

#[test]
fn score_renderer_empty_styled_text_errors_not_summed() {
    // A formatter emptying a non-empty file: the original raises a
    // per-file `Error("file is empty")`; the file contributes nothing to
    // the sums, so with no other files the score is 0.0.
    assert_eq!(
        score_output(
            vec![FileResult {
                path: PathBuf::from("a.c"),
                clean: false,
                source: Some("stuff\n".to_owned()),
                formatted: Some(String::new()),
            }],
            Vec::new(),
            false
        ),
        "file is empty\n0.0\n"
    );
}

#[test]
fn score_end_to_end_via_run_with_renderer() {
    let clean = temp_file("score_clean.c", "int main(void)\n{\nreturn 0;\n}\n");
    let dirty = temp_file("score_dirty.c", "int main(void)\n{\nretrun 0;\n}\n");
    let req = Request {
        files: vec![clean.clone(), dirty.clone()],
        output: Output::Score,
        color: false,
    };
    let sink = SharedBuf::default();
    {
        let mut renderer = builtin_renderer(req.output, req.color, Box::new(sink.clone()));
        let report = run_with_renderer(&req, &FixTy, renderer.as_mut());
        assert_eq!(report.results.len(), 2);
        assert!(report.errors.is_empty());
    }
    // Clean file -> no diffs; dirty file: one changed line ->
    // diffs = 2/2 = 1.0; lines = 4 + 4 = 8 -> score = 1 - 1/8 = 0.875.
    assert_eq!(
        String::from_utf8(sink.0.borrow().clone()).expect("utf8"),
        "0.875\n"
    );
    std::fs::remove_file(&clean).expect("cleanup");
    std::fs::remove_file(&dirty).expect("cleanup");
}

// ===================== comments hint: per-language counters =====================
// Marker: comments-hint-counting-tests. Expectations mirror style50 3.0.0's
// `count_comments` per language, as probed against the live tool.

#[test]
fn c_count_comments_ignores_double_quoted_strings() {
    assert_eq!(
        count_comments("char *s = \"// not a comment\";\n", Language::C),
        Some(0)
    );
}

#[test]
fn c_count_comments_counts_multiline_block_comments() {
    assert_eq!(
        count_comments("/* one\ntwo\nthree */\n", Language::C),
        Some(1)
    );
}

#[test]
fn c_count_comments_slash_star_slash_never_closes() {
    // `/*/` opens a block whose terminator search starts *after* the two
    // opening characters, so the `*/` inside is the opener itself: the
    // comment never closes and counts nothing.
    assert_eq!(count_comments("/*/ still open\n", Language::C), Some(0));
}

#[test]
fn c_count_comments_unclosed_block_counts_nothing() {
    assert_eq!(
        count_comments("int x; /* never closed\n", Language::C),
        Some(0)
    );
}

#[test]
fn c_count_comments_char_literal_quirk_counts_slashes() {
    // style50 strips only double-quoted strings: `'//'` survives the strip
    // pass and counts one comment (live-probed quirk).
    assert_eq!(count_comments("char c = '//';\n", Language::C), Some(1));
}

#[test]
fn js_count_comments_multiline_single_quoted_string_later_line() {
    // Js string literals are same-line only: the literal starting on line 1
    // is abandoned at the newline, so the `//` on the later line counts.
    assert_eq!(
        count_comments("'multi\n// line'\n", Language::JavaScript),
        Some(1)
    );
}

#[test]
fn js_count_comments_regex_literal_is_not_a_comment() {
    // A regex literal is consumed to its closing `/`: the `\/\/` inside
    // never surfaces as a `//` comment.
    assert_eq!(
        count_comments("var re = /a\\/\\/b/;\n", Language::JavaScript),
        Some(0)
    );
}

#[test]
fn python_count_comments_module_and_function_docstrings() {
    assert_eq!(
        count_comments("\"\"\"Module doc.\"\"\"\n", Language::Python),
        Some(1)
    );
    assert_eq!(
        count_comments("def f():\n    \"\"\"Doc.\"\"\"\n", Language::Python),
        Some(1)
    );
}

#[test]
fn python_count_comments_comment_line_then_docstring() {
    // A comment-only line counts one comment and leaves `prev` AND the
    // indent stack untouched, so the following indented docstring still
    // sees an Indent transition and counts too (2 total).
    assert_eq!(
        count_comments(
            "def f():\n    # note\n    \"\"\"Doc.\"\"\"\n",
            Language::Python
        ),
        Some(2)
    );
}

#[test]
fn python_count_comments_fstring_docstring_never_counts() {
    assert_eq!(
        count_comments("f\"\"\"not a docstring\"\"\"\n", Language::Python),
        Some(0)
    );
}

#[test]
fn python_count_comments_hash_inside_string_is_not_a_comment() {
    assert_eq!(
        count_comments("s = \"# not a comment\"\n", Language::Python),
        Some(0)
    );
}

// ===================== comments hint: ratio boundary =====================

#[test]
fn comment_hint_boundary_is_strictly_below_ten_percent() {
    // 1 comment in 11 non-blank lines: 1/11 â‰ˆ 0.0909 < 0.10 â†’ hint.
    assert!(comment_hint(
        &format!("/* c */\n{}", numbered_lines("line", 10)),
        Language::C
    ));
    // 1 comment in 10: exactly 0.10 is NOT strictly below the threshold.
    assert!(!comment_hint(
        &format!("/* c */\n{}", numbered_lines("line", 9)),
        Language::C
    ));
}

#[test]
fn comment_hint_fires_for_comment_less_files() {
    // ratio 0 < 0.10: style50 nags every comment-less file (oracle-checked
    // live: `int main(void)\n{\nreturn 0;\n}` gets the hint).
    assert!(comment_hint(
        "int main(void)\n{\nreturn 0;\n}\n",
        Language::C
    ));
}

#[test]
fn comment_hint_never_fires_without_a_counter() {
    for language in [Language::Html, Language::Css, Language::Sql] {
        assert!(
            !comment_hint("<div>whatever</div>\n", language),
            "{language:?} has no count_comments and must never hint"
        );
    }
}
