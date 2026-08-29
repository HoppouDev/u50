//! Benchmark harness for the adaptive diff strategy in `render::select_algorithm`.
//!
//! Measures wall time per `similar::Algorithm` on the input classes that
//! drive the strategy: real formatter output at golden scale, wholly-dirty
//! inputs at 7.5k/60k lines (zero shared lines), a *realistic* formatter-
//! mutated 60k input (every code line perturbed, structural lines like
//! braces/blanks persist — matching real clang-format output, which shares
//! a few dozen distinct lines even when wholly dirty), and a crossover
//! matrix at fixed common-line counts. Run with:
//!
//!   cargo run --release -p u50_style --example bench_diff

use std::collections::HashSet;
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use similar::algorithms::Algorithm;
use similar::{ChangeTag, TextDiff};

const ALL_IN_ONE_GROUP: usize = usize::MAX / 2;

/// Per-row wall-clock budget: algorithms measured after the budget is spent
/// are skipped (printed as `skip`) instead of stalling the whole run.
const ROW_BUDGET_SECS: u64 = 90;

fn build_diff<'a>(
    source: &'a str,
    formatted: &'a str,
    alg: Algorithm,
) -> TextDiff<'a, 'a, 'a, str> {
    TextDiff::configure()
        .algorithm(alg)
        .diff_lines(source, formatted)
}

fn render_unified(source: &str, formatted: &str, alg: Algorithm) -> String {
    build_diff(source, formatted, alg)
        .unified_diff()
        .context_radius(3)
        .header("x.c", "x.c")
        .to_string()
}

fn render_character(source: &str, formatted: &str, alg: Algorithm) -> String {
    let diff = build_diff(source, formatted, alg);
    let mut out = String::new();
    for group in &diff.grouped_ops(ALL_IN_ONE_GROUP) {
        for op in group {
            for change in diff.iter_inline_changes(op) {
                out.push(match change.tag() {
                    ChangeTag::Equal => ' ',
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                });
                for (_, value) in change.values() {
                    out.push_str(value.trim_end_matches(['\r', '\n']));
                }
                out.push('\n');
            }
        }
    }
    out
}

fn render_split(source: &str, formatted: &str, alg: Algorithm) -> String {
    let diff = build_diff(source, formatted, alg);
    let mut out = String::new();
    for group in &diff.grouped_ops(ALL_IN_ONE_GROUP) {
        for op in group {
            for change in diff.iter_changes(op) {
                out.push_str(change.value().trim_end_matches(['\r', '\n']));
                out.push('\n');
            }
        }
    }
    out
}

fn read_lines(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p)
        .unwrap_or_else(|e| panic!("read {p:?}: {e}"))
        .lines()
        .map(ToString::to_string)
        .collect()
}

fn join_lines(lines: &[String]) -> String {
    let mut out = String::with_capacity(lines.len() * 40);
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// Number of distinct lines shared by the two texts (the signal
/// `select_algorithm`'s overlap probe computes).
fn distinct_common(source: &str, formatted: &str) -> usize {
    let src: HashSet<&str> = source.lines().collect();
    formatted
        .lines()
        .collect::<HashSet<&str>>()
        .intersection(&src)
        .count()
}

/// Repeat `lines` up to `n` total lines.
fn repeat_to(lines: &[String], n: usize) -> Vec<String> {
    let mut pool = Vec::with_capacity(n);
    while pool.len() < n {
        pool.extend(lines.iter().cloned());
    }
    pool.truncate(n);
    pool
}

/// Suffix-index every line; `off` shifts the indices so two calls with
/// different offsets share nothing (wholly-dirty input).
fn uniquify(lines: &[String], off: usize) -> Vec<String> {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:06} {l}", i + off))
        .collect()
}

/// Perturb the first alphanumeric run so the line differs from the original
/// (and from every other mutated line) while keeping code shape — mimicking
/// a formatter that changed identifiers/numbers but kept structure.
fn mutate(i: usize, line: &str) -> String {
    match line.find(char::is_alphanumeric) {
        Some(pos) => {
            let mut out = String::with_capacity(line.len() + 10);
            out.push_str(&line[..pos]);
            out.push_str(&format!("v{i:06}"));
            out.push_str(&line[pos..]);
            out
        }
        None => format!("{line} /*v{i:06}*/"),
    }
}

