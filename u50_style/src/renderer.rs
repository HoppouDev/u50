//! Pluggable output rendering for style checks: a [`Renderer`] receives
//! the results of a run as a stream of events, and built-in implementations
//! reproduce the legacy console and JSON output byte for byte.
//!
//! Rendering is decoupled from processing ([`crate::engine::run_with`]):
//! [`crate::engine::run_with_renderer`] drives any [`Renderer`] over a
//! report, so custom sinks (HTML, SARIF, an editor panel, ...) need only
//! implement the trait — no changes to the engine.

use std::io::Write;
use std::path::Path;

use similar::ChangeTag;

use crate::render::{
    RESET, YELLOW, json_document, line_diff, render_character, render_split, render_unified,
};
use crate::request::{FileResult, Output, Report, Request};

/// A sink for the events of a style check.
///
/// Event order: [`begin`](Renderer::begin), then one
/// [`file`](Renderer::file) per successfully processed file and one
/// [`file_error`](Renderer::file_error) per file that could not be
/// processed, then [`finish`](Renderer::finish). Every method has an empty
/// default, so a custom renderer only overrides what it needs.
///
/// # Examples
///
/// A minimal HTML renderer: a table row per file, wrapped in a document at
/// the end.
///
/// ```
/// use u50_style::{FileResult, Output, Renderer, Report, Request};
///
/// struct HtmlRenderer {
///     buf: String,
/// }
///
/// impl Renderer for HtmlRenderer {
///     fn begin(&mut self, _req: &Request) {
///         self.buf.push_str("<html><body><table>\n");
///     }
///
///     fn file(&mut self, result: &FileResult) {
///         self.buf.push_str(&format!(
///             "<tr><td>{}</td><td>{}</td></tr>\n",
///             result.path.display(),
///             result.clean
///         ));
///     }
///
///     fn finish(&mut self, _report: &Report) {
///         self.buf.push_str("</table></body></html>\n");
///     }
/// }
///
/// let req = Request {
///     files: vec![],
///     output: Output::Character,
///     color: false,
/// };
/// let mut renderer = HtmlRenderer { buf: String::new() };
/// renderer.begin(&req);
/// renderer.file(&FileResult {
///     path: "x.c".into(),
///     clean: false,
///     source: Some("return 0;\n".into()),
///     formatted: Some("    return 0;\n".into()),
/// });
/// renderer.finish(&Report::default());
/// assert!(renderer.buf.starts_with("<html><body><table>\n"));
/// assert!(renderer.buf.contains("<tr><td>x.c</td><td>false</td></tr>\n"));
/// assert!(renderer.buf.ends_with("</table></body></html>\n"));
/// ```
pub trait Renderer {
    /// Called once before the first file is reported.
    fn begin(&mut self, _req: &Request) {}

    /// One successfully processed file (clean or dirty).
    fn file(&mut self, _result: &FileResult) {}

    /// One file that could not be processed.
    fn file_error(&mut self, _path: &Path, _message: &str) {}

    /// Called once after all files; final output (e.g. a document) is
    /// emitted here.
    fn finish(&mut self, _report: &Report) {}
}

/// Writes the legacy per-file console output (the configured diff for each
/// dirty file) and `error: <path>: <message>` lines to stderr. JSON-mode
/// requests are served by [`JsonRenderer`] instead; if a JSON output is
/// requested of this renderer directly, it falls back to the unified diff.
pub struct ConsoleRenderer {
    output: Output,
    color: bool,
    out: Box<dyn Write>,
}

impl Renderer for ConsoleRenderer {
    fn file(&mut self, result: &FileResult) {
        if result.clean {
            return;
        }
        let (Some(source), Some(formatted)) = (&result.source, &result.formatted) else {
            return;
        };
        let rendered = match self.output {
            Output::Character => render_character(source, formatted, self.color),
            Output::Split => render_split(source, formatted, self.color),
            // `Json`/`Score` never reach this renderer through
            // [`builtin_renderer`]; direct use falls back to the unified
            // diff, like JSON always has.
            Output::Unified | Output::Json | Output::Score => {
                render_unified(source, formatted, &result.path)
            }
        };
        let _ = write!(self.out, "{rendered}");
    }

    fn file_error(&mut self, path: &Path, message: &str) {
        eprintln!("error: {}: {message}", path.display());
    }
}

/// Writes the machine-readable JSON document (one entry per file, with the
/// unified patch for dirty files) in [`finish`](Renderer::finish), plus
/// `error: <path>: <message>` lines to stderr. Per-file events are no-ops:
/// nothing is buffered per file, the document is built from the report at
/// the end.
pub struct JsonRenderer {
    out: Box<dyn Write>,
}

impl Renderer for JsonRenderer {
    fn finish(&mut self, report: &Report) {
        let document = json_document(report);
        let _ = writeln!(self.out, "{document}");
    }

    fn file_error(&mut self, path: &Path, message: &str) {
        eprintln!("error: {}: {message}", path.display());
    }
}

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
/// accumulates the styled text's non-blank line count. The final score is
/// `max(1 - diffs/lines, 0)`, or `0.0` when no file was checked
/// successfully. Only successful files contribute — the engine never
/// reports [`Renderer::file`] for errored files — matching the original,
/// which sums over successfully checked files only. A styled text with no
/// non-blank lines contributes a `file is empty` error line instead of
/// touching the sums (the original raises a per-file `Error` there). No
/// diff text is produced. Note: u50 keeps its own exit codes in score
/// mode; the original style50 always exits 0.
pub struct ScoreRenderer {
    color: bool,
    out: Box<dyn Write>,
    errors: Vec<String>,
    diffs: f64,
    lines: u64,
}

impl Renderer for ScoreRenderer {
    fn file(&mut self, result: &FileResult) {
        let (Some(source), Some(formatted)) = (&result.source, &result.formatted) else {
            return;
        };
        let change_count = line_diff(source, formatted)
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
        // Line counts stay far below 2^53, so the conversions are exact.
        #[allow(clippy::cast_precision_loss)]
        let file_diffs = change_count as f64 / 2.0;
        self.diffs += file_diffs;
        self.lines += non_blank as u64;
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
                let _ = writeln!(self.out, "{YELLOW}{message}{RESET}");
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

/// Returns the built-in renderer for `output`: [`JsonRenderer`] for
/// [`Output::Json`], [`ScoreRenderer`] for [`Output::Score`], and
/// [`ConsoleRenderer`] for the text modes. `out` receives the rendered
/// bytes (stderr output is always written directly).
#[must_use]
pub fn builtin_renderer(output: Output, color: bool, out: Box<dyn Write>) -> Box<dyn Renderer> {
    match output {
        Output::Json => Box::new(JsonRenderer { out }),
        Output::Score => Box::new(ScoreRenderer {
            color,
            out,
            errors: Vec::new(),
            diffs: 0.0,
            lines: 0,
        }),
        Output::Character | Output::Split | Output::Unified => {
            Box::new(ConsoleRenderer { output, color, out })
        }
    }
}
