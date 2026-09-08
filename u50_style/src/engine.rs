//! Style-check driver: reads files, formats, and builds the report.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::format::{Cs50Formatter, Formatter, ensure_backends, locate_tool};
use crate::language::detect_language;
use crate::rendering::json_document;
use crate::rendering::{Renderer, builtin_renderer};
use crate::request::{FileResult, Output, Report, Request};

/// One per-file outcome of the parallel check pass ([`check_files`]):
/// either a [`FileResult`] or a `(path, message)` error entry.
type Outcome = (Option<FileResult>, Option<(PathBuf, String)>);

/// Runs the style check for `req` using the CS50 formatter stack,
/// printing results (the only place this crate prints) and returning the
/// report so the caller can decide the exit code.
///
/// Rendering is delegated to [`run_with_renderer`] with the built-in
/// renderer for `req.output` ([`builtin_renderer`]): rendered results for
/// every successfully processed file go to stdout (kept pure diff/JSON
/// output); per-file errors are written to stderr as
/// `error: <path>: <message>` lines after the results.
pub fn run(req: &Request) -> Report {
    tracing::debug!(?req, "u50_style::run");
    let mut renderer = builtin_renderer(req.output, req.color, Box::new(std::io::stdout().lock()));
    run_with_renderer(req, &Cs50Formatter, renderer.as_mut())
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
/// rendering is decoupled — drive a [`Renderer`] over the returned report
/// with [`run_with_renderer`].
///
/// Per-file problems (unreadable file, unsupported extension, formatter
/// failure) are recorded in [`Report::errors`] and processing continues
/// with the remaining files, so earlier results are never discarded.
///
/// The per-file machinery (read, normalize, format, compare) is shared with
/// [`fix_with`] via [`process_file`]; [`FileResult::formatted`] carries the
/// styled content for every successfully processed file.
pub fn run_with(req: &Request, formatter: &dyn Formatter) -> Report {
    let (files, _skipped) = expand_paths(&req.files);
    check_files(&files, formatter)
}

/// Processes already-expanded `files` (see [`expand_paths`]) into a
/// [`Report`]: the per-file pass shared verbatim by [`run_with`] (check)
/// and [`run_with_renderer`] (check + rendering), run in parallel with
/// rayon — a cold-cache provisioning pre-pass ([`provision_backends`])
/// fetches every missing backend in parallel first. Walk warnings are a
/// rendering concern: callers that render expand themselves (see
/// [`run_with_renderer`]) or print the warnings engine-side (see
/// [`fix_with`]); this helper never sees them.
fn check_files(files: &[PathBuf], formatter: &dyn Formatter) -> Report {
    provision_backends(files);
    // Rayon preserves input order in the collect, so results and errors
    // come out exactly as the sequential loop did (deterministic output).
    let outcomes: Vec<Outcome> = files
        .par_iter()
        .map(|path| match process_file(path, formatter) {
            Ok(result) => (Some(result), None),
            Err(e) => (None, Some((path.clone(), e.to_string()))),
        })
        .collect();
    let mut results = Vec::with_capacity(files.len());
    let mut errors = Vec::with_capacity(files.len());
    for (result, error) in outcomes {
        if let Some(result) = result {
            results.push(result);
        }
        if let Some(error) = error {
            errors.push(error);
        }
    }
    Report { results, errors }
}

/// Cold-cache provisioning pre-pass: runs **after** the walk
/// ([`expand_paths`] only classifies files — it never fetches) and
/// collects the distinct missing `(pip package, tool)` pairs of the
/// discovered files, then fetches them all **in parallel** through the
/// same uv pipeline `u50 --setup` uses
/// ([`crate::setup::install_backends`]: one fetch task per package, one
/// serialized venv install) — before [`check_files`] or [`fix_with`]
/// processes anything — so per-file first-use can never race concurrent
/// uv installs into the shared cache. Per-process dedupe and the
/// `U50_STYLE_NO_PROVISION` escape hatch live in
/// [`crate::format::ensure_backends`].
fn provision_backends(files: &[PathBuf]) {
    let mut seen = HashSet::new();
    let mut missing: Vec<(String, String)> = Vec::new();
    for path in files {
        let Some(language) = detect_language(path) else {
            continue;
        };
        let Some(tool) = language.required_tool() else {
            continue;
        };
        if seen.insert(tool) && locate_tool(tool).is_none() {
            missing.push((language.pip_package().to_owned(), tool.to_owned()));
        }
    }
    ensure_backends(&missing);
}

/// Runs the style check like [`run_with`], then drives `renderer` over the
/// outcome as a stream of [`Renderer`] events — the extension point for
/// custom output sinks (HTML, SARIF, an editor panel, ...): implement the
/// trait and pass it here, no engine changes needed.
///
/// Event order (see [`Renderer`]): `begin(req)` once, then
/// `total_files(count)` (results + errors; the count character mode needs
/// to decide on per-file headers), then one `skipped(path)` per
/// unsupported regular file found while walking a directory operand (in
/// walk order — the style50-parity `unknown file type ..., skipping...`
/// warning), then one `file(result)` per successfully processed file in
/// report order, then one `file_error(path, message)` per per-file error
/// in report order, then `finish(&report)` once. The built-in renderers
/// write the style50-parity console/JSON output ([`run`] uses
/// [`builtin_renderer`]).
pub fn run_with_renderer(
    req: &Request,
    formatter: &dyn Formatter,
    renderer: &mut dyn Renderer,
) -> Report {
    let (files, skipped) = expand_paths(&req.files);
    let report = check_files(&files, formatter);
    renderer.begin(req);
    // Results + errors (walk-warned unsupported files excluded) — the
    // count character mode uses to decide whether per-file headers apply.
    renderer.total_files(report.results.len() + report.errors.len());
    for path in &skipped {
        renderer.skipped(path);
    }
    // Per-file events in input order (results and errors interleaved, the
    // order style50's `files` list carries — the HTML report renders each
    // file at its position). Per-stream content is unchanged for every
    // built-in renderer: results go to stdout, errors to stderr, and both
    // sequences keep their report order.
    let results: HashMap<&Path, &FileResult> = report
        .results
        .iter()
        .map(|result| (result.path.as_path(), result))
        .collect();
    let errors: HashMap<&Path, &String> = report
        .errors
        .iter()
        .map(|(path, message)| (path.as_path(), message))
        .collect();
    for path in &files {
        if let Some(result) = results.get(path.as_path()) {
            renderer.file(result);
        } else if let Some(message) = errors.get(path.as_path()) {
            renderer.file_error(path.as_path(), message.as_str());
        }
    }
    renderer.finish(&report);
    report
}

/// Reads, normalizes (see [`normalize_source`]), formats, and compares a
/// single file, building the [`FileResult`]: the normalized input is kept
/// in [`FileResult::source`], the styled content in
/// [`FileResult::formatted`] — rendering (the diff/JSON presentation) is
/// the renderer's job, not this function's. Shared verbatim by
/// [`run_with`] (style check) and [`fix_with`] (in-place fix) so both see
/// identical clean/dirty semantics.
///
/// # Errors
/// Returns an error when the file cannot be read, has an unsupported
/// extension, is empty after normalization, or the formatter fails.
fn process_file(path: &Path, formatter: &dyn Formatter) -> anyhow::Result<FileResult> {
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
    Ok(FileResult {
        path: path.to_path_buf(),
        clean,
        source: Some(normalized),
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
/// deduplicated against the other arguments, and processed in sorted
/// order; unsupported regular files encountered during the walk are
/// warned (never errors) and skipped, without affecting the exit code.
///
/// One exception to no printing: unsupported regular files found while
/// walking a directory operand are warned to stderr
/// (`unknown file type "<path>", skipping...`, style50 parity) before the
/// per-file fix lines; dry runs warn too. Otherwise no printing happens
/// here (see [`fix`] for the printing entry point), and fix mode ignores
/// the per-file diff rendering.
pub fn fix_with(req: &Request, formatter: &dyn Formatter, dry_run: bool) -> Report {
    let (files, skipped) = expand_paths(&req.files);
    for path in &skipped {
        eprintln!("unknown file type \"{}\", skipping...", path.display());
    }
    // Same walk → provision → process pipeline as the check pass: the
    // missing backends are fetched in parallel before the sequential
    // (in-place) fix loop starts.
    provision_backends(&files);
    let mut results = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for path in &files {
        match process_file(path, formatter) {
            Ok(result) => {
                let written = if result.clean || dry_run {
                    true
                } else {
                    match &result.formatted {
                        // `process_file` always sets `formatted` on success.
                        Some(styled) => match write_atomic(path, styled) {
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
/// Returns `(files, skipped)`: `files` is the deduplicated, sorted list
/// to process; `skipped` holds the walked **regular files** whose
/// [`detect_language`] is `None`, in walk order, for the caller to emit
/// style50's `unknown file type "<path>", skipping...` warning (the
/// channel is the caller's: stderr in text/fix modes, stdout in score
/// mode; explicit operands are never reported there).
///
/// - a **directory** argument is walked recursively; only regular files
///   whose [`detect_language`] is `Some` are collected (the same
///   extension filtering style50 applies while walking, plus its per-file
///   warning for the skipped ones), and hidden
///   directories are included (style50's `--ignore` is the exclusion
///   mechanism, which u50 does not implement yet);
/// - a **symlinked directory operand is followed** (`metadata` resolves
///   links, matching `os.walk`'s top-level behavior — style50 3.0.0
///   formats the targets too), but **symlinked subdirectories inside the
///   walk are never descended into** (`followlinks=false`), and
///   **symlinked regular files are checked** like any file (style50
///   filters by name, and the resolved target is read through the link);
/// - anything else (a file, a symlink to a file, a missing path) is kept
///   unchanged, so explicit file arguments preserve their existing
///   per-file error semantics (unsupported extension, could not read);
/// - a directory containing zero supported files contributes nothing
///   (no error — style50 likewise skips unknown file types, warning once
///   per skipped regular file; the warning never affects exit codes);
/// - **unreadable directories are skipped silently** (there is no error
///   channel here; this also matches `os.walk`'s ignored-error default);
/// - the final list is deduplicated **by canonical path** (a directory
///   and a file inside it can both be named, and equivalent spellings
///   like `dir` and `./dir/a.c` resolve to the same file; unresolvable
///   paths fall back to their raw spelling) and returned in sorted order
///   for deterministic output.
pub(crate) fn expand_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    let mut skipped: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for path in paths {
        // `metadata` (unlike `symlink_metadata`) resolves links, so a
        // symlinked directory operand is walked — matching `os.walk`'s
        // top-level behavior. Anything unreadable or non-directory is
        // kept unchanged for its per-file error semantics.
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => walk_dir(path, &mut files, &mut skipped, &mut seen),
            _ => {
                let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                if seen.insert(key) {
                    files.insert(path.clone());
                }
            }
        }
    }
    (files.into_iter().collect(), skipped)
}

/// Recursively collects the supported regular files under `dir` into
/// `files`, appending unsupported regular files to `skipped` in visit
/// order. Symlinked regular files are resolved through the link and
/// collected (or warned about) like any file; symlinked subdirectories
/// are never descended into (`os.walk` with `followlinks=false`), and
/// broken links, FIFOs, and devices contribute nothing. Entries are
/// visited in file-name order at each level, and an unreadable directory
/// is skipped silently (see [`expand_paths`]). Every collected or
/// skipped path is deduplicated against `seen` by its canonical path.
fn walk_dir(
    dir: &Path,
    files: &mut BTreeSet<PathBuf>,
    skipped: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk_dir(&path, files, skipped, seen);
        } else if file_type.is_file() {
            classify_file(&path, files, skipped, seen);
        } else if file_type.is_symlink() {
            // Resolve the link: a symlinked regular file is checked like
            // any file (style50 filters by name); a symlinked directory
            // is never descended into (`followlinks=false`); broken
            // links and special targets contribute nothing.
            if std::fs::metadata(&path).is_ok_and(|meta| meta.is_file()) {
                classify_file(&path, files, skipped, seen);
            }
        }
        // Non-regular entries (FIFOs, devices, …) contribute nothing.
    }
}

/// Collects (or walk-warns) one file: deduplicated by canonical path so
/// equivalent spellings of the same file are processed once; unresolvable
/// paths fall back to their raw spelling.
fn classify_file(
    path: &Path,
    files: &mut BTreeSet<PathBuf>,
    skipped: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if detect_language(path).is_some() {
        if seen.insert(key) {
            files.insert(path.to_path_buf());
        }
    } else if seen.insert(key) {
        skipped.push(path.to_path_buf());
    }
}

/// Writes `styled` to `path` without ever leaving it truncated: the
/// content lands in a sibling temp file first and is then renamed onto
/// the target — a rename within one directory is atomic, so a failed or
/// interrupted write leaves the original byte-for-byte intact and a
/// successful one is all-or-nothing (`std::fs::write` would truncate at
/// open, before the first byte lands).
fn write_atomic(path: &Path, styled: &str) -> std::io::Result<()> {
    // A read-only target fails BEFORE anything is written. `std::fs::write`
    // used to fail with EACCES at open; the rename below would silently
    // replace a read-only file (rename needs directory, not file, write
    // permission) — so the mode is checked up front to keep the old
    // failure semantics under the new atomicity.
    if std::fs::metadata(path).is_ok_and(|meta| meta.permissions().readonly()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "file is read-only",
        ));
    }
    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".u50-tmp");
    let temp = PathBuf::from(temp_name);
    let result = std::fs::write(&temp, styled).and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        // Never leave the temp sibling behind on a failed write or rename.
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Runs the in-place fix for `req` using the CS50 formatter stack
/// ([`Cs50Formatter`], exactly like [`run`]), printing per-file
/// outcomes (the only place this crate prints) and returning the report so
/// the caller can decide the exit code. Mirrors the original style50's
/// `-i`/`--in-place`.
///
/// Printing policy:
///
/// - plain fix (`dry_run == false`): each processed file prints to stdout
///   as `fixed: <path>` or `already clean: <path>` (diff rendering is
///   ignored).
/// - dry run (`dry_run == true`, text modes): nothing is written; each
///   processed file prints to stdout as `would fix: <path>` or
///   `already clean: <path>` — no diff rendering (the exit code 1
///   signals what would have changed).
/// - dry run in JSON mode (`dry_run == true`, `Output::Json`): the JSON
///   document of would-fix results only — no status lines (the output is
///   machine-readable; already-clean files are omitted).
/// - errors always go to stderr as `error: <path>: <message>`.
/// - walk warnings (`unknown file type "<path>", skipping...` for each
///   unsupported regular file found while expanding directory operands)
///   are printed by [`fix_with`] to stderr before the fix lines above.
pub fn fix(req: &Request, dry_run: bool) -> Report {
    tracing::debug!(?req, dry_run, "u50_style::fix");
    let report = fix_with(req, &Cs50Formatter, dry_run);
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
                if result.clean {
                    println!("already clean: {}", result.path.display());
                } else {
                    println!("would fix: {}", result.path.display());
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
