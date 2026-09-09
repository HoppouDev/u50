//! Pluggable report renderers: a [`Renderer`] receives the results of a
//! run as a stream of events, and built-in implementations reproduce the
//! legacy console and JSON output byte for byte.

use std::io::Write;
use std::path::Path;

use crate::request::{FileResult, Output, Report, Request};

pub(crate) mod console;
pub(crate) mod html;
pub(crate) mod json;
pub(crate) mod score;

pub use console::ConsoleRenderer;
pub use html::HtmlRenderer;
pub use json::JsonRenderer;
pub use score::ScoreRenderer;

/// A sink for the events of a style check.
///
/// Event order: [`begin`](Renderer::begin), then one
/// [`skipped`](Renderer::skipped) per unsupported regular file found while
/// walking a directory operand, then one [`file`](Renderer::file) per
/// successfully processed file and one [`file_error`](Renderer::file_error)
/// per file that could not be processed, then [`finish`](Renderer::finish).
/// Every method has an empty default, so a custom renderer only overrides
/// what it needs.
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

    /// The total number of files this run covers — successfully processed
    /// files plus per-file errors (walk-warned unsupported files are
    /// excluded), called by [`crate::run_with_renderer`] right after
    /// [`begin`](Renderer::begin) so renderers that must know whether
    /// per-file headers apply (character mode prints them only when a run
    /// has more than one file) learn the count before the first
    /// [`file`](Renderer::file) event. Default: no-op.
    fn total_files(&mut self, _count: usize) {}

    /// One unsupported regular file found while walking a directory
    /// operand (the style50-parity `unknown file type "<path>",
    /// skipping...` warning). Never called for explicit file arguments;
    /// symlinks, FIFOs, and devices inside the walk are never reported.
    fn skipped(&mut self, _path: &Path) {}

    /// One successfully processed file (clean or dirty).
    fn file(&mut self, _result: &FileResult) {}

    /// One file that could not be processed.
    fn file_error(&mut self, _path: &Path, _message: &str) {}

    /// Called once after all files; final output (e.g. a document) is
    /// emitted here.
    fn finish(&mut self, _report: &Report) {}
}

/// One output format, self-contained: builds the [`Renderer`] serving
/// its outputs. Implemented by a zero-sized struct in the renderer's
/// own module and registered in `crate::registry::renderers()`.
pub(crate) trait RendererPlugin: Sync {
    /// The output formats this plugin serves.
    fn outputs(&self) -> &'static [Output];

    /// Diagnostics name (`"console"`, `"json"`, ...).
    fn name(&self) -> &'static str;

    /// Builds the renderer for one run.
    fn create(&self, output: Output, color: bool, out: Box<dyn Write>) -> Box<dyn Renderer>;
}

/// Returns the built-in renderer for `output`, looked up in the
/// renderer registry (one plugin per [`Output`]; the console plugin
/// serves the three text modes). `out` receives the rendered bytes
/// (stderr output is always written directly).
///
/// # Panics
/// Panics when no plugin is registered for `output` — impossible for
/// the compiled-in set, guarded by the registry tests.
#[must_use]
pub fn builtin_renderer(output: Output, color: bool, out: Box<dyn Write>) -> Box<dyn Renderer> {
    if let Some(plugin) = crate::registry::renderers()
        .iter()
        .find(|plugin| plugin.outputs().contains(&output))
    {
        plugin.create(output, color, out)
    } else {
        let known = crate::registry::renderers()
            .iter()
            .map(|plugin| plugin.name())
            .collect::<Vec<_>>()
            .join(", ");
        panic!("no renderer plugin registered for {output:?} (registered: {known})")
    }
}
