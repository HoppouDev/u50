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

use crate::render::{json_document, render_character, render_split, render_unified};
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
            Output::Unified | Output::Json => render_unified(source, formatted, &result.path),
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

/// Returns the built-in renderer for `output`: [`JsonRenderer`] for
/// [`Output::Json`], [`ConsoleRenderer`] for the text modes. `out` receives
/// the rendered bytes (stderr output is always written directly).
#[must_use]
pub fn builtin_renderer(output: Output, color: bool, out: Box<dyn Write>) -> Box<dyn Renderer> {
    match output {
        Output::Json => Box::new(JsonRenderer { out }),
        Output::Character | Output::Split | Output::Unified => {
            Box::new(ConsoleRenderer { output, color, out })
        }
    }
}
