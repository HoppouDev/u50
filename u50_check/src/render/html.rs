//! The HTML report renderer (check50's `results.html` template,
//! byte-matched against the Jinja2 render for parity with the real
//! check50's detailed report).

#![allow(clippy::format_push_string)]
use super::RenderInput;
use crate::result::{Cause, CheckResult};

/// Generates the HTML report as a complete document (check50 parity:
/// the `results.html` template rendered through Jinja2, with the same
/// whitespace patterns from the template's `{% %}` tags and branches).
#[must_use]
pub(crate) fn render_html(input: &RenderInput) -> String {
    let mut body = String::with_capacity(input.results.len() * 512 + 1024);

    for result in input.results {
        body.push_str("\n              \n                  \n");
        render_result(&mut body, result);
        body.push_str("\n              \n");
    }

    format!(
        "<!DOCTYPE html>\n<html>\n    <head>\n        <link rel=\"stylesheet\" href=\"https://maxcdn.bootstrapcdn.com/bootstrap/3.3.7/css/bootstrap.min.css\" integrity=\"sha384-BVYiiSIFeK1dGmJRAkycuHAHRg32OmUcww7on3RYdg4Va+PmSTsz/K68vbdEjh4u\" crossorigin=\"anonymous\">\n        <title>This is check50.</title>\n    </head>\n    <body>\n        <div class=\"container\">\n            <div class=\"row\">\n\n            <h1>check50</h1>\n            <h2>{slug}</h2>\n            <hr>\n{body}\n\n            </div>\n        </div>\n    </body>\n</html>\n",
        slug = html_escape(input.slug),
    )
}

fn render_result(out: &mut String, result: &CheckResult) {
    let (emoji, color) = match result.passed {
        Some(true) => (":)", "green"),
        Some(false) => (":(", "red"),
        None => (":|", "orange"),
    };

    // <h3> with emoji + description (no extra spaces, no trailing ;)
    out.push_str(&format!(
        "                      <h3 style=\"color:{color}\">{emoji} {}</h3>\n",
        html_escape(&result.description),
    ));

    // Cause block for failed/skipped checks (check50: the Jinja2
    // if/elif/else branches render the cause inside the well div).
    match (result.passed, &result.cause) {
        (Some(false), Some(cause)) => {
            out.push_str(
                "\n                  \n                      <div class=\"well well-sm\">\n",
            );
            render_cause(out, cause);
            render_log(out, &result.log);
            out.push_str("\n                      </div>");
        }
        (_, Some(cause)) if matches!(cause, Cause::Skipped { .. }) => {
            out.push_str(
                "\n                  \n                      <div class=\"well well-sm\">\n",
            );
            render_cause(out, cause);
            render_log(out, &result.log);
            out.push_str("\n                      </div>");
        }
        _ => {
            // Passing checks: well div with log only.
            out.push_str("\n                  \n\n                  \n                      <div class=\"well well-sm\">\n");
            render_log(out, &result.log);
            out.push_str("\n                      </div>");
        }
    }
}

fn render_cause(out: &mut String, cause: &Cause) {
    out.push_str("                          \n");
    match cause {
        Cause::Failure { rationale, help } => {
            out.push_str(&format!(
                "                          <p style=\"color:orange\">{}</p>\n",
                html_escape(rationale),
            ));
            if let Some(help) = help {
                out.push_str(&format!(
                    "                          <p>{}</p>\n",
                    html_escape(help),
                ));
            }
        }
        Cause::Mismatch {
            rationale,
            help,
            expected,
            actual,
        } => {
            out.push_str(&format!(
                "                          <p style=\"color:orange\">{}</p>\n",
                html_escape(rationale),
            ));
            if let Some(help) = help {
                out.push_str(&format!(
                    "                          <p>{}</p>\n",
                    html_escape(help),
                ));
            }
            out.push_str(&format!(
                "                          <p><b>Expected:</b> {}</p>\n",
                html_escape(expected),
            ));
            out.push_str(&format!(
                "                          <p><b>Actual:</b> {}</p>\n",
                html_escape(actual),
            ));
        }
        Cause::Skipped { rationale } => {
            out.push_str(&format!(
                "                          <p style=\"color:orange\">{}</p>\n",
                html_escape(rationale),
            ));
        }
        Cause::Error { rationale, error } => {
            out.push_str(&format!(
                "                          <p style=\"color:red\">{}</p>\n",
                html_escape(rationale),
            ));
            out.push_str(&format!(
                "                          <p><b>{}:</b> {}</p>\n",
                html_escape(&error.kind),
                html_escape(&error.value),
            ));
        }
    }
    out.push_str("                          \n");
}

fn render_log(out: &mut String, log: &[String]) {
    if log.is_empty() {
        return;
    }
    out.push_str("\n                          \n\n                          \n                              <samp>\n                                  <b>Log</b><br/>\n                                  <div style=\"border:2px solid; padding:2px; max-height:20em; overflow:scroll;\">\n");
    for line in log {
        out.push_str("                                      \n");
        out.push_str(&format!(
            "                                          {}\n                                          <br/>\n",
            html_escape(line),
        ));
    }
    out.push_str("                                      \n                                  </div>\n                              </samp>\n");
}

/// Escapes HTML special characters.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Generates a random 8-character lowercase alphanumeric suffix for
/// temp file names (matching Python's `tempfile.mkstemp` naming).
pub(crate) fn random_suffix() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..8)
        .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
        .collect()
}
