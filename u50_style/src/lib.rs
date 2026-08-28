#![warn(clippy::pedantic)]

/// Diff output format for `u50 style`.
#[derive(Debug, Clone, Copy)]
pub enum Output {
    /// One-character-per-line diff style (style50 default).
    Character,
    /// Side-by-side split diff.
    Split,
    /// Unified diff.
    Unified,
    /// Machine-readable JSON (u50 addition for tooling).
    Json,
}

/// Parameters for a `u50 style` invocation.
#[derive(Debug, Clone)]
pub struct Request {
    /// Files to style-check.
    pub files: Vec<std::path::PathBuf>,
    /// Diff output format.
    pub output: Output,
}

/// Checks code style for `req`, reporting violations.
///
/// # Errors
/// Returns an error until the style engine is implemented.
pub fn run(req: &Request) -> anyhow::Result<()> {
    tracing::debug!(?req, "u50_style::run");
    anyhow::bail!("`u50 style` is not implemented yet")
}
