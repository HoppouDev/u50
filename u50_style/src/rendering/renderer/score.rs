//! The aggregate score renderer.

use std::io::Write;
use std::path::Path;

use similar::ChangeTag;

use crate::language::{detect_language, style50_count_lines};
use crate::rendering::line_diff::line_diff_score;
use crate::rendering::palette::{reset, yellow};
use crate::request::{FileResult, Report};

use super::Renderer;

/// Formats an `f64` the way Python's `str()` formats style scores: the
/// shortest decimal string that round-trips, always with a decimal point
/// (`1.0`, not `1`). Rust's `Debug` for `f64` uses the same
/// shortest-round-trip algorithm, so the two agree on every realistic
/// score (a value in `[0, 1]` built from small rational ratios); they
/// differ only for extreme magnitudes where Python switches to
/// exponent notation with a zero-padded exponent (`1e-07`).
#[must_use]
pub(crate) fn py_str_f64(value: f64) -> String {
    format!("{value:?}")
}

/// Writes the style50-compatible aggregate score — a single line such as
/// `0.85` — in [`finish`](Renderer::finish), preceded by one line per
/// file that could not be processed (yellow when `color` is set, matching
/// the original's unconditional termcolor yellow).
///
/// The score mirrors the original style50's score mode exactly: for each
/// successfully processed file, `diffs` accumulates half the number of
/// inserted/deleted lines between the normalized source and its styled
/// content (via the same line diff the display modes use), and `lines`
/// accumulates the styled text's line count — ALL lines for Python (the
/// original's `Python.count_lines` counts blank lines too, per PEP 8),
/// non-blank lines only for every other language. The final score is
/// `max(1 - diffs/lines, 0)`, or `0.0` when no file was checked
/// successfully. Only successful files contribute — the engine never
/// reports [`Renderer::file`] for errored files — matching the original,
/// which sums over successfully checked files only. A styled text with no
/// non-blank lines contributes a `file is empty` error line instead of
/// touching the sums (the original raises a per-file `Error` there). No
/// diff text is produced. Note: u50 keeps its own exit codes in score
/// mode; the original style50 always exits 0.
pub struct ScoreRenderer {
    pub(crate) color: bool,
    pub(crate) out: Box<dyn Write>,
    pub(crate) errors: Vec<String>,
    pub(crate) diffs: f64,
    pub(crate) lines: u64,
}

impl Renderer for ScoreRenderer {
    fn skipped(&mut self, path: &Path) {
        // Buffered so the warning prints before the score line, matching
        // the original's error-then-score ordering.
        self.errors.push(format!(
            "unknown file type \"{}\", skipping...",
            path.display()
        ));
    }

    fn file(&mut self, result: &FileResult) {
        let (Some(source), Some(formatted)) = (&result.source, &result.formatted) else {
            return;
        };
        let change_count = line_diff_score(source, formatted)
            .iter_all_changes()
            .filter(|change| !matches!(change.tag(), ChangeTag::Equal))
            .count();
        let non_blank = formatted
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        if non_blank == 0 {
            // The original raises `Error("file is empty")` when the styled
            // text has no non-blank lines; the file is then errored (not
            // summed). u50's engine already errors up front for empty
            // input, so this only covers a formatter emptying a file.
            self.errors.push("file is empty".to_owned());
            return;
        }
        // style50's `Python.count_lines` counts ALL lines of the styled
        // text ("blank lines are relevant to style per pep8"); every other
        // language counts non-blank lines only (the shared
        // `style50_count_lines` helper). The empty-text check above stays
        // on the non-blank count for all languages.
        let line_count = detect_language(&result.path).map_or(non_blank, |language| {
            style50_count_lines(formatted, language)
        });
        // Line counts stay far below 2^53, so the conversions are exact.
        #[allow(clippy::cast_precision_loss)]
        let file_diffs = change_count as f64 / 2.0;
        self.diffs += file_diffs;
        self.lines += line_count as u64;
    }

    fn file_error(&mut self, _path: &Path, message: &str) {
        self.errors.push(message.to_owned());
    }

    fn finish(&mut self, _report: &Report) {
        // The original prints each error message bare (no `error: ` prefix
        // — the messages themselves name the file) in file order, colored
        // yellow, then the uncolored score line with a trailing newline.
        for message in &self.errors {
            if self.color {
                let _ = writeln!(self.out, "{}{message}{}", yellow(), reset());
            } else {
                let _ = writeln!(self.out, "{message}");
            }
        }
        let score = if self.lines == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let ratio = self.diffs / self.lines as f64;
            (1.0 - ratio).max(0.0)
        };
        let _ = writeln!(self.out, "{}", py_str_f64(score));
    }
}
