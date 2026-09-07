//! Golden fixture tests: u50_style's formatter output vs ground truth.
//!
//! For each language, `fixtures/<lang>/dirty.<ext>` is a deliberately badly
//! formatted input and `fixtures/<lang>/expected.<ext>` is the ground truth
//! produced by style50 3.0.0's own tooling (`style50 -o format dirty.<ext>`),
//! each of which was verified clean by style50 itself (`style50 -o unified
//! expected.<ext>` shows no diff).
//!
//! These tests are GATED: they only run when `U50_STYLE_GOLDEN=1` is set in
//! the environment AND the language's backing formatter binary is
//! resolvable from the u50 style cache (bare tool names are cache-only;
//! the system `PATH` is never consulted).
//! Rationale: the ground truth is only byte-stable for a given set of tool
//! versions, and clang-format in particular varies across machines. CI
//! runs `u50 --setup` (provisioning the exact pinned versions from
//! `tests/tool-versions.txt` — the single source of truth for backend
//! versions — into the u50 style cache) and runs these tests; because
//! resolution is cache-only, a system-PATH tool is never silently used.
//! Without the env var they skip.
//! When regenerating fixtures, use the pinned versions (or refresh
//! `tool-versions.txt` and the goldens together). Run locally with:
//!
//! ```sh
//! U50_STYLE_GOLDEN=1 cargo test --test golden
//! ```

use std::path::PathBuf;

use u50_style::{
    Cs50Formatter, Formatter, Language, Output, Request, builtin_renderer, normalize_source,
    run_with, run_with_renderer,
};

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

/// Whether the engine can resolve `<tool>` — the same cache-only
/// resolution the formatter uses (the u50 style cache installed by
/// `u50 --setup`; the system `PATH` is never consulted), so the
/// gate never skips a language the engine itself would check.
fn tool_available(tool: &str) -> bool {
    u50_style::locate_tool(tool).is_some()
}

/// Whether the golden test for `dir` should run: requires `U50_STYLE_GOLDEN=1`
/// and the language's backing tool on PATH; prints a skip line otherwise.
fn gate(dir: &str, tool: &str) -> bool {
    if std::env::var("U50_STYLE_GOLDEN").as_deref() != Ok("1") {
        eprintln!("skip {dir} golden: U50_STYLE_GOLDEN is not set to 1");
        return false;
    }
    if !tool_available(tool) {
        eprintln!("skip {dir} golden: `{tool}` not available in the u50 style cache");
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
    let formatted = Cs50Formatter
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

/// In-memory `Write` sink for renderer output: an `Rc`-shared buffer so a
/// clone can be owned by the renderer's `Box<dyn Write>` and the bytes
/// read back afterwards.
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

/// Runs score mode (`Output::Score`) over `<dir>/dirty.<ext>` with the real
/// formatter and returns the rendered bytes; `None` when the test is gated
/// off (same gate as the golden format tests).
fn score_of(dir: &str, ext: &str, tool: &str) -> Option<String> {
    if !gate(dir, tool) {
        return None;
    }
    let req = Request {
        files: vec![fixture(dir, &format!("dirty.{ext}"))],
        output: Output::Score,
        color: false,
    };
    let sink = SharedBuf::default();
    let mut renderer = builtin_renderer(Output::Score, false, Box::new(sink.clone()));
    let report = run_with_renderer(&req, &Cs50Formatter, renderer.as_mut());
    assert!(
        report.errors.is_empty(),
        "{dir}: errors {:?}",
        report.errors
    );
    Some(String::from_utf8(sink.0.borrow().clone()).expect("utf8 score"))
}

/// Verified live against style50 3.0.0 (`STYLE50_V3_CROSSCHECK.md` §5):
/// `style50 -o score fixtures/py/dirty.py` → `0.9814814814814815`
/// (diffs = 15.0; `Python.count_lines` counts ALL 810 lines — blank lines
/// matter per PEP 8).
#[test]
fn py_dirty_score_matches_style50() {
    let Some(score) = score_of("py", "py", "autopep8") else {
        return; // gated off (no U50_STYLE_GOLDEN / autopep8 not cached)
    };
    assert_eq!(score, "0.9814814814814815\n");
}

/// Verified live against style50 3.0.0 (`STYLE50_V3_CROSSCHECK.md` §5):
/// `style50 -o score fixtures/c/dirty.c` → `0.5036334275333064` (C counts
/// non-blank lines; the denominator for non-Python languages is unchanged).
#[test]
fn c_dirty_score_matches_style50() {
    let Some(score) = score_of("c", "c", "clang-format") else {
        return; // gated off (no U50_STYLE_GOLDEN / clang-format not cached)
    };
    assert_eq!(score, "0.5036334275333064\n");
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
        let report = run_with(&req, &Cs50Formatter);
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
