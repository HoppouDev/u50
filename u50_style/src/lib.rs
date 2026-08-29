#![warn(clippy::pedantic)]

mod engine;
mod formatter;
mod language;
mod listing;
mod render;
mod request;
mod setup;

#[cfg(test)]
mod tests;

pub use engine::{fix, fix_with, normalize_source, run, run_with};
pub use formatter::{Cs50Formatter, Formatter, ToolOrigin, locate_tool};
pub use language::{Language, detect_language};
pub use listing::list_languages;
pub use request::{FileResult, Output, Report, Request};
pub use setup::setup_missing;
