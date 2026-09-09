//! The compiled-in plugin registries: the single place that names every
//! plugin (the `gate.AddPlugin` analog). Registering a plugin here is
//! the only core change adding a language or renderer requires — the
//! plugin module itself carries everything else.

use crate::language::LanguagePlugin;
use crate::language::{c, css, html, javascript, python, rust, sql};
use crate::rendering::renderer::RendererPlugin;
use crate::rendering::renderer::{console, html as html_renderer, json, score};

/// The compiled-in language plugins, in listing order: `--status`, the
/// extension-detection precedence, and all per-language iteration
/// derive from this list.
pub(crate) fn languages() -> &'static [&'static dyn LanguagePlugin] {
    const PLUGINS: [&'static dyn LanguagePlugin; 9] = [
        &c::C_PLUGIN,
        &c::CPP_PLUGIN,
        &c::JAVA_PLUGIN,
        &python::PLUGIN,
        &javascript::PLUGIN,
        &html::PLUGIN,
        &css::PLUGIN,
        &sql::PLUGIN,
        &rust::PLUGIN,
    ];
    &PLUGINS
}

/// The compiled-in renderer plugins, one per output format (console
/// serves the three text modes).
pub(crate) fn renderers() -> &'static [&'static dyn RendererPlugin] {
    const PLUGINS: [&'static dyn RendererPlugin; 4] = [
        &console::PLUGIN,
        &json::PLUGIN,
        &html_renderer::PLUGIN,
        &score::PLUGIN,
    ];
    &PLUGINS
}
