//! Golden fixture tests: u50_style's formatter output vs ground truth.
//!
//! For each language, `fixtures/<lang>/dirty.<ext>` is a deliberately badly
//! formatted input and `fixtures/<lang>/expected.<ext>` is the ground truth
//! produced by style50 3.0.0's own tooling (`style50 -o format dirty.<ext>`),
//! each of which was verified clean by style50 itself (`style50 -o unified
//! expected.<ext>` shows no diff).
//!
//! These tests are GATED: they only run when `U50_STYLE_GOLDEN=1` is set in
//! the environment AND the language's backing formatter binary is on PATH.
//! Rationale: the ground truth is only byte-stable for a given set of tool
//! versions, and clang-format in particular varies across machines (version
//! skew would flake CI); CI therefore runs without `U50_STYLE_GOLDEN` and
//! skips these tests. Run locally with:
//!
//! ```sh
//! U50_STYLE_GOLDEN=1 cargo test --test golden
//! ```

use std::path::PathBuf;
use std::process::Command;

use u50_style::{Cs50Formatter, Formatter, Language, Output, Request, normalize_source, run_with};

/// (directory under `tests/fixtures`, file extension, language, backing tool).
const LANGUAGES: &[(&str, &str, Language, &str)] = &[
    ("c", "c", Language::C, "clang-format"),
    ("cpp", "cpp", Language::Cpp, "clang-format"),
    ("java", "java", Language::Java, "clang-format"),
    ("py", "py", Language::Python, "autopep8"),
    ("js", "js", Language::JavaScript, "js-beautify"),
    ("html", "html", Language::Html, "djhtml"),
    ("css", "css", Language::Css, "css-beautify"),
    ("sql", "sql", Language::Sql, "sqlformat"),
];

/// Whether `<tool> --version` can be spawned successfully.
fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Whether the golden test for `dir` should run: requires `U50_STYLE_GOLDEN=1`
/// and the language's backing tool on PATH; prints a skip line otherwise.
fn gate(dir: &str, tool: &str) -> bool {
    if std::env::var("U50_STYLE_GOLDEN").as_deref() != Ok("1") {
        eprintln!("skip {dir} golden: U50_STYLE_GOLDEN is not set to 1");
        return false;
    }
    if !tool_available(tool) {
        eprintln!("skip {dir} golden: `{tool}` not available on PATH");
        return false;
    }
    true
}

fn fixture(dir: &str, file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(dir)
        .join(file)
}

fn run_golden(dir: &str, ext: &str, language: Language, tool: &str) {
    if !gate(dir, tool) {
        return;
    }
    let dirty = std::fs::read_to_string(fixture(dir, &format!("dirty.{ext}")))
        .unwrap_or_else(|e| panic!("read dirty fixture {dir}: {e}"));
    let normalized = normalize_source(&dirty);
    let formatted = Cs50Formatter::default()
        .format(&normalized, language)
        .unwrap_or_else(|e| panic!("format {dir}: {e}"));
    let expected = std::fs::read_to_string(fixture(dir, &format!("expected.{ext}")))
        .unwrap_or_else(|e| panic!("read expected fixture {dir}: {e}"));
    assert_ne!(
        dirty, expected,
        "vacuous golden fixture for {dir}: dirty == expected; regenerate dirty \
         from the pre-first-pass input (see AGENTS.md golden fixture section)"
    );
    assert_eq!(
        formatted, expected,
        "formatter output differs from style50 3.0.0 ground truth for {dir}\n\
         --- formatted ---\n{formatted}\n--- expected (style50 -o format) ---\n{expected}"
    );
    eprintln!("PASS {dir} golden");
}

macro_rules! golden_test {
    ($name:ident, $dir:literal, $ext:literal, $language:expr, $tool:literal) => {
        #[test]
        fn $name() {
            run_golden($dir, $ext, $language, $tool);
        }
    };
}

golden_test!(c_golden, "c", "c", Language::C, "clang-format");
golden_test!(cpp_golden, "cpp", "cpp", Language::Cpp, "clang-format");
golden_test!(java_golden, "java", "java", Language::Java, "clang-format");
golden_test!(python_golden, "py", "py", Language::Python, "autopep8");
golden_test!(js_golden, "js", "js", Language::JavaScript, "js-beautify");
golden_test!(html_golden, "html", "html", Language::Html, "djhtml");
golden_test!(css_golden, "css", "css", Language::Css, "css-beautify");
golden_test!(sql_golden, "sql", "sql", Language::Sql, "sqlformat");

/// Every expected fixture must itself be clean per u50's own engine (the same
/// property style50 verified during generation); gated per language.
#[test]
fn expected_files_are_clean() {
    for (dir, ext, _language, tool) in LANGUAGES {
        if !gate(dir, tool) {
            continue;
        }
        let req = Request {
            files: vec![fixture(dir, &format!("expected.{ext}"))],
            output: Output::Json,
            color: false,
        };
        let report = run_with(&req, &Cs50Formatter::default());
        assert!(
            report.errors.is_empty(),
            "{}: errors {:?}",
            dir,
            report.errors
        );
        assert!(report.clean(), "{dir} expected fixture is not clean");
        assert!(
            report.results[0].clean,
            "{dir} expected fixture is not clean"
        );
        eprintln!("PASS {dir} expected fixture is clean per u50_style");
    }
}
