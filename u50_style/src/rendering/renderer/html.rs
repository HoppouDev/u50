//! The HTML report renderer (style50's `results.html` template through
//! leptos).

use std::io::Write;
use std::path::Path;

use leptos::prelude::*;
use leptos::tachys::view::Position;

use crate::language::{comment_hint, detect_language};
use crate::rendering::doc_flavor::markup_escape;
use crate::rendering::html_diff::render_html_diff;
use crate::request::{FileResult, Report};

use super::Renderer;

/// One per-file entry of the HTML report: the template's
/// `{% if "error" in file %}` / `{% elif file.score == 1.0 %}` / else
/// branches, pre-resolved.
pub(crate) enum HtmlFile {
    /// `file.score == 1.0`: `<pre style="color: #32cf55">Looks good!</pre>`.
    Clean {
        name: String,
        /// `file.comments` (the comment-ratio hint).
        comments: bool,
    },
    /// Violations found: `<pre>{{ file.diff|safe }}</pre>` — `diff` is the
    /// raw `html_diff` HTML (inserted via `inner_html`, never escaped).
    Dirty {
        name: String,
        diff: String,
        comments: bool,
    },
    /// `"error" in file`: `<pre style="color: yellow">` + the escaped error.
    Error { name: String, error: String },
}

/// The `<style>` element content of style50's `results.html` template
/// (verbatim; no HTML-special characters, so leptos text serialization is
/// byte-identical to the Jinja render).
const HTML_STYLE: &str = concat!(
    "\n            ins {\n",
    "                text-decoration: none;\n",
    "                background-color: #27A844;\n",
    "            }\n",
    "            del {\n",
    "                text-decoration: none;\n",
    "                background-color: #DC3546;\n",
    "            }\n",
    "            pre {\n",
    "                border: none;\n",
    "                background-color: inherit;\n",
    "                color: inherit;\n",
    "            }\n",
    "        "
);

/// Serializes a leptos view to an HTML string (SSR path: `to_html_with_buf`
/// with escaping enabled and no branch markers).
fn render_fragment<V: RenderHtml>(view: V) -> String {
    let mut buf = String::new();
    let mut position = Position::FirstChild;
    view.to_html_with_buf(&mut buf, &mut position, true, false, Vec::new());
    buf
}

/// Whitespace text nodes: leptos strips whitespace between tags in `view!`,
/// so every template-space run the Jinja render emits is passed as a
/// dynamic text child.
fn ws(text: &str) -> String {
    text.to_owned()
}

/// Renders one per-file entry exactly as style50's `results.html` template
/// does per loop iteration (`results.html:27-45`): whitespace text nodes
/// around the `<h3>` (the template's `{{ file.name }}` keeps its
/// surrounding spaces, which become part of the `inner_html`), the styled
/// `<div>`, and the branch-resolved body. The template's container/row
/// `<div>`s are never closed (`results.html:25-45`), so the document body
/// interior is assembled as raw HTML — a leptos element would be
/// serialized with closing tags, which the original never has.
fn html_file_chunk(entry: &HtmlFile) -> String {
    // Whitespace runs and the styled `<pre>`s are raw strings: the
    // template's exact indentation is reproduced verbatim, and leptos'
    // `style` serialization appends a trailing `;" the template does not
    // have. The dynamic parts (the escaped name, the diff HTML) are
    // leptos views.
    let mut chunk = String::from("\n                ");
    match entry {
        HtmlFile::Clean { name, comments } => {
            let name_html = format!(" {} ", markup_escape(name));
            chunk.push_str(&render_fragment(view! {
                <h3 inner_html = name_html></h3>
            }));
            chunk.push_str("\n                <div style=\"background-color: black; color: white;\">\n                    \n                        <pre style=\"color: #32cf55\">Looks good!</pre>\n                        ");
            if *comments {
                chunk.push_str("\n                        <pre style=\"color: yellow\">But consider adding more comments!</pre>\n                        ");
            }
            chunk.push_str("\n                    ");
        }
        HtmlFile::Dirty {
            name,
            diff,
            comments,
        } => {
            let name_html = format!(" {} ", markup_escape(name));
            chunk.push_str(&render_fragment(view! {
                <h3 inner_html = name_html></h3>
            }));
            chunk.push_str("\n                <div style=\"background-color: black; color: white;\">\n                    \n                        ");
            chunk.push_str(&render_fragment(view! {
                <pre inner_html = diff.as_str()></pre>
            }));
            chunk.push_str("\n                        ");
            if *comments {
                chunk.push_str("\n                            <pre style=\"color: yellow\">And consider adding more comments!</pre>\n                        ");
            }
            chunk.push_str("\n                    ");
        }
        HtmlFile::Error { name, error } => {
            let name_html = format!(" {} ", markup_escape(name));
            chunk.push_str(&render_fragment(view! {
                <h3 inner_html = name_html></h3>
            }));
            // Raw (like the other styled pres): leptos' style
            // serialization appends a trailing ; the template does not
            // have.
            chunk.push_str("\n                <div style=\"background-color: black; color: white;\">\n                    \n                        <pre style=\"color: yellow\">");
            chunk.push_str(&markup_escape(error));
            chunk.push_str("</pre>\n                    ");
        }
    }
    chunk.push_str("\n                </div>\n            ");
    chunk
}

