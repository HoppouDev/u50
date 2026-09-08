//! The JSON renderer and the JSON document builder.

use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::rendering::unified::patch;
use crate::request::Report;

use super::Renderer;

/// Writes the machine-readable JSON document (one entry per file, with the
/// unified patch for dirty files) in [`finish`](Renderer::finish), plus
/// `error: <path>: <message>` lines to stderr. Per-file events are no-ops:
/// nothing is buffered per file, the document is built from the report at
/// the end.
pub struct JsonRenderer {
    pub(crate) out: Box<dyn Write>,
}

impl Renderer for JsonRenderer {
    fn finish(&mut self, report: &Report) {
        let document = json_document(report);
        let _ = writeln!(
            self.out,
            "{}",
            String::from_utf8_lossy(&json_pretty(&document))
        );
    }

    fn file_error(&mut self, path: &Path, message: &str) {
        eprintln!("error: {}: {message}", path.display());
    }
}

/// Serializes `document` pretty-printed with a **4-space indent**, matching
/// the original style50's JSON formatting (the schema itself stays u50's
/// own — a documented by-design divergence). The bytes carry no trailing
/// newline.
pub(crate) fn json_pretty(document: &serde_json::Value) -> Vec<u8> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut buf, formatter);
    // Serializing a `serde_json::Value` into an in-memory buffer cannot
    // fail, so the result is ignored.
    let _ = Serialize::serialize(document, &mut serializer);
    buf
}

/// Builds the single JSON document printed in JSON mode.
pub(crate) fn json_document(report: &Report) -> serde_json::Value {
    serde_json::json!({
        "clean": report.clean(),
        "files": report
            .results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "path": r.path.display().to_string(),
                    "clean": r.clean,
                    // Clean files carry no patch (null), matching the legacy
                    // schema; dirty files render the unified diff of source
                    // against formatted.
                    "patch": patch(r),
                })
            })
            .collect::<Vec<serde_json::Value>>(),
    })
}
