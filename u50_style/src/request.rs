//! Request parameters and result/report types for style checking.

use std::path::PathBuf;

/// Diff output format for `u50 style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// One-character-per-line diff style (style50 default).
    Character,
    /// Side-by-side split diff.
    Split,
    /// Unified diff.
    Unified,
    /// Machine-readable JSON (also present in the original via `-o json`).
    Json,
}

/// Parameters for a `u50 style` invocation.
#[derive(Debug, Clone)]
pub struct Request {
    /// Files to style-check.
    pub files: Vec<PathBuf>,
    /// Diff output format.
    pub output: Output,
    /// Whether ANSI colors are enabled for text output modes (JSON output
    /// is never colored).
    pub color: bool,
}

/// The style-check outcome for a single file.
#[derive(Debug, Clone)]
pub struct FileResult {
    /// The checked file.
    pub path: PathBuf,
    /// Whether the file already conforms to CS50 style.
    pub clean: bool,
    /// Human-readable per-file output for text modes (or the unified patch
    /// in JSON mode); `None` when the file is clean.
    pub rendered: Option<String>,
}

/// The aggregated style-check outcome for a request.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// One entry per requested file.
    pub results: Vec<FileResult>,
}

impl Report {
    /// Whether every requested file is clean (true for an empty request).
    #[must_use]
    pub fn clean(&self) -> bool {
        self.results.iter().all(|r| r.clean)
    }
}
