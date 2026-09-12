//! The process bridge: discovery and per-check invocation of Python
//! checks on the provisioned `CPython` (Phase 0-2 of
//! `docs/U50_CHECK_PYTHON_PLAN.md`). Discovery runs the shipped package
//! once and emits the check registry as JSON; invocation spawns one
//! subprocess per check in its own process group (the runner's deadline
//! machinery kills the interpreter and every student process it spawned).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use serde::Deserialize;

use super::venv;
use crate::api::Failure;
use crate::result::ErrorInfo;

/// A discovered check (from the registry emitted by the shipped
/// package's discovery pass).
#[derive(Debug)]
pub struct PyCheckSpec {
    pub name: String,
    pub description: String,
    pub dependency: Option<String>,
    pub timeout: Option<Duration>,
    pub hidden: Option<String>,
}

/// Why a Python check did not pass: either the check raised a
/// `check50.Failure` (the normal failure path) or something went wrong
/// inside the check process itself (rendered as `Cause::Error`, like
/// the panic path).
pub(crate) enum PyFailure {
    Check(Failure),
    Internal(ErrorInfo),
}

#[derive(Debug, Deserialize)]
struct Discovery {
    checks: Vec<DiscoveredCheck>,
}

#[derive(Debug, Deserialize)]
struct DiscoveredCheck {
    name: String,
    description: String,
    dependency: Option<String>,
    timeout: Option<f64>,
    hidden: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    log: Vec<String>,
    #[serde(default)]
    data: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    cause: Option<EnvelopeCause>,
    #[serde(default)]
    error: Option<ErrorInfo>,
}

#[derive(Debug, Deserialize)]
struct EnvelopeCause {
    rationale: String,
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    expected: Option<String>,
    #[serde(default)]
    actual: Option<String>,
}

/// The per-check state file: each check's pickled return value lives in
/// its run dir; dependents inherit it through the run-dir copy (each
/// check overwrites it with its own state, so a dependent always reads
/// its direct dependency's value).
pub(crate) const STATE_FILE: &str = ".check50-state";

/// Discovers the check registry of a Python checks file (declaration
/// order, descriptions, dependencies, timeouts) by importing it on the
/// provisioned interpreter.
///
/// # Errors
/// Returns an error when provisioning, spawning, or parsing fails.
pub fn discover(checks_file: &Path) -> anyhow::Result<Vec<PyCheckSpec>> {
    let python = venv::interpreter()?;
    let out = Command::new(python)
        .args([
            "-m",
            "check50.bridge",
            "discover",
            &checks_file.display().to_string(),
        ])
        .env(
            "CHECK50_CHECK_DIR",
            checks_file
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        )
        .output()
        .context("run the check discovery pass")?;
    anyhow::ensure!(
        out.status.success(),
        "check discovery failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let discovery: Discovery =
        serde_json::from_str(stdout.trim()).context("parse the discovery envelope")?;
    Ok(discovery
        .checks
        .into_iter()
        .map(|check| PyCheckSpec {
            timeout: check.timeout.map(Duration::from_secs_f64),
            name: check.name,
            description: check.description,
            dependency: check.dependency,
            hidden: check.hidden,
        })
        .collect())
}

/// Invokes one check: spawns the interpreter in the check's run dir
/// (own process group), feeds it the dependency's pickled state, and
/// translates the result envelope into the engine's failure model. Log
/// lines and payload data flow through the check context like they do
/// for native checks.
///
/// # Errors
/// Returns [`PyFailure::Check`] when the check raised a
/// `check50.Failure`, and [`PyFailure::Internal`] when the check
/// process itself failed (bad envelope, import error, ...). A check
/// that overruns `timeout` is killed (process group) and reports the
/// timeout failure.
pub(crate) fn invoke(
    ctx: &mut crate::api::CheckContext,
    checks_file: &Path,
    check: &str,
    timeout: Duration,
) -> Result<(), PyFailure> {
    let python = venv::interpreter().map_err(|error| internal_error(&error))?;
    let state_file = ctx.run_dir.join(STATE_FILE);
    let mut command = Command::new(python);
    command
        .args([
            "-m",
            "check50.bridge",
            "invoke",
            &checks_file.display().to_string(),
            check,
            &state_file.display().to_string(),
        ])
        .current_dir(&ctx.run_dir)
        .env(
            "CHECK50_CHECK_DIR",
            checks_file
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| {
        internal_error(&anyhow::anyhow!("could not run the check interpreter: {e}"))
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let buffer: Arc<Mutex<String>> = Arc::default();
    if let Some(stdout) = stdout {
        let buffer = Arc::clone(&buffer);
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut { stdout }, &mut text);
            *buffer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = text;
        });
    }
    // Drain stderr (import errors and tracebacks surface in the error
    // path when no envelope arrives).
    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            let mut sink = String::new();
            let _ = std::io::Read::read_to_string(&mut { stderr }, &mut sink);
        });
    }
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().is_ok_and(|status| status.is_some()) {
            break;
        }
        if Instant::now() >= deadline {
            crate::api::kill_child(&mut child);
            let _ = child.wait();
            return Err(PyFailure::Check(Failure::new(format!(
                "check timed out after {} seconds",
                timeout.as_secs()
            ))));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let envelope = buffer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let envelope: Envelope = serde_json::from_str(envelope.trim())
        .map_err(|e| internal_error(&anyhow::anyhow!("bad check envelope: {e}")))?;
    for line in envelope.log {
        ctx.log(line);
    }
    for (key, value) in envelope.data {
        ctx.data(key, value);
    }
    match (envelope.ok, envelope.cause, envelope.error) {
        (true, _, _) => Ok(()),
        (false, Some(cause), _) => {
            let failure = Failure {
                rationale: cause.rationale,
                help: cause.help,
                expected: cause.expected,
                actual: cause.actual,
            };
            Err(PyFailure::Check(failure))
        }
        (false, None, Some(error)) => Err(PyFailure::Internal(ErrorInfo {
            kind: error.kind,
            value: error.value,
            traceback: error.traceback,
            data: error.data,
        })),
        (false, None, None) => Err(internal_error(&anyhow::anyhow!(
            "the check process reported failure without a cause"
        ))),
    }
}

/// An internal (non-`Failure`) failure from the check process.
fn internal_error(error: &anyhow::Error) -> PyFailure {
    PyFailure::Internal(ErrorInfo {
        kind: "Error".to_owned(),
        value: format!("{error:#}"),
        traceback: vec![format!("Error: {}", error.root_cause())],
        data: serde_json::Map::new(),
    })
}

// Silence the unused-path warning on platforms without process groups:
// the kill path still runs there, just without group semantics.
#[allow(unused)]
fn _path_typecheck(path: &Path) -> PathBuf {
    path.to_path_buf()
}
