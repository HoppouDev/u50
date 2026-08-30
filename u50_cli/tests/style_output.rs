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
fn fix_dry_run_prints_status_lines_and_leaves_files_untouched() {
    let dirty = temp_py("fix_dry_run", "x = 1\n");
    let clean = temp_py("fix_dry_run_clean", "X = 1\n");
    let args = [
        "--fix",
        "--dry-run",
        dirty.to_str().expect("utf-8 temp path"),
        clean.to_str().expect("utf-8 temp path"),
    ];
    let (code, stdout, stderr) = style(&args, UPPER);
    assert_eq!(
        code, 1,
        "dry run with a would-fix must exit 1 (stderr: {stderr})"
    );
    // Dry run reports per-file status (no diff since 12545d8): the label
    // is `would fix` because nothing was written.
    assert!(
        stdout.contains(&format!("would fix: {}", dirty.display())),
        "expected `would fix:` status line: {stdout}"
    );
    assert!(
        stdout.contains(&format!("already clean: {}", clean.display())),
        "expected `already clean:` line: {stdout}"
    );
    assert_eq!(
        read_back(&dirty),
        "x = 1\n",
        "dry run must not touch the dirty file"
    );
    assert_eq!(
        read_back(&clean),
        "X = 1\n",
        "dry run must not touch the clean file"
    );
    cleanup(&dirty);
    cleanup(&clean);
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
    // Status lines are printed in dry-run text mode too, but no diff.
    assert!(
        stdout.contains(&format!("already clean: {}", path.display())),
        "expected `already clean:` line: {stdout}"
    );
    assert!(
        !stdout.contains("-x=1") && !stdout.contains('+'),
        "no diff may be printed: {stdout}"
    );
    assert_eq!(read_back(&path), "x=1\n");
    cleanup(&path);
}