/// Wholly-dirty pair at `n` lines with exactly zero shared lines.
fn wholly_dirty(base: &[String], n: usize) -> (String, String) {
    let src = join_lines(&uniquify(&repeat_to(base, n), 0));
    let fmt = join_lines(&uniquify(&repeat_to(base, n), 1_000_000));
    (src, fmt)
}

/// Crossover pair: `n` lines, `common` distinct lines shared (kept verbatim
/// and spread evenly); everything else is uniquely mutated.
fn with_common(base: &[String], n: usize, common: usize) -> (String, String) {
    let lines = uniquify(&repeat_to(base, n), 0);
    let src = join_lines(&lines);
    let step = n.div_ceil(common.max(1));
    let fmt = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if common > 0 && i % step == 0 {
                l.clone()
            } else {
                mutate(i, l)
            }
        })
        .collect::<Vec<_>>();
    (src, join_lines(&fmt))
}

/// Realistic formatter-mutated pair: every line carrying an identifier or
/// number is mutated (so the file is wholly dirty), but lines made purely of
/// punctuation/whitespace (`{`, `}`, blank, ...) persist verbatim — exactly
/// what real clang-format output does on an already-structured file.
fn formatter_mutated(base: &[String], n: usize) -> (String, String) {
    let lines = repeat_to(base, n);
    let src = join_lines(&lines);
    let fmt = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if l.chars().any(char::is_alphanumeric) {
                mutate(i, l)
            } else {
                l.clone()
            }
        })
        .collect::<Vec<_>>();
    (src, join_lines(&fmt))
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cfix = root.join("tests/fixtures/c");
    let dirty = read_lines(&cfix.join("dirty.c"));
    let expected = read_lines(&cfix.join("expected.c"));

    let mut rows: Vec<(String, (String, String), bool)> = Vec::new();
    // Golden-scale real formatter output (realistic shared-line profile).
    rows.push((
        "golden 2.5k real dirty->expected".into(),
        (join_lines(&dirty), join_lines(&expected)),
        true,
    ));
    for &n in &[7_500, 60_000] {
        rows.push((
            format!("{n} wholly-dirty (0 common)"),
            wholly_dirty(&expected, n),
            true,
        ));
    }
    let (src, fmt) = formatter_mutated(&expected, 60_000);
    rows.push(("60k formatter-mutated (realistic)".into(), (src, fmt), true));
    for &n in &[7_500, 60_000] {
        for &common in &[0, 28, 100, 500] {
            rows.push((
                format!("{n} lines, {common} common"),
                with_common(&expected, n, common),
                false,
            ));
        }
    }

    println!(
        "{:<36}{:>9} {:>8} | {:>22} {:>22} {:>22}",
        // Cell order below is [Myers, Lcs, Patience]; keep the header in sync
        // (an earlier revision printed `patience lcs` here while measuring
        // in `[Myers, Lcs, Patience]` order, swapping the two labels).
        "input",
        "bytes",
        "common",
        "myers",
        "lcs",
        "patience"
    );
    for (name, (src, fmt), all_modes) in &rows {
        let bytes = src.len() + fmt.len();
        let common = distinct_common(src, fmt);
        let mut cells: Vec<String> = Vec::new();
        let mut spent = Duration::ZERO;
        for alg in [Algorithm::Myers, Algorithm::Lcs, Algorithm::Patience] {
            if spent.as_secs() >= ROW_BUDGET_SECS {
                cells.push("skip".into());
                continue;
            }
            let t0 = Instant::now();
            let u = render_unified(src, fmt, alg);
            let t1 = Instant::now();
            let cell = if *all_modes {
                let ch = render_character(src, fmt, alg);
                let t2 = Instant::now();
                let sp = render_split(src, fmt, alg);
                let t3 = Instant::now();
                black_box((&u, &ch, &sp));
                format!(
                    "u={:>6.2?} c={:>6.2?} s={:>6.2?}",
                    t1 - t0,
                    t2 - t1,
                    t3 - t2
                )
            } else {
                black_box(&u);
                format!("u={:>7.2?}", t1 - t0)
            };
            spent += t1 - t0;
            cells.push(cell);
        }
        println!(
            "{name:<36}{bytes:>9} {common:>8} | {:>22} {:>22} {:>22}",
            cells[0], cells[1], cells[2]
        );
    }
}