/// Assembles the full document: the template through `<hr>`, one chunk per
/// file, and the tail (the template's trailing newline is stripped by
/// Jinja's `keep_trailing_newline=False` default, so the output ends
/// without one). The doctype is prepended as a constant.
fn html_document(entries: &[HtmlFile]) -> String {
    let mut body = String::from(
        "\n        <div class=\"container\">\n            <div class=\"row\">\n\n            <h1>style50</h1>\n            <hr>\n            ",
    );
    for entry in entries {
        body.push_str(&html_file_chunk(entry));
    }
    body.push_str("\n    ");
    let doc = render_fragment(view! {
        <html>{ws("\n    ")}<head>
                {ws("\n        ")}
                <link
                    rel = "stylesheet"
                    href = "https://maxcdn.bootstrapcdn.com/bootstrap/3.3.7/css/bootstrap.min.css"
                    integrity = "sha384-BVYiiSIFeK1dGmJRAkycuHAHRg32OmUcww7on3RYdg4Va+PmSTsz/K68vbdEjh4u"
                    crossorigin = "anonymous"
                />
                {ws("\n        ")}
                <style inner_html = HTML_STYLE></style>
                {ws("\n\n        ")}
                <title>"This is style50."</title>
                {ws("\n    ")}
            </head>{ws("\n    ")}<body inner_html = body></body>{ws("\n\n")}</html>
    });
    format!("<!DOCTYPE html>\n{doc}")
}

/// Writes the style50-compatible HTML report (style50's `results.html`
/// template, rendered through **leptos**: the head, the per-file `<h3>`/
/// `<div>` chunks, and the branch-resolved bodies are leptos views
/// serialized via `RenderHtml::to_html_with_buf`; see `html_file_chunk`
/// and `html_document`). Per-file entries are driven in input order —
/// results and errors interleaved, exactly like style50's `files` list.
/// The report goes to stdout; style50 instead writes a temp file and opens
/// a browser (documented divergence). Skipped walk warnings have no
/// element in the template and are ignored (as in [`JsonRenderer`]).
pub struct HtmlRenderer {
    pub(crate) entries: Vec<HtmlFile>,
    pub(crate) out: Box<dyn Write>,
}

impl Renderer for HtmlRenderer {
    fn file(&mut self, result: &FileResult) {
        let name = result.path.display().to_string();
        let comments = result
            .source
            .as_deref()
            .zip(detect_language(&result.path))
            .is_some_and(|(source, language)| comment_hint(source, language));
        if result.clean {
            self.entries.push(HtmlFile::Clean { name, comments });
            return;
        }
        let diff = match (&result.source, &result.formatted) {
            (Some(source), Some(formatted)) => render_html_diff(source, formatted),
            // Unreachable through the engine (dirty files carry both).
            _ => String::new(),
        };
        self.entries.push(HtmlFile::Dirty {
            name,
            diff,
            comments,
        });
    }

    fn file_error(&mut self, path: &Path, message: &str) {
        self.entries.push(HtmlFile::Error {
            name: path.display().to_string(),
            error: message.to_owned(),
        });
    }

    fn finish(&mut self, _report: &Report) {
        let _ = write!(self.out, "{}", html_document(&self.entries));
    }
}
