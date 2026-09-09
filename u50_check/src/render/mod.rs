//! Result rendering, dispatched through the render registry (one
//! renderer per output format, mirroring check50's `to_ansi`/`to_json`).

pub(crate) mod ansi;
pub(crate) mod json;

use crate::result::CheckResult;

/// The inputs a renderer receives (check50: `to_*(slug, results,
/// version)`).
pub(crate) struct RenderInput<'a> {
    pub slug: &'a str,
    pub results: &'a [CheckResult],
    pub version: &'a str,
    /// Whether the run errored (the `error` key replaces `results`).
    pub error: Option<&'a crate::result::EngineError>,
    /// Whether the log is displayed in the ansi output (check50:
    /// `--ansi-log`).
    pub show_log: bool,
    /// Whether ANSI colors may be emitted (stdout is a terminal).
    pub color: bool,
}
