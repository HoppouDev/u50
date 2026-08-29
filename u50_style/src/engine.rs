//! Style-check driver: reads files, formats, and builds the report.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

/// Normalizes `source` exactly as style50 3.0.0's `_api.py` does before
/// formatting and comparison:
///
/// 1. rstrip every line (trailing whitespace, including `\r`, removed),
/// 2. join with `\n`,
/// 3. ensure a trailing `\n` (unless the result is empty).
///
/// Both the formatter and the clean/dirty comparison operate on the
/// normalized text, so trailing whitespace, CRLF line endings, and a
/// missing final newline are never flagged.
#[must_use]
pub fn normalize_source(source: &str) -> String {
    let mut normalized = source
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    if !normalized.is_empty() && !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

/// Directory arguments are expanded recursively before processing (see
/// [`expand_paths`]): every supported file inside a directory is checked,
/// deduplicated against the other arguments, and processed in sorted order.
///
/// Like [`run`], but injects the formatter so tests can run without the
/// external formatter binaries installed. Builds no output for the caller;
/// rendering lives on the [`FileResult`]s.
///
/// Per-file problems (unreadable file, unsupported extension, formatter
/// failure) are recorded in [`Report::errors`] and processing continues
/// with the remaining files, so earlier results are never discarded.
///
/// The per-file machinery (read, normalize, format, compare, render) is
/// shared with [`fix_with`] via [`process_file`]; [`FileResult::formatted`]
/// carries the styled content for every successfully processed file.
pub fn run_with(req: &Request, formatter: &dyn Formatter) -> Report {
    let files = expand_paths(&req.files);
    let mut results = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for path in &files {
        match process_file(path, formatter, req.output, req.color) {
            Ok(result) => results.push(result),
            Err(e) => errors.push((path.clone(), e.to_string())),
        }
    }
    Report { results, errors }
}

/// Formats, compares, and renders a single file: reads it, normalizes the
/// source (see [`normalize_source`]), formats it, and builds the
/// [`FileResult`] (styled content in [`FileResult::formatted`], diff in
/// [`FileResult::rendered`] when the file is dirty). Shared verbatim by
/// [`run_with`] (style check) and [`fix_with`] (in-place fix) so both see
/// identical clean/dirty semantics.
///
/// # Errors
/// Returns an error when the file cannot be read, has an unsupported
/// extension, is empty after normalization, or the formatter fails.
fn process_file(
    path: &Path,
    formatter: &dyn Formatter,
    output: Output,
    color: bool,
) -> anyhow::Result<FileResult> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("could not read `{}`: {e}", path.display()))?;
    let Some(language) = detect_language(path) else {
        anyhow::bail!(
            "unsupported file type `{}`; supported extensions: \
             c, h, cpp, hpp, java, py, js, html, css, sql",
            path.display()
        );
    };
    // style50 3.0.0 input normalization (`_api.py`); see
    // [`normalize_source`]. Empty/whitespace-only files normalize
    // to "" and are a per-file error ("file is empty").
    let normalized = normalize_source(&source);
    if normalized.trim().is_empty() {
        anyhow::bail!("file is empty");
    }
    let styled = formatter.format(&normalized, language)?;
    let clean = normalized == styled;
    let rendered = if clean {
        None
    } else {
        Some(match output {
            Output::Character => render_character(&normalized, &styled, color),
            Output::Split => render_split(&normalized, &styled, color),
            Output::Unified | Output::Json => render_unified(&normalized, &styled, path),
        })
    };
    Ok(FileResult {
        path: path.to_path_buf(),
        clean,
        rendered,
        formatted: Some(styled),
    })
}

/// Like [`run_with`], but rewrites dirty files in place with the styled
/// content instead of only reporting violations. Reuses the exact per-file
/// machinery of the style check via [`process_file`], so clean/dirty and
/// error semantics are identical. Mirrors the original style50's
/// `-i`/`--in-place` mode.
///
/// Per file: on error, the problem is recorded in [`Report::errors`] and
/// processing continues with the remaining files; if the file is clean,
/// nothing is written; if it is dirty and `dry_run` is false, the styled
/// content ([`FileResult::formatted`]) is written back — a write failure is
/// recorded in [`Report::errors`], never a bail. With `dry_run` true,
/// nothing is written at all; the report then reflects what *would* be
/// fixed (dirty files appear as `clean == false` results).
///
/// Directory arguments are expanded recursively before processing (see
/// [`expand_paths`]): every supported file inside a directory is fixed,
/// deduplicated against the other arguments, and processed in sorted order.
///
/// No printing happens here (see [`fix`] for the printing entry point), and
/// fix mode otherwise ignores the per-file diff rendering.
pub fn fix_with(req: &Request, formatter: &dyn Formatter, dry_run: bool) -> Report {
    let files = expand_paths(&req.files);
    let mut results = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for path in &files {
        match process_file(path, formatter, req.output, req.color) {
            Ok(result) => {
                let written = if result.clean || dry_run {
                    true
                } else {
                    match &result.formatted {
                        // `process_file` always sets `formatted` on success.
                        Some(styled) => match std::fs::write(path, styled) {
                            Ok(()) => true,
                            Err(e) => {
                                errors.push((
                                    path.clone(),
                                    format!("could not write `{}`: {e}", path.display()),
                                ));
                                false
                            }
                        },
                        None => true,
                    }
                };
                if written {
                    results.push(result);
                }
            }
            Err(e) => errors.push((path.clone(), e.to_string())),
        }
    }
    Report { results, errors }
}

