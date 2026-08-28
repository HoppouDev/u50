use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::formatter::{overrides_from_env, run_tool};
use crate::render::{BOLD, GREEN, RED, RESET, json_document, render_character, render_split};

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
fn overrides_from_env_parses_lang_keys_and_splits_whitespace() {
    let vars = |name: &str| match name {
        "U50_STYLE_PYTHON" => Some("ruff format -".to_owned()),
        "U50_STYLE_JAVA" => Some("google-java-format -".to_owned()),
        _ => None,
    };
    let overrides = overrides_from_env(vars);
    assert_eq!(overrides.len(), 2);
    assert_eq!(
        overrides.get(&Language::Python).expect("python override"),
        &["ruff".to_owned(), "format".to_owned(), "-".to_owned()]
    );
    assert_eq!(
        overrides.get(&Language::Java).expect("java override"),
        &["google-java-format".to_owned(), "-".to_owned()]
    );
}

#[test]
fn overrides_from_env_ignores_empty_and_unknown_vars() {
    let vars = |name: &str| match name {
        "U50_STYLE_CPP" => Some("   ".to_owned()),
        "U50_STYLE_JAVASCRIPT" => Some(String::new()),
        "U50_STYLE_RUBY" => Some("rubocop -".to_owned()),
        _ => None,
    };
    assert!(overrides_from_env(vars).is_empty());
}

#[test]
fn override_routes_to_custom_tool_via_stdin() {
    let mut overrides = HashMap::new();
    overrides.insert(Language::Python, vec!["cat".to_owned()]);
    let formatter = Cs50Formatter::with_overrides(overrides);
    assert_eq!(
        formatter
            .format("x = 1\n", Language::Python)
            .expect("cat succeeds"),
        "x = 1\n"
    );
}

#[test]
fn override_spawn_failure_names_var_and_binary() {
    let mut overrides = HashMap::new();
    overrides.insert(
        Language::Python,
        vec!["definitely-not-a-real-u50-tool".to_owned()],
    );
    let formatter = Cs50Formatter::with_overrides(overrides);
    let err = formatter
        .format("x = 1\n", Language::Python)
        .expect_err("errors");
    let msg = err.to_string();
    assert!(msg.contains("U50_STYLE_PYTHON"), "got: {msg}");
    assert!(msg.contains("definitely-not-a-real-u50-tool"), "got: {msg}");
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
    };
    let clean = FileResult {
        path: PathBuf::from("clean.c"),
        clean: true,
        rendered: None,
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
        assert_eq!(
            Cs50Formatter::default().format("", language).expect("ok"),
            ""
        );
        assert_eq!(
            Cs50Formatter::default()
                .format("\n  ", language)
                .expect("ok"),
            "\n  "
        );
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
