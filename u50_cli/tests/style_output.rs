//! Integration tests for the `u50 style` subcommand's printing, exit
//! codes, and `U50_STYLE_<LANG>` override handling (including the in-place
//! `--fix` path). Each test spawns the real binary with the Python
//! override set to universally available coreutils tools (`cat`, `tr`),
//! so no external formatter is needed and the tests are deterministic.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_u50");

/// Identity override (`cat`): the engine compares the *normalized* source
/// against the formatter's output, and `cat` echoes the normalized source
/// back unchanged — every non-empty file is clean under it.
const CAT: (&str, &str) = ("U50_STYLE_PYTHON", "cat");

/// Byte-changing override (`tr a-z A-Z`; no quoting needed, the argv is
/// split on whitespace): lower-case files are dirty under it, upper-case
/// files are clean.
const UPPER: (&str, &str) = ("U50_STYLE_PYTHON", "tr a-z A-Z");

/// Writes `contents` to a fresh `.py` temp file named after the test
/// (plus the pid, so parallel test binaries never collide).
fn temp_py(test: &str, contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "u50_style_output_{}_{}.py",
        std::process::id(),
        test
    ));
    std::fs::write(&path, contents).expect("write temp file");
    path
}

/// A `.py` temp path that does not exist (for the missing-file error path).
fn missing_py(test: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "u50_style_output_missing_{}_{}.py",
        std::process::id(),
        test
    ));
    let _ = std::fs::remove_file(&path);
    path
}

/// Runs `u50 style <args> --color never` with `(var, value)` set in the
/// child's environment only (per-process, so parallel tests cannot race)
/// and returns `(exit code, stdout, stderr)`.
fn style(args: &[&str], (var, value): (&str, &str)) -> (i32, String, String) {
    let output = Command::new(EXE)
        .args(["style"])
        .args(args)
        .args(["--color", "never"])
        .env(var, value)
        .output()
        .expect("spawn u50");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn read_back(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read temp file back")
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[test]
fn run_mode_dirty_exits_1_and_prints_diff() {
    let path = temp_py("run_dirty", "x = 1\n");
    let args = [path.to_str().expect("utf-8 temp path")];
    let (code, stdout, stderr) = style(&args, UPPER);
    assert_eq!(code, 1, "dirty file must exit 1 (stderr: {stderr})");
    assert!(
        stdout.contains("-x = 1"),
        "diff must show deletion: {stdout}"
    );
    assert!(
        stdout.contains("+X = 1"),
        "diff must show insertion: {stdout}"
    );
    assert!(stderr.is_empty(), "no errors expected: {stderr}");
    cleanup(&path);
}

#[test]
fn run_mode_clean_exits_0_and_prints_nothing() {
    let path = temp_py("run_clean", "x = 1\n");
    let args = [path.to_str().expect("utf-8 temp path")];
    let (code, stdout, stderr) = style(&args, CAT);
    assert_eq!(code, 0, "clean file must exit 0 (stderr: {stderr})");
    assert!(stdout.is_empty(), "clean file prints no diff: {stdout}");
    cleanup(&path);
}

#[test]
fn fix_rewrites_dirty_file_then_reports_already_clean() {
    let path = temp_py("fix", "x = 1\n");
    let p = path.to_str().expect("utf-8 temp path");
    let args = ["--fix", p];

    let (code, stdout, stderr) = style(&args, UPPER);
    assert_eq!(code, 0, "fix of dirty file must exit 0 (stderr: {stderr})");
    assert!(
        stdout.contains(&format!("fixed: {}", path.display())),
        "expected `fixed:` line: {stdout}"
    );
    assert_eq!(read_back(&path), "X = 1\n", "file must be rewritten");

    let (code, stdout, stderr) = style(&args, UPPER);
    assert_eq!(
        code, 0,
        "re-fix of clean file must exit 0 (stderr: {stderr})"
    );
    assert!(
        stdout.contains(&format!("already clean: {}", path.display())),
        "expected `already clean:` line: {stdout}"
    );
    cleanup(&path);
}

#[test]
fn fix_dry_run_prints_diff_but_leaves_file_untouched() {
    let path = temp_py("fix_dry_run", "x = 1\n");
    let p = path.to_str().expect("utf-8 temp path");
    let args = ["--fix", "--dry-run", p];
    let (code, stdout, stderr) = style(&args, UPPER);
    assert_eq!(
        code, 1,
        "dry run with a would-fix must exit 1 (stderr: {stderr})"
    );
    assert!(
        stdout.contains("+X = 1"),
        "dry run must print diff: {stdout}"
    );
    assert_eq!(
        read_back(&path),
        "x = 1\n",
        "dry run must not touch the file"
    );
    cleanup(&path);
}

#[test]
fn fix_honors_override_instead_of_builtin_formatter() {
    // `x=1` is clean under `cat` (the normalized source echoes back
    // unchanged) but NOT under the built-in autopep8, which rewrites it to
    // `x = 1` — so if `fix()` used `default()` instead of `from_env()`,
    // this dry run would exit 1 with a diff. Regression test for the
    // override contract: must exit 0 with NO diff on stdout.
    let path = temp_py("fix_override", "x=1\n");
    let p = path.to_str().expect("utf-8 temp path");
    let args = ["--fix", "--dry-run", p];
    let (code, stdout, stderr) = style(&args, CAT);
    assert_eq!(
        code, 0,
        "clean-under-override must exit 0 (stdout: {stdout}, stderr: {stderr})"
    );
    assert!(stdout.is_empty(), "no diff may be printed: {stdout}");
    assert_eq!(read_back(&path), "x=1\n");
    cleanup(&path);
}

#[test]
fn missing_file_errors_to_stderr_while_valid_file_diff_still_prints() {
    let valid = temp_py("err_valid", "x = 1\n");
    let missing = missing_py("err_missing");
    let args = [
        valid.to_str().expect("utf-8 temp path"),
        missing.to_str().expect("utf-8 temp path"),
    ];
    let (code, stdout, stderr) = style(&args, UPPER);
    assert_eq!(code, 3, "per-file error must exit 3");
    assert!(
        stderr.contains("error:"),
        "stderr must carry the error line: {stderr}"
    );
    assert!(
        stdout.contains("+X = 1"),
        "valid file's diff must survive: {stdout}"
    );
    cleanup(&valid);
}
