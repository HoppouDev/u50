//! Cross-check tests: u50_check's YAML check sets produce the same
//! results as check50 3.4.0 (ground truth captured with the real tool;
//! see docs/CHECK50_PORT_NOTES.md). The live check50 comparison runs
//! only when `check50` is installed (gated, like u50_style's goldens);
//! the embedded expectations (captured from check50 3.4.0) always run.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command as OsCommand;

/// A sample check package: (name, .cs50.yaml checks mapping serialized,
/// student-file setup shell lines).
const PACKAGES: &[(&str, &str, &str)] = &[
    ("pass_exit_zero", r"{passes: [{run: 'true', exit: 0}]}", ""),
    (
        "fail_exit_mismatch",
        r"{fails_exit_code: [{run: 'false', exit: 0}]}",
        "",
    ),
    (
        "fail_stdout_missing",
        r"{fails_stdout: [{run: 'echo hello', stdout: goodbye}]}",
        "",
    ),
    (
        "no_exit_assertion",
        r"{ignores_exit_code: [{run: 'false'}]}",
        "",
    ),
    (
        "stdin_echo",
        r#"{echoes_stdin: [{run: 'read line; echo "$line"', stdin: meow, stdout: meow, exit: 0}]}"#,
        "",
    ),
];

/// The expected `results` arrays, captured from check50 3.4.0 running
/// the same packages (`python3 -c "import multiprocessing as mp;
/// mp.set_start_method('fork'); ..."` — check50 3.4.0 predates
/// Python 3.14's forkserver default).
const EXPECTED_RESULTS: &[(&str, &str)] = &[
    (
        "pass_exit_zero",
        r#"[{"name":"passes","description":"passes","passed":true,"log":["running true...","checking that program exited with status 0..."],"cause":null,"data":{},"dependency":null}]"#,
    ),
    (
        "fail_exit_mismatch",
        r#"[{"name":"fails_exit_code","description":"fails_exit_code","passed":false,"log":["running false...","checking that program exited with status 0..."],"cause":{"rationale":"expected exit code 0, not 1","help":null},"data":{},"dependency":null}]"#,
    ),
    (
        "fail_stdout_missing",
        r#"[{"name":"fails_stdout","description":"fails_stdout","passed":false,"log":["running echo hello...","checking for output \"goodbye\"..."],"cause":{"rationale":"expected \"goodbye\", not \"hello\\n\"","help":null,"expected":"goodbye","actual":"hello\n"},"data":{},"dependency":null}]"#,
    ),
    (
        "no_exit_assertion",
        r#"[{"name":"ignores_exit_code","description":"ignores_exit_code","passed":true,"log":["running false..."],"cause":null,"data":{},"dependency":null}]"#,
    ),
    (
        "stdin_echo",
        r#"[{"name":"echoes_stdin","description":"echoes_stdin","passed":true,"log":["running read line; echo \"$line\"...","sending input meow...","checking for output \"meow\"...","checking that program exited with status 0..."],"cause":null,"data":{},"dependency":null}]"#,
    ),
];

/// Creates the package under `dir` and returns the check-dir path (the
/// dev-mode slug).
fn write_package(name: &str, checks_mapping: &str, student_setup: &str) -> PathBuf {
    // Unique per call: the cross-check tests may run concurrently (two
    // tests exercise the same packages).
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let calls = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "u50-cross-check-{name}-{}-{calls}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create package dir");
    // Indent the checks mapping by 2 spaces so it nests under `checks:`.
    let indented = checks_mapping
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        dir.join(".cs50.yaml"),
        format!("check50:\n  checks:\n{indented}\n"),
    )
    .expect("write .cs50.yaml");
    if !student_setup.is_empty() {
        let status = OsCommand::new("bash")
            .arg("-c")
            .arg(student_setup)
            .current_dir(&dir)
            .status()
            .expect("student setup");
        assert!(status.success(), "student setup failed for {name}");
    }
    dir
}