/// Expands the request's file arguments before processing, matching
/// style50 3.0.0's directory handling (an `os.walk` expansion with
/// `followlinks=false`):
///
/// - a **directory** argument is walked recursively; only regular files
///   whose [`detect_language`] is `Some` are collected (the same
///   extension filtering style50 applies while walking), and hidden
///   directories are included (style50's `--ignore` is the exclusion
///   mechanism, which u50 does not implement yet);
/// - a **symlinked directory is not followed** (`symlink_metadata` says
///   it is a link, not a directory — matches `os.walk` defaults);
/// - anything else (a file, a symlink to a file, a missing path) is kept
///   unchanged, so explicit file arguments preserve their existing
///   per-file error semantics (unsupported extension, could not read);
/// - a directory containing zero supported files contributes nothing
///   (no error — style50 likewise skips unknown file types);
/// - **unreadable directories are skipped silently** (there is no error
///   channel here; this also matches `os.walk`'s ignored-error default);
/// - the final list is deduplicated (a directory and a file inside it can
///   both be named) and returned in sorted order for deterministic output.
pub(crate) fn expand_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    for path in paths {
        match std::fs::symlink_metadata(path) {
            // `symlink_metadata` never follows the link: a symlinked
            // directory reads as a link, not a directory, so it is kept
            // (and later errors per-file) instead of being walked.
            Ok(meta) if meta.is_dir() => walk_dir(path, &mut files),
            _ => {
                files.insert(path.clone());
            }
        }
    }
    files.into_iter().collect()
}

/// Recursively collects the supported regular files under `dir` into
/// `files` (regular only: symlinks, FIFOs, devices, etc. inside the
/// walked tree are skipped — `os.walk` with `followlinks=false` never
/// visits them either). Entries are visited in file-name order at each
/// level, and an unreadable directory is skipped silently (see
/// [`expand_paths`]).
fn walk_dir(dir: &Path, files: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        match std::fs::symlink_metadata(&path) {
            // `is_file()` (regular file only, not a symlink —
            // `symlink_metadata` never follows links) matches
            // `os.walk` `followlinks=false`, which leaves symlinked
            // entries unvisited, and also skips FIFOs/devices, which
            // could block on open when read.
            Ok(meta) if meta.is_dir() => walk_dir(&path, files),
            Ok(meta) if meta.is_file() => {
                if detect_language(&path).is_some() {
                    files.insert(path);
                }
            }
            // Unreadable entries and non-regular files (symlinks,
            // FIFOs, devices, …) contribute nothing.
            Ok(_) | Err(_) => {}
        }
    }
}

/// Runs the in-place fix for `req` using the CS50 formatter stack
/// ([`Cs50Formatter::from_env()`], honoring any `U50_STYLE_<LANG>`
/// overrides exactly like [`run`]), printing per-file outcomes (the only
/// place this crate prints) and returning the report so the caller can
/// decide the exit code. Mirrors the original style50's `-i`/`--in-place`.
///
/// Printing policy:
///
/// - plain fix (`dry_run == false`): each processed file prints to stdout
///   as `fixed: <path>` or `already clean: <path>` (diff rendering is
///   ignored).
/// - dry run (`dry_run == true`): nothing is written; for every file that
///   would change, the rendered diff is printed (per `req.output`; JSON
///   mode prints the JSON document of would-fix results only —
///   already-clean files are omitted).
/// - errors always go to stderr as `error: <path>: <message>`.
pub fn fix(req: &Request, dry_run: bool) -> Report {
    tracing::debug!(?req, dry_run, "u50_style::fix");
    let report = fix_with(req, &Cs50Formatter::from_env(), dry_run);
    if dry_run {
        if req.output == Output::Json {
            // The JSON document promises *would-fix* results only, so feed
            // it a report filtered to dirty files (errors were never part
            // of the document; they still go to stderr below).
            let would_fix = Report {
                results: report
                    .results
                    .iter()
                    .filter(|result| !result.clean)
                    .cloned()
                    .collect(),
                errors: Vec::new(),
            };
            println!("{}", json_document(&would_fix));
        } else {
            for result in &report.results {
                if !result.clean
                    && let Some(rendered) = &result.rendered
                {
                    print!("{rendered}");
                }
            }
        }
    } else {
        for result in &report.results {
            if result.clean {
                println!("already clean: {}", result.path.display());
            } else {
                println!("fixed: {}", result.path.display());
            }
        }
    }
    for (path, message) in &report.errors {
        eprintln!("error: {}: {message}", path.display());
    }
    report
}
