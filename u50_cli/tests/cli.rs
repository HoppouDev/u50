//! End-to-end CLI tests (spawns the real binary).

use std::process::Command;

#[test]
fn style_list_prints_language_table() {
    let output = Command::new(env!("CARGO_BIN_EXE_u50"))
        .args(["style", "--list"])
        .output()
        .expect("failed to spawn u50");
    assert!(output.status.success(), "--list must exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Header and separator structure.
    for column in ["Language", "Extensions", "Binary", "Status"] {
        assert!(stdout.contains(column), "missing column header `{column}`");
    }
    assert!(stdout.contains("-----"), "missing separator rule");

    // Exactly 8 data rows, one per language name.
    for name in [
        "C",
        "C++",
        "Java",
        "Python",
        "JavaScript",
        "HTML",
        "CSS",
        "SQL",
    ] {
        let rows = stdout
            .lines()
            .filter(|l| l.starts_with(&format!("{name} ")))
            .count();
        assert_eq!(rows, 1, "expected exactly one row for {name}");
    }

    // Status column only ever says found (PATH|cache) or missing.
    for line in stdout.lines().skip(2) {
        if line.trim().is_empty() {
            continue;
        }
        let status = line.rsplit(' ').next().unwrap_or("");
        assert!(
            ["(PATH)", "(cache)", "missing"].contains(&status),
            "unexpected status token {status:?} in {line:?}"
        );
    }
}
