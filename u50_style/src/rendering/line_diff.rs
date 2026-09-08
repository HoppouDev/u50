//! Line-wise diffing with the measured adaptive algorithm strategy
//! (Myers by default, Lcs for large low-overlap pairs).

use similar::TextDiff;
use similar::algorithms::Algorithm;

/// Context radius passed to `TextDiff::grouped_ops` to keep every change in
/// a single group. Must satisfy `n * 2 <= usize::MAX` (see
/// `similar::common::group_diff_ops`); `usize::MAX` itself would overflow.
pub(crate) const ALL_IN_ONE_GROUP: usize = usize::MAX / 2;

pub(crate) fn trim_line(value: &str) -> String {
    value.trim_end_matches(['\r', '\n']).to_owned()
}

/// Below this line count the overlap probe is not worth its hashing cost
/// (rule: see `select_algorithm`):
/// Myers handles such inputs in single-digit milliseconds (see
/// `examples/bench_diff.rs` for the measurements behind these choices).
const ADAPTIVE_MIN_LINES: usize = 1024;

/// Measured (release, `examples/bench_diff.rs`; wall time for the unified
/// render):
///
/// | input                                    | Myers   | Lcs     |
/// |------------------------------------------|---------|---------|
/// | golden 2.5k real dirty→expected, 26 common | 11.5ms | 0.57ms |
/// | 7.5k wholly-dirty (0 common)             | 509.6ms | 205.7ms |
/// | 60k wholly-dirty (0 common)              | 32.21s  | 13.14s  |
/// | 60k, 28 common (earlier build)           | 32.5s   | 12.9s   |
/// | 7.5k, 8 common (earlier build)           | ~1s     | ~3s     |
///
/// Myers degrades quadratically on large low-overlap pairs while Lcs stays
/// linear-ish — but Lcs collapses once the inputs share a real number of
/// lines (7.5k with 8 common: 3s vs Myers' 1s). Lcs is therefore engaged
/// only when the larger side has at least [`ADAPTIVE_MIN_LINES`] lines AND
/// the distinct shared lines are fewer than a thousandth of it. The 1000x
/// multiplier is a measured heuristic: the crossover between the
/// 8-common@7.5k collapse and the 28-common@60k win lies between those
/// points, and `examples/bench_diff.rs` records the matrix behind it.
///
/// Display-only concern: only diff rendering consults this; the formatter
/// results (and thus clean/dirty decisions) are unaffected.
pub(crate) fn select_algorithm(source: &str, formatted: &str) -> Algorithm {
    let max_lines = source.lines().count().max(formatted.lines().count());
    if max_lines < ADAPTIVE_MIN_LINES {
        return Algorithm::Myers;
    }
    let src: std::collections::HashSet<&str> = source.lines().collect();
    let common = formatted
        .lines()
        .collect::<std::collections::HashSet<&str>>()
        .intersection(&src)
        .count();
    if common.saturating_mul(1000) < max_lines {
        Algorithm::Lcs
    } else {
        Algorithm::Myers
    }
}

/// Diffs the two texts line-wise with the measured algorithm strategy.
pub(crate) fn line_diff<'a>(source: &'a str, formatted: &'a str) -> TextDiff<'a, 'a, 'a, str> {
    TextDiff::configure()
        .algorithm(select_algorithm(source, formatted))
        .diff_lines(source, formatted)
}

/// Line diff for the score renderer: Myers **unconditionally**. The
/// adaptive Lcs switch is display-only — the score sums the change count
/// and must match style50's ndiff-based value on every input.
pub(crate) fn line_diff_score<'a>(
    source: &'a str,
    formatted: &'a str,
) -> TextDiff<'a, 'a, 'a, str> {
    TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .diff_lines(source, formatted)
}