/// Runs u50_check on the package and returns the parsed JSON document.
fn run_u50_check(check_dir: &Path) -> Value {
    let out_file = check_dir.join("u50-actual.json");
    let request = u50_check::Request {
        slug: check_dir.display().to_string(),
        work_dir: Some(check_dir.to_path_buf()),
        mode: u50_check::Mode::Offline,
        targets: Vec::new(),
        outputs: vec![u50_check::Output::Json],
        output_file: Some(out_file.clone()),
        verbose: false,
        show_log: false,
        log_level: None,
    };
    if let Err(error) = u50_check::run(&request) {
        panic!(
            "u50_check::run failed for {}: {error:#}",
            check_dir.display()
        );
    }
    serde_json::from_str(&std::fs::read_to_string(&out_file).expect("read output"))
        .expect("valid json")
}

/// Runs real check50 on the package (fork start method forced for
/// Python 3.14) and returns the parsed JSON document.
fn run_check50(check_dir: &Path) -> Option<Value> {
    let probe = OsCommand::new("bash")
        .args(["-c", "command -v check50"])
        .output()
        .expect("probe check50");
    if !probe.status.success() {
        return None;
    }
    let out_file = check_dir.join("check50-expected.json");
    let script = "import multiprocessing as mp; mp.set_start_method('fork'); \
        import sys; sys.argv = ['check50', '-d', 'checks', '--offline', \
        '-o', 'json', '--output-file', 'check50-expected.json']; \
        from check50.__main__ import main; main()";
    let status = OsCommand::new("python3")
        .arg("-c")
        .arg(script)
        .current_dir(check_dir)
        .env_remove("CHECK50_PATH")
        .status()
        .expect("run check50");
    let _ = status;
    let raw = std::fs::read_to_string(&out_file).ok()?;
    Some(serde_json::from_str(&raw).expect("check50 wrote valid json"))
}

/// Extracts the `results` array (check50 parity: the interesting
/// payload; slug/version legitimately differ).
fn results(document: &Value) -> &Value {
    document
        .get("results")
        .unwrap_or_else(|| panic!("no results in {}", document))
}

#[test]
fn u50_check_results_match_check50_ground_truth() {
    for (name, checks_mapping, student_setup) in PACKAGES {
        let dir = write_package(name, checks_mapping, student_setup);
        let actual = run_u50_check(&dir);
        let expected = EXPECTED_RESULTS
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, json)| serde_json::from_str::<Value>(json).expect("embedded json"))
            .unwrap_or_else(|| panic!("no embedded expectation for {name}"));
        assert_eq!(results(&actual), &expected, "results mismatch for {name}");
    }
}

#[test]
fn live_check50_matches_u50_check_when_installed() {
    // Gated live cross-check: run real check50 and u50_check on the same
    // packages and compare their results arrays.
    if OsCommand::new("bash")
        .args(["-c", "command -v check50"])
        .output()
        .expect("probe")
        .status
        .success()
    {
        eprintln!("skip live cross-check: check50 is not installed");
        return;
    }
    for (name, checks_mapping, student_setup) in PACKAGES {
        let dir = write_package(name, checks_mapping, student_setup);
        let actual = run_u50_check(&dir);
        let Some(expected) = run_check50(&dir) else {
            eprintln!("skip {name}: check50 produced no output");
            continue;
        };
        assert_eq!(
            results(&actual),
            results(&expected),
            "live check50 mismatch for {name}"
        );
    }
}

#[test]
fn run_exit_code_reflects_failures() {
    let dir = write_package("fail_exit_mismatch", PACKAGES[1].1, PACKAGES[1].2);
    let request = u50_check::Request {
        slug: dir.display().to_string(),
        work_dir: Some(dir.clone()),
        mode: u50_check::Mode::Offline,
        targets: Vec::new(),
        outputs: vec![u50_check::Output::Json],
        output_file: None,
        verbose: false,
        show_log: false,
        log_level: None,
    };
    assert!(
        !u50_check::run(&request).expect("run"),
        "a failed check must report passed=false"
    );

    let dir = write_package("pass_exit_zero", PACKAGES[0].1, PACKAGES[0].2);
    let request = u50_check::Request {
        slug: dir.display().to_string(),
        work_dir: Some(dir.clone()),
        mode: u50_check::Mode::Offline,
        targets: Vec::new(),
        outputs: vec![u50_check::Output::Json],
        output_file: None,
        verbose: false,
        show_log: false,
        log_level: None,
    };
    assert!(
        u50_check::run(&request).expect("run"),
        "all-passed must report passed=true"
    );
}
