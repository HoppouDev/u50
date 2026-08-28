#![warn(clippy::pedantic)]

mod engine;
mod formatter;
mod language;
mod render;
mod request;

#[cfg(test)]
mod tests;

pub use engine::{run, run_with};
pub use formatter::{Cs50Formatter, Formatter};
pub use language::{Language, detect_language};
pub use request::{FileResult, Output, Report, Request};
