use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::formatter::{overrides_from_env, run_tool};
use crate::render::json_document;

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
        ("a.cc", Some(Language::Cpp)),
        ("a.cxx", Some(Language::Cpp)),
        ("a.java", Some(Language::Java)),
        ("a.py", Some(Language::Python)),
        ("a.js", Some(Language::JavaScript)),
        ("a.html", None),
        ("a.css", None),
        ("a.sql", None),
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
fn run_with_empty_js_file_is_clean() {
    let path = temp_file("empty.js", "");
    let req = Request {
        files: vec![path.clone()],
        output: Output::Unified,
        color: false,
    };
    let report = run_with(&req, &Cs50Formatter::default()).expect("run succeeds");
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
    let path = temp_file("bad.sql", "select 1;\n");
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
