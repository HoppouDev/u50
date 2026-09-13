//! The HTML report renderer (check50's `results.html` template through
//! leptos SSR, the same approach `u50_style`'s renderer uses).

use leptos::prelude::*;
use leptos::tachys::view::Position;

use super::RenderInput;
use crate::result::{Cause, CheckResult};

/// One check result's branch-resolved HTML chunk (like style50's
/// `HtmlFile` enum: the template's per-result if/elif/else branches,
/// pre-resolved).
pub(crate) enum HtmlResult {
    /// `passed == true`: green `<h3>`, no cause block.
    Pass {
        description: String,
        log: Vec<String>,
    },
    /// `passed == false`: red `<h3>` with the cause (rationale/help,
    /// mismatch expected/actual, or error).
    Fail {
        description: String,
        rationale: String,
        help: Option<String>,
        mismatch: Option<(String, String)>,
        error: Option<(String, String)>,
        log: Vec<String>,
    },
    /// `passed == None`: orange `<h3>` with the skip rationale.
    Skip {
        description: String,
        rationale: String,
        log: Vec<String>,
    },
}

/// Serializes a leptos view to an HTML string (SSR path:
/// `to_html_with_buf` with escaping enabled, no branch markers).
fn render_fragment<V: RenderHtml>(view: V) -> String {
    let mut buf = String::new();
    let mut position = Position::FirstChild;
    view.to_html_with_buf(&mut buf, &mut position, true, false, Vec::new());
    buf
}

/// Whitespace text nodes: leptos strips whitespace between tags in
/// `view!`, so template-space runs are passed as dynamic text children.
fn ws(text: &str) -> &str {
    text
}

/// Renders one check result chunk using leptos views (check50's
/// `results.html` template per-result structure).
fn html_result_chunk(result: &HtmlResult) -> String {
    let mut chunk = String::from("\n                ");

    let (emoji, color) = match result {
        HtmlResult::Pass { .. } => (":)", "green"),
        HtmlResult::Fail { .. } => (":(", "red"),
        HtmlResult::Skip { .. } => (":|", "orange"),
    };

    let description = match result {
        HtmlResult::Pass { description, .. }
        | HtmlResult::Fail { description, .. }
        | HtmlResult::Skip { description, .. } => description,
    };

    let h3_html = format!(" {emoji} {description} ");
    chunk.push_str(&render_fragment(view! {
        <h3 style = format!("color:{color}") inner_html = h3_html></h3>
    }));

    chunk.push_str("\n                <div class=\"well well-sm\">");

    // Cause block for failed/skipped checks.
    match result {
        HtmlResult::Fail {
            rationale,
            help,
            mismatch,
            error,
            ..
        } => {
            let rationale_html = format!(" {rationale} ");
            chunk.push_str(&render_fragment(view! {
                <p style = format!("color:{color}") inner_html = rationale_html></p>
            }));
            if let Some(help) = help {
                let help_html = format!(" {help} ");
                chunk.push_str(&render_fragment(view! {
                    <p inner_html = help_html></p>
                }));
            }
            if let Some((expected, actual)) = mismatch {
                let expected_html = format!(" <b>Expected:</b> {expected} ");
                let actual_html = format!(" <b>Actual:</b> {actual} ");
                chunk.push_str(&render_fragment(view! {
                    <p>
                        <span inner_html = expected_html></span>
                        {ws(" ")}
                        <span inner_html = actual_html></span>
                    </p>
                }));
            }
            if let Some((kind, value)) = error {
                let error_html = format!(" <b>{kind}</b> {value} ");
                chunk.push_str(&render_fragment(view! {
                    <p inner_html = error_html></p>
                }));
            }
        }
        HtmlResult::Skip { rationale, .. } => {
            let rationale_html = format!(" {rationale} ");
            chunk.push_str(&render_fragment(view! {
                <p style = format!("color:{color}") inner_html = rationale_html></p>
            }));
        }
        HtmlResult::Pass { .. } => {}
    }

    // Log block (check50 parity: scrollable <samp> with the check log).
    let log = match result {
        HtmlResult::Pass { log, .. }
        | HtmlResult::Fail { log, .. }
        | HtmlResult::Skip { log, .. } => log,
    };
    if !log.is_empty() {
        chunk.push_str("\n                <samp>\n                    <b>Log</b><br/>\n                    <div style=\"border:2px solid; padding:2px; max-height:20em; overflow:scroll;\">");
        for line in log {
            let line_html = format!(" {line} ");
            chunk.push_str(&render_fragment(view! {
                <div inner_html = line_html></div>
            }));
        }
        chunk.push_str("\n                    </div>\n                </samp>");
    }

    chunk.push_str("\n                </div>");
    chunk
}

