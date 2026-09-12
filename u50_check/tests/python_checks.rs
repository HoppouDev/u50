//! End-to-end tests for legacy Python checks (`__init__.py`-style
//! check sets, Phases 0-2 of docs/U50_CHECK_PYTHON_PLAN.md): the
//! provisioned CPython, the shipped `check50` package, and the process
//! bridge, through the same engine YAML/native checks use.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command as OsCommand;

/// Creates a check package: a `.cs50.yaml` naming `__init__.py`, the
/// checks module itself, and (optionally) a `hello.sh` student program
/// printing `Hello, world!`.
fn write_package(name: &str, checks_module: &str, with_hello: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u50-python-checks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create package dir");
    std::fs::write(dir.join(".cs50.yaml"), "check50:\n  checks: __init__.py\n")
        .expect("write .cs50.yaml");
    std::fs::write(dir.join("__init__.py"), checks_module).expect("write checks module");
    if with_hello {
        std::fs::write(dir.join("hello.sh"), "#!/bin/sh\necho 'Hello, world!'\n")
            .expect("write hello.sh");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let hello = dir.join("hello.sh");
            let mut permissions = std::fs::metadata(&hello).expect("stat").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&hello, permissions).expect("chmod");
        }
    }
    dir
}

/// Runs `u50_check::run` on the package and returns the parsed JSON
/// document (offline mode, JSON output to a file).
fn run_u50_check(dir: &Path) -> Value {
    let out_file = dir.join("u50-actual.json");
    let request = u50_check::Request {
        slug: dir.display().to_string(),
        work_dir: Some(dir.to_path_buf()),
        mode: u50_check::Mode::Offline,
        targets: Vec::new(),
        outputs: vec![u50_check::Output::Json],
        output_file: Some(out_file.clone()),
        verbose: false,
        show_log: false,
        log_level: None,
    };
    if let Err(error) = u50_check::run(&request) {
        panic!("u50_check::run failed for {}: {error:#}", dir.display());
    }
    serde_json::from_str(&std::fs::read_to_string(&out_file).expect("read output"))
        .expect("valid json")
}

fn results(document: &Value) -> &Value {
    document
        .get("results")
        .unwrap_or_else(|| panic!("no results in {document}"))
}

#[test]
fn hello_world_matches_the_documented_shape() {
    let module = r#"
import check50


@check50.check()
def exists():
    """hello.sh exists"""
    check50.exists("hello.sh")


@check50.check(exists)
def prints_hello():
    """prints hello"""
    check50.run("./hello.sh").stdout("[Hh]ello, world!\\n").exit(0)
"#;
    let dir = write_package("hello", module, true);
    let document = run_u50_check(&dir);
    let expected: Value = serde_json::from_str(
        r#"[
            {"name":"exists","description":"hello.sh exists","passed":true,
             "log":["checking that hello.sh exists..."],"cause":null,"data":{},"dependency":null},
            {"name":"prints_hello","description":"prints hello","passed":true,
             "log":["running ./hello.sh...","checking for output \"[Hh]ello, world!\\n\"...",
                     "checking that program exited with status 0..."],
             "cause":null,"data":{},"dependency":"exists"}
        ]"#,
    )
    .expect("embedded json");
    assert_eq!(results(&document), &expected);
}

#[test]
fn failures_and_skips_cascade_like_native_checks() {
    let module = r#"
import check50


@check50.check()
def fails():
    """fails"""
    raise check50.Failure("deliberate failure")


@check50.check(fails)
def dependent():
    """dependent"""
"#;
    let dir = write_package("cascade", module, false);
    let document = run_u50_check(&dir);
    let results = results(&document);
    assert_eq!(results[0]["passed"], Value::Bool(false));
    assert_eq!(
        results[0]["cause"]["rationale"],
        Value::String("deliberate failure".into())
    );
    assert_eq!(results[1]["passed"], Value::Null);
    assert_eq!(
        results[1]["cause"]["rationale"],
        Value::String("can't check until a frown turns upside down".into())
    );
}

