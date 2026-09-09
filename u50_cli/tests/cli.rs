//! End-to-end CLI tests (spawns the real binary).

use std::process::Command;

#[test]
fn status_prints_language_table() {
    let output = Command::new(env!("CARGO_BIN_EXE_u50"))
        .args(["--status"])
        .output()
        .expect("failed to spawn u50");
    assert!(output.status.success(), "--status must exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Header and separator structure.
    for column in ["Language", "Extensions", "Binary", "Status"] {
        assert!(stdout.contains(column), "missing column header `{column}`");
    }
    assert!(stdout.contains("-----"), "missing separator rule");

    // Exactly 9 data rows, one per language name (the style50 3.0.0 set
    // plus Rust).
    for name in [
        "C",
        "C++",
        "Java",
        "Python",
        "JavaScript",
        "HTML",
        "CSS",
        "SQL",
        "Rust",
    ] {
        let rows = stdout
            .lines()
            .filter(|l| l.starts_with(&format!("{name} ")))
            .count();
        assert_eq!(rows, 1, "expected exactly one row for {name}");
    }

    // Status column only ever says found (cache)/(toolchain) or
    // missing: bare tool names resolve cache-only (plus the Rust
    // toolchain for rustfmt); `PATH` is never claimed, and the
    // `ToolOrigin::Path` arm in the listing is unreachable for bare
    // names (kept for exhaustiveness).
    for line in stdout.lines().skip(2) {
        if line.trim().is_empty() {
            continue;
        }
        let status = line.rsplit(' ').next().unwrap_or("");
        assert!(
            ["(cache)", "(toolchain)", "missing"].contains(&status),
            "unexpected status token {status:?} in {line:?}"
        );
    }
}
