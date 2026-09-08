//! Output rendering: the shared palette and diff renderers, plus the
//! pluggable report renderers under [`renderer`].
//!
//! Rendering is decoupled from processing ([`crate::engine::run_with`]):
//! [`crate::engine::run_with_renderer`] drives any [`Renderer`] over a
//! report, so custom sinks (HTML, SARIF, an editor panel, ...) need only
//! implement the trait — no changes to the engine.

pub(crate) mod character;
pub(crate) mod doc_flavor;
pub(crate) mod html_diff;
pub(crate) mod line_diff;
pub(crate) mod palette;
pub(crate) mod renderer;
pub(crate) mod split;
pub(crate) mod unified;

pub(crate) use renderer::json::json_document;
pub(crate) use renderer::{Renderer, builtin_renderer};

/// The visible marker text for a warned character (style50 renders the
/// literal two-character sequences `\n` / `\t` instead of the raw control
/// characters), shared by the character and HTML diff renderers.
const NEWLINE_MARKER: &str = "\\n";
/// The visible marker text for a warned tab, paired with
/// [`NEWLINE_MARKER`].
const TAB_MARKER: &str = "\\t";
