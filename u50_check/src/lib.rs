#![warn(clippy::pedantic)]

//! `u50_check`: the check50 port. Languages are irrelevant here —
//! *check sets* are the plugins: one module per problem's checks,
//! registered in [`registry`], executed by [`runner`] against student
//! code, and rendered through [`render`] in check50's output formats.

pub mod api;
mod checks;
mod graph;
mod plugin;
pub mod python;
mod registry;
mod render;
pub mod result;
mod runner;
mod yaml;

use std::io::IsTerminal as _;

use anyhow::Context as _;

pub use api::{CheckContext, EOF, Eof, decimal_regex};
pub use plugin::{CheckSetPlugin, CheckSpec, RunKind, YamlStep};
pub use result::{Cause, CheckResult, EngineError};

/// Execution mode for `u50 check`, replacing the original tool's four
/// mutually-exclusive boolean mode flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Fetch checks from the cs50 check server.
    Online,
    /// Build and run checks from a local check directory.
    Local,
    /// Run checks entirely offline (no network access).
    Offline,
    /// Developer mode (uncommitted check changes).
    Dev,
}

/// Output format for `u50 check` results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Human-readable colored terminal output.
    Ansi,
    /// HTML report.
    Html,
    /// Machine-readable JSON (same schema as check50's `--output json`).
    Json,
}

/// Parameters for a `u50 check` invocation.
///
/// The flag fields mirror the CLI's invocation options one-to-one (see
/// `u50_cli`'s dispatch), so the surface churns only when the CLI does.
#[derive(Debug, Clone)]
pub struct Request {
    /// Problem slug. In local/dev modes this is the path to the check
    /// directory (the `.cs50.yaml` parent); in online mode it is the
    /// cs50 slug (remote execution is a documented divergence: not
    /// implemented).
    pub slug: String,
    /// Working directory for student code (`None` means the current
    /// directory — check50 operates on the cwd by default).
    pub work_dir: Option<std::path::PathBuf>,
    /// Execution mode.
    pub mode: Mode,
    /// Named checks to run (plus dependencies); empty means all.
    pub targets: Vec<String>,
    /// Output formats to render.
    pub outputs: Vec<Output>,
    /// Write output to a file instead of stdout (`None` means stdout).
    pub output_file: Option<std::path::PathBuf>,
    /// List available checks and exit (`--verbose`'s companion view).
    pub verbose: bool,
    /// Print the check log (`--log`).
    pub show_log: bool,
    /// Log verbosity for the check run (`--log-level`; `None` = default).
    pub log_level: Option<String>,
}

