#![warn(clippy::pedantic)]

mod diff;
mod engine;
mod format;
mod language;
mod listing;
mod rendering;
mod request;
mod setup;

#[cfg(test)]
mod tests;

pub use engine::{fix, fix_with, normalize_source, run, run_with, run_with_renderer};
pub use format::locate_tool;
pub use format::{Cs50Formatter, Formatter, ToolOrigin};
pub use language::{Language, detect_language};
pub use listing::list_languages;
pub use rendering::renderer::{
    ConsoleRenderer, HtmlRenderer, JsonRenderer, Renderer, ScoreRenderer, builtin_renderer,
};
pub use request::{FileResult, Output, Report, Request};
pub use setup::setup_missing;
