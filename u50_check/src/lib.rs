#![warn(clippy::pedantic)]

/// Execution mode for `u50 check`, replacing the original tool's four
/// mutually-exclusive boolean mode flags.
#[derive(Debug, Clone)]
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

/// Parameters for a `u50 check` invocation.
#[derive(Debug, Clone)]
pub struct Request {
    /// Problem slug (server-side contract; kept identical to check50).
    pub slug: String,
    /// Execution mode.
    pub mode: Mode,
    /// Named checks to run (plus dependencies); empty means all.
    pub targets: Vec<String>,
    /// Output formats to render (ansi/html/json).
    pub outputs: Vec<String>,
}

/// Runs checks for `req` against student code.
///
/// # Errors
/// Returns an error until the check engine is implemented.
pub fn run(req: &Request) -> anyhow::Result<()> {
    tracing::debug!(?req, "u50_check::run");
    anyhow::bail!("`u50 check` is not implemented yet")
}
