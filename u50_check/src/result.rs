//! The check50-parity results model: `CheckResult`, `Cause`, and the
//! embedded error info, JSON-shaped exactly like check50's documented
//! output (`docs/source/json_specification.rst` in the upstream repo).
//!
//! The `Cause` variants mirror check50's per-origin payload shapes:
//! failures carry `{rationale, help}`, skips carry `{rationale}` only,
//! and internal errors carry `{rationale, error}`.

use serde::{Deserialize, Serialize};

/// One check's outcome, in declaration order.
///
/// `passed` is `None` when the check was **skipped** — either because its
/// dependency did not pass, or because the check itself hit an internal
/// error (in which case `cause.error` carries the details).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    /// Unique name of the check (the registered function/spec name).
    pub name: String,
    /// Human-readable description (check50: the check's docstring).
    pub description: String,
    /// `true` passed, `false` failed, `None` skipped.
    pub passed: Option<bool>,
    /// Log lines accrued during the check (student-visible).
    pub log: Vec<String>,
    /// Why the check did not pass; `null` iff passed.
    pub cause: Option<Cause>,
    /// Arbitrary data emitted by the check via [`crate::api::CheckContext::data`].
    pub data: serde_json::Map<String, serde_json::Value>,
    /// Name of the check this one depends on, if any.
    pub dependency: Option<String>,
}

/// Why a check did not pass. The JSON shape varies by origin, exactly as
/// check50's per-exception payloads do.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Cause {
    /// A `Failure`/`Timeout` assertion: `{rationale, help}` (help always
    /// present, `null` when there is no hint).
    Failure {
        rationale: String,
        help: Option<String>,
    },
    /// An output mismatch: `{rationale, help, expected, actual}`
    /// (check50: `Mismatch` adds the expected/actual pair).
    Mismatch {
        rationale: String,
        help: Option<String>,
        expected: String,
        actual: String,
    },
    /// A skipped check (dependency failed or the check errored):
    /// `{rationale}` only.
    Skipped { rationale: String },
    /// An internal error raised while the check ran:
    /// `{rationale, error}`.
    Error { rationale: String, error: ErrorInfo },
}

impl Cause {
    /// A plain failure rationale (no help hint).
    #[must_use]
    pub fn failure(rationale: impl Into<String>) -> Self {
        Self::Failure {
            rationale: rationale.into(),
            help: None,
        }
    }

    /// The skip rationale (check50: "can't check until a frown turns
    /// upside down").
    #[must_use]
    pub fn skipped() -> Self {
        Self::Skipped {
            rationale: "can't check until a frown turns upside down".to_owned(),
        }
    }
}

/// An unexpected (non-`Failure`) error raised while a check ran.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorInfo {
    /// The error type name (check50: the exception class name).
    #[serde(rename = "type")]
    pub kind: String,
    /// The error value (message).
    pub value: String,
    /// The traceback, line by line (check50 parity; the port emits a
    /// single synthesized line since Rust has no Python-style tracebacks).
    pub traceback: Vec<String>,
    /// Extra payload data attached to the error, if any.
    #[serde(default)]
    pub data: serde_json::Map<String, serde_json::Value>,
}

/// The top-level JSON shape for a failed engine run (invalid slug, I/O
/// error, ...): the `results` key is replaced by an `error` key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EngineError {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
    pub traceback: Vec<String>,
    pub actions: ErrorActions,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub data: serde_json::Map<String, serde_json::Value>,
}

/// The `actions` block of an engine error (what the UI should show).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorActions {
    pub show_traceback: bool,
    pub message: String,
}

/// Builds an [`EngineError`] from an anyhow error chain.
#[must_use]
pub fn engine_error(error: &anyhow::Error) -> EngineError {
    EngineError {
        kind: "Error".to_owned(),
        value: format!("{error:#}"),
        traceback: vec![format!("Error: {}", error.root_cause())],
        actions: ErrorActions {
            show_traceback: false,
            message: format!("{error:#}"),
        },
        data: serde_json::Map::new(),
    }
}