/// The outcome of a check run: `true` when every executed check passed.
///
/// The rendered outputs are written to the request's output file (or
/// stdout), and the exit-code mapping (check50: exit 1 when any check is
/// not passed) happens in the caller.
///
/// # Errors
/// Returns an error for infrastructure failures (unresolvable check
/// directory, unwritable output). Check *failures* are not errors —
/// they are rendered and reported through the returned bool.
///
/// # Panics
/// Panics if the current directory cannot be determined (an OS-level
/// failure; check50's same assumption is implicit).
pub fn run(req: &Request) -> anyhow::Result<bool> {
    tracing::debug!(?req, "u50_check::run");

    // Remote (online) execution is a documented divergence.
    if req.mode == Mode::Online {
        anyhow::bail!(
            "remote check execution is not supported; run with --local, --offline, or --dev"
        );
    }

    // Resolve the check directory: dev/local/offline all treat the slug
    // as a local path to the check directory (the .cs50.yaml parent).
    let check_dir = std::path::PathBuf::from(&req.slug);
    anyhow::ensure!(
        check_dir.is_dir(),
        "{} is not a directory (the check directory must contain a .cs50.yaml)",
        check_dir.display()
    );

    // Load the config and build the plugin: YAML simple checks are
    // interpreted natively; native check sets come from the registry.
    let config = yaml::load_config(&check_dir)?;
    let yaml_specs = yaml::specs_from_config(&config)?;
    let (_plugin_id, specs, plugin_check_dir) =
        resolve_specs(&config, &yaml_specs, &check_dir, &req.slug)?;

    let work_dir = req
        .work_dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("current directory is readable"));

    // Run the checks (declaration order, dependency scheduling, skip
    // cascade, timeouts).
    let results = runner::run_checks(&specs, &plugin_check_dir, &work_dir, &req.targets);

    // check50's should_fail: an error anywhere, or any check not passed.
    let passed = !results.is_empty() && results.iter().all(|result| result.passed == Some(true));

    // Render each requested output format.
    let input = render::RenderInput {
        slug: &req.slug,
        results: &results,
        version: env!("CARGO_PKG_VERSION"),
        error: None,
        show_log: req.show_log,
        color: std::io::stdout().is_terminal(),
    };
    let mut rendered = String::new();
    for output in &req.outputs {
        match output {
            Output::Json => {
                rendered.push_str(&render::json::render_json(&input));
                rendered.push('\n');
            }
            Output::Ansi => {
                rendered.push_str(&render::ansi::render_ansi(&input));
                rendered.push('\n');
            }
            Output::Html => {
                // The html renderer is deferred (see
                // docs/U50_CHECK_PLUGIN_PLAN.md Phase 5).
                anyhow::bail!("html output is not implemented yet");
            }
        }
    }
    if let Some(file) = &req.output_file {
        std::fs::write(file, rendered)
            .with_context(|| format!("could not write {}", file.display()))?;
    } else {
        use std::io::Write as _;
        std::io::stdout()
            .write_all(rendered.as_bytes())
            .context("could not write to stdout")?;
    }

    Ok(passed)
}

/// Resolves the check set for a request: a `checks:` string naming a
/// Python checks file routes to the provisioned-CPython bridge (the
/// `check50` package, Phases 0-2 of `docs/U50_CHECK_PYTHON_PLAN.md`); a
/// string naming a native registered check set uses the registry; a
/// `checks:` mapping is a YAML simple-check set interpreted natively.
///
/// # Errors
/// Returns an error when the config names neither an existing Python
/// checks file nor a registered check set, or when discovery fails.
fn resolve_specs(
    config: &yaml::Config,
    yaml_specs: &[plugin::CheckSpec],
    check_dir: &std::path::Path,
    slug: &str,
) -> anyhow::Result<(String, Vec<plugin::CheckSpec>, std::path::PathBuf)> {
    if !(yaml_specs.is_empty()
        && config
            .check50
            .checks
            .as_ref()
            .is_some_and(serde_yaml::Value::is_string))
    {
        // YAML check set: the plugin is constructed from the config.
        return Ok((
            slug.to_owned(),
            yaml_specs.to_vec(),
            check_dir.to_path_buf(),
        ));
    }

    let checks_file = config
        .check50
        .checks
        .as_ref()
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let file = check_dir.join(checks_file);
    if std::path::Path::new(checks_file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
        && file.is_file()
    {
        let py_specs = python::bridge::discover(&file)
            .with_context(|| format!("could not discover checks from {}", file.display()))?;
        let specs = py_specs
            .into_iter()
            .map(|spec| plugin::CheckSpec {
                name: spec.name.clone(),
                description: spec.description,
                dependency: spec.dependency,
                timeout: spec.timeout,
                hidden_rationale: spec.hidden,
                run: plugin::RunKind::Python {
                    checks_file: file.clone(),
                    check: spec.name,
                },
            })
            .collect();
        return Ok((slug.to_owned(), specs, check_dir.to_path_buf()));
    }

    // A native registered check set, matched by id (the checks-file
    // stem).
    let id = std::path::Path::new(checks_file).file_stem().map_or_else(
        || slug.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let Some(plugin) = registry::builtin_plugins()
        .into_iter()
        .find(|plugin| plugin.id() == id)
    else {
        anyhow::bail!(
            "no registered check set matches `{id}` and `{checks_file}` does not exist (a Python checks file must end in .py)"
        );
    };
    Ok((plugin.id().to_owned(), plugin.checks(), plugin.check_dir()))
}