/// Assembles the full document: head (Bootstrap CSS), container with
/// check50 branding, one chunk per result, and the closing tags.
fn html_document(slug: &str, results: &[HtmlResult]) -> String {
    let mut body = String::from(
        "\n        <div class=\"container\">\n            <div class=\"row\">\n\n            <h1>check50</h1>\n            ",
    );
    let slug_html = format!(" {slug} ");
    body.push_str(&render_fragment(view! {
        <h2 inner_html = slug_html></h2>
    }));
    body.push_str("\n            <hr>\n            ");

    for result in results {
        body.push_str(&html_result_chunk(result));
    }

    body.push_str("\n            </div>\n        ");

    let doc = render_fragment(view! {
        <html>
            {ws("\n    ")}
            <head>
                {ws("\n        ")}
                <link
                    rel = "stylesheet"
                    href = "https://maxcdn.bootstrapcdn.com/bootstrap/3.3.7/css/bootstrap.min.css"
                    integrity = "sha384-BVYiiSIFeK1dGmJRAkycuHAHRg32OmUcww7on3RYdg4Va+PmSTsz/K68vbdEjh4u"
                    crossorigin = "anonymous"
                />
                {ws("\n        ")}
                <title>"This is check50."</title>
                {ws("\n    ")}
            </head>
            {ws("\n    ")}
            <body inner_html = body></body>
            {ws("\n\n")}
        </html>
    });
    format!("<!DOCTYPE html>\n{doc}")
}

/// Renders the HTML report from the check results (check50 parity:
/// per-result chunks with emoji, description, cause, and log).
#[must_use]
pub(crate) fn render_html(input: &RenderInput) -> String {
    let results: Vec<HtmlResult> = input.results.iter().map(resolve_result).collect();
    html_document(input.slug, &results)
}

/// Resolves a `CheckResult` into the branch-resolved `HtmlResult`.
fn resolve_result(result: &CheckResult) -> HtmlResult {
    let log = result.log.clone();
    let description = result.description.clone();
    match (result.passed, &result.cause) {
        (Some(true), _) | (_, None) => HtmlResult::Pass { description, log },
        (Some(false), Some(cause)) => {
            let rationale = cause_rationale(cause);
            let help = cause_help(cause);
            let (mismatch, error) = match cause {
                Cause::Mismatch {
                    expected, actual, ..
                } => (Some((expected.clone(), actual.clone())), None),
                Cause::Error { error, .. } => {
                    (None, Some((error.kind.clone(), error.value.clone())))
                }
                _ => (None, None),
            };
            HtmlResult::Fail {
                description,
                rationale,
                help,
                mismatch,
                error,
                log,
            }
        }
        (_, Some(cause)) => {
            let rationale = cause_rationale(cause);
            HtmlResult::Skip {
                description,
                rationale,
                log,
            }
        }
    }
}

fn cause_rationale(cause: &Cause) -> String {
    match cause {
        Cause::Failure { rationale, .. }
        | Cause::Mismatch { rationale, .. }
        | Cause::Skipped { rationale }
        | Cause::Error { rationale, .. } => rationale.clone(),
    }
}

fn cause_help(cause: &Cause) -> Option<String> {
    match cause {
        Cause::Failure { help, .. } | Cause::Mismatch { help, .. } => help.clone(),
        _ => None,
    }
}

/// Generates a random 8-character lowercase alphanumeric suffix for
/// temp file names (matching Python's tempfile.mkstemp naming).
pub(crate) fn random_suffix() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..8)
        .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
        .collect()
}
