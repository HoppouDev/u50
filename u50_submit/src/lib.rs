#![warn(clippy::pedantic)]

/// Parameters for a `u50 submit` invocation.
///
/// The flag fields mirror the CLI's `--yes`/`--ssh`/`--dry-run`/`--logout`
/// options one-to-one, so the bool count is inherent to the interface.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct Request {
    /// Problem slug (server-side contract; kept identical to submit50).
    pub slug: String,
    /// Skip the confirmation prompt.
    pub yes: bool,
    /// Force SSH transport.
    pub ssh: bool,
    /// Show what would be submitted without pushing.
    pub dry_run: bool,
    /// Log out of the current session.
    pub logout: bool,
}

/// Submits the work described by `req` to GitHub via git.
///
/// # Errors
/// Returns an error until the submission engine is implemented.
pub fn run(req: &Request) -> anyhow::Result<()> {
    tracing::debug!(?req, "u50_submit::run");
    anyhow::bail!("`u50 submit` is not implemented yet")
}