#[test]
fn cache_backends_spawn_when_path_lacks_the_formatter() {
    use std::os::unix::fs::PermissionsExt;

    // Exercises the cache-spawn branch deterministically, with no network:
    // bare tool names resolve cache-only, so a hostile `autopep8` on PATH
    // (one that fails loudly if it were ever spawned) plus a populated
    // cache proves the cached backend wins over anything on PATH.
    let root = std::env::temp_dir().join(format!(
        "u50_style_output_cache_spawn_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create root");

    // Restricted PATH: `cat`, plus a HOSTILE `autopep8` that fails loudly
    // (`FAKE` on stderr) if it were ever spawned — it must never win.
    let bins = root.join("bins");
    std::fs::create_dir_all(&bins).expect("create bins");
    std::os::unix::fs::symlink("/usr/bin/cat", bins.join("cat")).expect("symlink cat");
    let fake = bins.join("autopep8");
    std::fs::write(&fake, "#!/bin/sh\necho FAKE >&2\nexit 42\n").expect("write fake");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake executable");

    // A fake `--setup`-populated cache: `venv/bin/autopep8` as an
    // identity stub (`cat`), mirroring the uv-managed venv layout.
    let cache = root.join("cache");
    let stub = cache.join("u50/style50/venv/bin/autopep8");
    std::fs::create_dir_all(stub.parent().expect("cache bin parent")).expect("create cache bin");
    std::fs::write(&stub, "#!/bin/sh\ncat\n").expect("write stub");
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
        .expect("chmod stub executable");

    // Trailing whitespace: normalization strips it and the identity
    // formatter echoes the normalized source back unchanged, so the file
    // is CLEAN under the stub — the `already clean` outcome that proves
    // the cached formatter spawned and ran without error.
    let path = temp_py("cache_spawn", "x = 1   \n");
    let p = path.to_str().expect("utf-8 temp path");

    // `U50_STYLE_NO_PROVISION` keeps the runs hermetic: without it the
    // empty-cache control would attempt a real network auto-provision.
    let run = |cache: &Path| {
        let out = Command::new(EXE)
            .args(["style", "--fix", "--dry-run", p])
            .args(["--color", "never"])
            .env("XDG_CACHE_HOME", cache)
            .env("PATH", &bins)
            .env("U50_STYLE_NO_PROVISION", "1")
            .env_remove("U50_STYLE_PYTHON")
            .output()
            .expect("spawn u50");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };

    // Populated cache: the cached stub is used even though a hostile
    // `autopep8` sits on PATH — PATH is never consulted.
    let (code, stdout, stderr) = run(&cache);
    assert_eq!(
        code, 0,
        "cached identity formatter must find the file clean (stdout: {stdout}, stderr: {stderr})"
    );
    assert!(
        stdout.contains(&format!("already clean: {}", path.display())),
        "expected `already clean:` line: {stdout}"
    );
    assert!(
        !stderr.contains("FAKE"),
        "the PATH fake must never be spawned: {stderr}"
    );

    // Control: the SAME hostile PATH with an EMPTY cache — the tool is
    // nowhere to be found (provisioning disabled), so the per-file error
    // naming the backend fires (exit 3). The fake's loud failure (FAKE,
    // exit 42) must not leak into the result: PATH was ignored.
    let empty = root.join("empty-cache");
    std::fs::create_dir_all(&empty).expect("create empty cache");
    let (code, stdout, stderr) = run(&empty);
    assert_eq!(code, 3, "missing backend must exit 3 (stdout: {stdout})");
    assert!(
        stderr.contains("autopep8"),
        "error must name the missing backend: {stderr}"
    );
    assert!(
        !stderr.contains("FAKE"),
        "the PATH fake must never be spawned: {stderr}"
    );

    cleanup(&path);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn path_fake_is_never_used_for_formatter_tools() {
    use std::os::unix::fs::PermissionsExt;

    // Cache-only resolution, end to end: a scratch cache, a trivial py
    // file, and a PATH whose `autopep8` is a fake that would report the
    // file as "already clean" if it were ever used. The run must NOT go
    // through the fake: with provisioning disabled it errors on the real
    // missing cache backend (exit 3) instead of the fake's clean exit 0.
    let root =
        std::env::temp_dir().join(format!("u50_style_output_path_fake_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bins = root.join("bins");
    std::fs::create_dir_all(&bins).expect("create bins");
    let fake = bins.join("autopep8");
    std::fs::write(&fake, "#!/bin/sh\ncat\n").expect("write fake");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake executable");

    let scratch_cache = root.join("cache");
    std::fs::create_dir_all(&scratch_cache).expect("create scratch cache");
    let path = temp_py("path_fake", "x = 1\n");
    let p = path.to_str().expect("utf-8 temp path");

    let out = Command::new(EXE)
        .args(["style", "--fix", "--dry-run", p])
        .args(["--color", "never"])
        .env("XDG_CACHE_HOME", &scratch_cache)
        .env("PATH", &bins)
        .env("U50_STYLE_NO_PROVISION", "1")
        .env_remove("U50_STYLE_PYTHON")
        .output()
        .expect("spawn u50");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    assert_ne!(
        code, 0,
        "the PATH fake's clean outcome must not happen (stdout: {stdout})"
    );
    assert!(
        !stdout.contains("already clean"),
        "the PATH fake must never win: {stdout}"
    );
    assert_eq!(
        code, 3,
        "cache-only miss must produce the per-file error exit 3 (stderr: {stderr})"
    );
    assert!(
        stderr.contains("autopep8"),
        "error must name the real backend, not use the fake: {stderr}"
    );

    cleanup(&path);
    let _ = std::fs::remove_dir_all(&root);
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

/// The six cache backends `--setup` checks (the required tools of the
/// fixed language set; C/C++/Java share `clang-format`).
const BACKEND_TOOLS: [&str; 6] = [
    "clang-format",
    "autopep8",
    "js-beautify",
    "djhtml",
    "css-beautify",
    "sqlformat",
];

#[test]
fn setup_succeeds_when_every_backend_is_already_cached() {
    use std::os::unix::fs::PermissionsExt;

    // The root-level `--setup` success path, hermetically: a scratch
    // XDG_CACHE_HOME
    // pre-seeded with the six fake backend scripts in the venv bin dir
    // (same stub pattern as the cache-spawn test) makes every backend
    // resolve from the cache, so `--setup` must be a no-op that prints
    // the already-available line and exits 0 — no network, no install.
    let root =
        std::env::temp_dir().join(format!("u50_style_output_setup_ok_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let cache = root.join("cache");
    let bin = cache.join("u50/style50/venv/bin");
    std::fs::create_dir_all(&bin).expect("create cache bin");
    for tool in BACKEND_TOOLS {
        let stub = bin.join(tool);
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").expect("write stub");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("chmod stub executable");
    }

    let out = Command::new(EXE)
        .args(["--setup"])
        .args(["--color", "never"])
        .env("XDG_CACHE_HOME", &cache)
        .env("U50_STYLE_NO_PROVISION", "1")
        .output()
        .expect("spawn u50");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    assert_eq!(
        code, 0,
        "--setup with a full cache must exit 0 (stdout: {stdout}, stderr: {stderr})"
    );
    assert!(
        stdout.contains("all formatter backends are already available"),
        "expected the already-available line: {stdout}"
    );
    assert!(
        !stdout.contains("installed:"),
        "nothing may be installed: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn list_reports_missing_for_every_backend_when_cache_is_empty() {
    // `--list` with a scratch EMPTY cache, provisioning disabled, and a
    // formatter-free PATH: every language row must report `missing` (a
    // listing is not an error, so exit 0), and no row may claim a cache
    // hit. Deterministic regardless of the host's installed formatters.
    let root = std::env::temp_dir().join(format!(
        "u50_style_output_list_missing_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let cache = root.join("cache");
    std::fs::create_dir_all(&cache).expect("create empty cache");
    let bins = root.join("bins");
    std::fs::create_dir_all(&bins).expect("create empty bins");
    assert!(!BACKEND_TOOLS.iter().any(|tool| bins.join(tool).exists()));

    let out = Command::new(EXE)
        .args(["style", "--list"])
        .args(["--color", "never"])
        .env("XDG_CACHE_HOME", &cache)
        .env("PATH", &bins)
        .env("U50_STYLE_NO_PROVISION", "1")
        .output()
        .expect("spawn u50");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

    assert_eq!(code, 0, "--list must exit 0");
    assert!(
        stdout.contains("missing"),
        "rows must show the missing status: {stdout}"
    );
    assert_eq!(
        stdout.matches("missing").count(),
        8,
        "one missing row per language: {stdout}"
    );
    assert!(
        !stdout.contains("found (cache)"),
        "no row may claim a cache hit: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
