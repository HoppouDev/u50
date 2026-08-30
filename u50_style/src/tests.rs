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
use crate::render::{
    BOLD, GREEN, RED, RESET, json_document, render_character, render_split, render_unified,
    select_algorithm,
};
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

/// Formatter that always fails (models a broken external tool).
struct Failing;

impl Formatter for Failing {
    fn format(&self, _source: &str, _language: Language) -> anyhow::Result<String> {
        anyhow::bail!("boom: formatter exploded")
    }
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
    let report = run_with(&req, &Reindent);
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
    let report = run_with(&req, &Reindent);
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
    let report = run_with(&req, &Reindent);
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
        rendered: Some(
            "--- dirty.c\n+++ dirty.c\n@@ -1 +1 @@\n-return 0;\n+    return 0;\n".to_owned(),
        ),
        formatted: None,
    };
    let clean = FileResult {
        path: PathBuf::from("clean.c"),
        clean: true,
        rendered: None,
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
    assert!(report.results[0].rendered.is_none());
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
    let rendered = report.results[0].rendered.clone().expect("rendered");
    assert!(rendered.lines().any(|l| l.starts_with('+')));
    assert_eq!(report.errors.len(), 1);
    assert_eq!(&report.errors[0].0, &missing);
    assert!(report.errors[0].1.contains("could not read"));
    std::fs::remove_file(&dirty).expect("cleanup");
}

#[test]
fn character_render_colored_uses_red_and_green_and_restores_line_color() {
    let source = "int main(void)\n{\nreturn 0;\n}\n";
    let formatted = "int main(void)\n{\n    return 0;\n}\n";
    let out = render_character(source, formatted, true);
    assert!(out.contains(RED), "red line color missing: {out:?}");
    assert!(out.contains(GREEN), "green line color missing: {out:?}");
    // An emphasized span must not cancel the enclosing line color: after
    // the span's RESET (which follows the emphasized text) the line color
    // code reappears before the rest of the line.
    assert!(out.contains(BOLD), "no emphasized span: {out:?}");
    assert!(
        out.contains(&format!("{RESET}{GREEN}")),
        "line color not restored after an emphasized span: {out:?}"
    );
    // The non-colored rendering of the same diff must not contain ANSI.
    let plain = render_character(source, formatted, false);
    assert!(!plain.contains('\u{1b}'), "unexpected ANSI: {plain:?}");
}

#[test]
fn split_render_colored_wraps_cells_in_red_and_green() {
    let out = render_split("return 0;\n", "    return 0;\n", true);
    let line = out.lines().next().expect("one row");
    assert!(line.starts_with(RED), "left cell not red-wrapped: {line:?}");
    assert!(
        line.contains(&format!("{RESET} | {GREEN}")),
        "right cell not green-wrapped: {line:?}"
    );
    assert!(line.ends_with(RESET), "row not reset-terminated: {line:?}");
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
    assert!(report.results[0].rendered.is_some());
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
    write_in(&root, "unsupported.rb", "puts 1\n");
    let hidden = write_in(&root, ".hiddendir/dirty2.c", DIRTY_C);
    let expanded = expand_paths(std::slice::from_ref(&root));
    // Hidden dirs are included (style50 parity: --ignore is the filter);
    // unsupported extensions are dropped; result is sorted and unique.
    let mut expected = vec![c, hidden, js, py];
    expected.sort();
    assert_eq!(expanded, expected);
    std::fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn expand_paths_keeps_explicit_unsupported_file() {
    let root = temp_dir("keepunsup");
    let rb = write_in(&root, "bad.rb", "puts 1\n");
    assert_eq!(expand_paths(std::slice::from_ref(&rb)), vec![rb]);
    std::fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn expand_paths_keeps_missing_path() {
    let missing = std::env::temp_dir().join(format!(
        "u50_style_test_{}_gone_dir_missing.c",
        std::process::id()
    ));
    assert_eq!(expand_paths(std::slice::from_ref(&missing)), vec![missing]);
}

#[test]
fn expand_paths_dedupes_dir_and_file_inside() {
    let root = temp_dir("dedupe");
    let c = write_in(&root, "dirty.c", DIRTY_C);
    let py = write_in(&root, "sub/dirty.py", "x = 1\n");
    let expanded = expand_paths(&[root.clone(), c.clone(), root]);
    assert_eq!(expanded, vec![c, py]);
}

#[test]
fn expand_paths_empty_input_is_empty() {
    assert!(expand_paths(&[]).is_empty());
}

#[test]
fn expand_paths_does_not_follow_symlinked_dirs() {
    let root = temp_dir("symlink");
    let other = temp_dir("symlink_target");
    write_in(&other, "other.js", "x = 1;\n");
    #[cfg(unix)]
    {
        let link = root.join("link");
        if std::os::unix::fs::symlink(&other, &link).is_ok() {
            // Inside a walked tree the symlinked dir is neither descended
            // into (os.walk followlinks=false) nor collected as a file.
            assert!(expand_paths(std::slice::from_ref(&root)).is_empty());
            // A symlinked dir passed directly is a non-dir argument: kept
            // unchanged (its per-file error happens downstream).
            assert_eq!(
                expand_paths(std::slice::from_ref(&link)),
                vec![link.clone()]
            );
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn expand_paths_empty_directory_contributes_nothing() {
    let root = temp_dir("emptydir");
    assert!(expand_paths(std::slice::from_ref(&root)).is_empty());
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

    let character = render_character(&source, &formatted, false);
    let char_dels = character.lines().filter(|l| l.starts_with('-')).count();
    let char_adds = character.lines().filter(|l| l.starts_with('+')).count();
    assert_eq!(char_dels, 5000, "character deletions incomplete");
    assert_eq!(char_adds, 5000, "character insertions incomplete");

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
    // Absolute paths are reported even when the file does not exist —
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
