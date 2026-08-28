//! Style-check driver: reads files, formats, and builds the report.

use crate::formatter::{Cs50Formatter, Formatter};
use crate::language::detect_language;
use crate::render::{json_document, render_character, render_split, render_unified};
use crate::request::{FileResult, Output, Report, Request};

/// Runs the style check for `req` using the CS50 formatter stack
/// printing results (the only place this crate prints) and returning the
/// report so the caller can decide the exit code.
///
/// # Errors
/// Returns an error for unreadable files, unsupported file types, or
/// formatter failures.
pub fn run(req: &Request) -> anyhow::Result<Report> {
    tracing::debug!(?req, "u50_style::run");
    let report = run_with(req, &Cs50Formatter::from_env())?;
    if req.output == Output::Json {
        println!("{}", json_document(&report));
    } else {
        for result in &report.results {
            if let Some(rendered) = &result.rendered {
                print!("{rendered}");
            }
        }
    }
    Ok(report)
}

/// Like [`run`], but injects the formatter so tests can run without the
/// external formatter binaries installed. Builds no output for the caller; rendering lives
/// on the [`FileResult`]s.
///
/// # Errors
/// Returns an error for unreadable files, unsupported file types, or
/// formatter failures.
pub fn run_with(req: &Request, formatter: &dyn Formatter) -> anyhow::Result<Report> {
    let mut results = Vec::with_capacity(req.files.len());
    for path in &req.files {
        let source = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("could not read `{}`: {e}", path.display()))?;
        let Some(language) = detect_language(path) else {
            anyhow::bail!(
                "unsupported file type `{}`; supported extensions: \
                 c, h, cpp, hpp, cc, cxx, java, py, js",
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
        results.push(FileResult {
            path: path.clone(),
            clean,
            rendered,
        });
    }
    Ok(Report { results })
}