#[test]
fn dependency_state_is_passed_to_dependents() {
    let module = r#"
import check50


@check50.check()
def gives():
    """gives the state"""
    return "state-value"


@check50.check(gives)
def takes(state):
    """takes the state"""
    if state != "state-value":
        raise check50.Failure(f"bad state: {state!r}")
"#;
    let dir = write_package("state", module, false);
    let document = run_u50_check(&dir);
    for result in results(&document).as_array().expect("array") {
        assert_eq!(result["passed"], Value::Bool(true), "{result}");
    }
}

#[test]
fn output_mismatch_carries_expected_and_actual() {
    let module = r#"
import check50


@check50.check()
def wrong():
    """wrong output"""
    check50.run("echo hello").stdout("goodbye", regex=False)
"#;
    let dir = write_package("mismatch", module, false);
    let document = run_u50_check(&dir);
    let cause = &results(&document)[0]["cause"];
    assert_eq!(
        cause["rationale"],
        Value::String("expected \"goodbye\", not \"hello\\n\"".into())
    );
    assert_eq!(cause["expected"], Value::String("goodbye".into()));
    assert_eq!(cause["actual"], Value::String("hello\n".into()));
}

#[test]
fn a_check_that_overruns_its_timeout_is_killed_and_fails() {
    let module = r#"
import time

import check50


@check50.check(timeout=1)
def slow():
    """sleeps"""
    time.sleep(30)
"#;
    let dir = write_package("timeout", module, false);
    let document = run_u50_check(&dir);
    let result = &results(&document)[0];
    assert_eq!(result["passed"], Value::Bool(false));
    assert!(
        result["cause"]["rationale"]
            .as_str()
            .expect("rationale")
            .contains("timed out"),
        "{result}"
    );
}

#[test]
fn the_interpreter_resolves_idempotently_from_the_cache() {
    let first = u50_check::python::venv::interpreter().expect("interpreter");
    assert!(first.is_file(), "{} must exist", first.display());
    let second = u50_check::python::venv::interpreter().expect("interpreter");
    assert_eq!(first, second);
}

#[test]
fn live_check50_matches_u50_check_when_installed() {
    // Gated live cross-check: run real check50 on the hello package and
    // compare its results array against ours (skipped when check50 is
    // not installed, like cross_check.rs's live mode).
    let probe = OsCommand::new("bash")
        .args(["-c", "command -v check50"])
        .output()
        .expect("probe");
    if !probe.status.success() {
        eprintln!("skip live cross-check: check50 is not installed");
        return;
    }
    let module = r##"
import check50


@check50.check()
def exists():
    """hello.sh exists"""
    check50.exists("hello.sh")


@check50.check(exists)
def prints_hello():
    """prints hello"""
    check50.run("./hello.sh").stdout("[Hh]ello, world!\n").exit(0)
"##;
    let dir = write_package("live", module, true);
    let ours = run_u50_check(&dir);
    let theirs = run_real_check50(&dir);
    let Some(theirs) = theirs else {
        eprintln!("skip: check50 produced no output");
        return;
    };
    assert_eq!(results(&ours), results(&theirs));
}

/// Runs real check50 on the package and returns the parsed JSON
/// document (`None` when check50 is unusable).
fn run_real_check50(dir: &Path) -> Option<Value> {
    let out_file = dir.join("check50-actual.json");
    let status = OsCommand::new("python3")
        .args([
            "-m",
            "check50",
            dir.display().to_string().as_str(),
            "--offline",
            "-o",
            "json",
            "--output-file",
            out_file.display().to_string().as_str(),
        ])
        .env_remove("CHECK50_PATH")
        .output()
        .expect("run check50");
    if !status.status.success() {
        return None;
    }
    let raw = std::fs::read_to_string(&out_file).ok()?;
    serde_json::from_str(&raw).ok()
}
