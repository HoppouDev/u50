//! Style-check driver: reads files, formats, and builds the report.

use crate::formatter::{Cs50Formatter, Formatter};
use crate::language::detect_language;
use crate::render::{json_document, render_character, render_split, render_unified};
use crate::request::{FileResult, Output, Report, Request};

/// Runs the style check for `req` using the CS50 formatter stack,
/// printing results (the only place this crate prints) and returning the
/// report so the caller can decide the exit code.
///
/// Rendered results for every successfully processed file go to stdout
/// (kept pure diff/JSON output); per-file errors are written to stderr as
/// `error: <path>: <message>` lines after the results.
pub fn run(req: &Request) -> Report {
    tracing::debug!(?req, "u50_style::run");
    let report = run_with(req, &Cs50Formatter::from_env());
    if req.output == Output::Json {
        println!("{}", json_document(&report));
    } else {
        for result in &report.results {
            if let Some(rendered) = &result.rendered {
                print!("{rendered}");
            }
        }
    }
    for (path, message) in &report.errors {
        eprintln!("error: {}: {message}", path.display());
    }
    report
}

/// Like [`run`], but injects the formatter so tests can run without the
/// external formatter binaries installed. Builds no output for the caller; rendering lives
/// on the [`FileResult`]s.
///
/// Per-file problems (unreadable file, unsupported extension, formatter
/// failure) are recorded in [`Report::errors`] and processing continues
/// with the remaining files, so earlier results are never discarded.
pub fn run_with(req: &Request, formatter: &dyn Formatter) -> Report {
    let mut results = Vec::with_capacity(req.files.len());
    let mut errors = Vec::new();
    for path in &req.files {
        let outcome = (|| -> anyhow::Result<FileResult> {
            let source = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("could not read `{}`: {e}", path.display()))?;
            let Some(language) = detect_language(path) else {
                anyhow::bail!(
                    "unsupported file type `{}`; supported extensions: \
                     c, h, cpp, hpp, java, py, js",
                    path.display()
                );
            };
            let expected = formatter.format(&source, language)?;
            let clean = source == expected;
            let rendered = if clean {
                None
            } else {
                Some(match req.output {
                    Output::Character => render_character(&source, &expected, req.color),
                    Output::Split => render_split(&source, &expected, req.color),
                    Output::Unified | Output::Json => render_unified(&source, &expected, path),
                })
            };
            Ok(FileResult {
                path: path.clone(),
                clean,
                rendered,
            })
        })();
        match outcome {
            Ok(result) => results.push(result),
            Err(e) => errors.push((path.clone(), e.to_string())),
        }
    }
    Report { results, errors }
}
