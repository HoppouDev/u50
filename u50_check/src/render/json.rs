//! The JSON renderer: check50's documented schema with 4-space
//! indentation (`{slug, results[], version}`, with the `error` key
//! replacing `results` when the run itself failed).

use serde::Serialize as _;
use serde_json::{Value, json};

use super::RenderInput;

/// Renders the JSON document (pretty-printed with 4-space indent).
#[must_use]
pub(crate) fn render_json(input: &RenderInput) -> String {
    let mut document = match input.error {
        Some(error) => json!({
            "slug": input.slug,
            "error": error,
            "version": input.version,
        }),
        None => json!({
            "slug": input.slug,
            "results": input.results,
            "version": input.version,
        }),
    };
    // check50's results carry `dependency: null` (never omitted).
    if let Some(results) = document.get_mut("results")
        && let Some(results) = results.as_array_mut()
    {
        for result in results {
            result
                .as_object_mut()
                .expect("results are objects")
                .entry("dependency")
                .or_insert(Value::Null);
        }
    }
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut buf = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut buf, formatter);
    document
        .serialize(&mut serializer)
        .expect("serializing to a Vec cannot fail");
    String::from_utf8(buf).expect("the JSON serializer emits valid UTF-8")
}
