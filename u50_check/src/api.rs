//! The check-authoring API: the Rust analog of check50's `_api.py`.
//!
//! Checks are plain `fn(&mut CheckContext) -> Result<(), Failure>`
//! functions. [`CheckContext::run`] returns a chainable [`Run`] builder
//! mirroring check50's `check50.run(...).stdin(...).stdout(...).exit(...)`
//! chain, with the same prompt-absorption, EOF, regex/exact matching,
//! reject, and exit-code semantics. Every escape the builder's spawned
//! programs produce comes from the programs themselves; the builder never
//! colors anything.

use std::fmt::Write as _;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;

/// The default timeout for [`Run::stdin`] and [`Run::stdout`] (check50:
/// 3 seconds).
pub const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(3);
/// The default timeout for [`Run::exit`] (check50: 5 seconds).
pub const DEFAULT_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
/// The default timeout for [`Run::reject`] (check50: 1 second).
pub const DEFAULT_REJECT_TIMEOUT: Duration = Duration::from_secs(1);

/// Sentinel for "send end-of-file to the program's stdin" (check50:
/// `check50.EOF`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eof;

/// Input for [`Run::stdin`]: a line, or the EOF sentinel.
#[derive(Debug, Clone)]
pub enum StdinInput {
    /// A line to send (newline appended, like pexpect's `sendline`).
    Line(String),
    /// Send EOF (close the program's stdin).
    Eof,
}

impl From<&str> for StdinInput {
    fn from(line: &str) -> Self {
        Self::Line(line.to_owned())
    }
}

impl From<String> for StdinInput {
    fn from(line: String) -> Self {
        Self::Line(line)
    }
}

impl From<Eof> for StdinInput {
    fn from(_: Eof) -> Self {
        Self::Eof
    }
}

/// The EOF sentinel (check50: `check50.EOF`).
pub const EOF: Eof = Eof;

/// Expected output for [`Run::stdout`]: a pattern, or the EOF sentinel.
#[derive(Debug, Clone)]
pub enum MatchInput {
    /// A pattern, matched as a regex or exactly (per the `exact` flag).
    Pattern(String),
    /// Expect the program's stdout to end (EOF).
    Eof,
}

impl From<&str> for MatchInput {
    fn from(pattern: &str) -> Self {
        Self::Pattern(pattern.to_owned())
    }
}

impl From<Eof> for MatchInput {
    fn from(_: Eof) -> Self {
        Self::Eof
    }
}

/// Signifies a check failure (check50: `check50.Failure`).
#[derive(Debug, Clone)]
pub struct Failure {
    /// Why the check failed (student-facing).
    pub rationale: String,
    /// Optional student-facing hint.
    pub help: Option<String>,
    /// Expected output (for [`Mismatch`]).
    pub expected: Option<String>,
    /// Actual output (for [`Mismatch`]).
    pub actual: Option<String>,
}

impl Failure {
    /// A plain failure rationale.
    #[must_use]
    pub fn new(rationale: impl Into<String>) -> Self {
        Self {
            rationale: rationale.into(),
            help: None,
            expected: None,
            actual: None,
        }
    }

    /// A failure with a student-facing help hint.
    #[must_use]
    pub fn with_help(rationale: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            rationale: rationale.into(),
            help: Some(help.into()),
            expected: None,
            actual: None,
        }
    }

    /// A failure with an expected/actual mismatch payload and check50's
    /// `"expected X, not Y"` rationale.
    #[must_use]
    pub fn mismatch(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        let expected = expected.into();
        let actual = actual.into();
        Self {
            rationale: format!("expected {expected:?}, not {actual:?}"),
            help: None,
            expected: Some(expected),
            actual: Some(actual),
        }
    }

    /// A mismatch with a help hint.
    #[must_use]
    pub fn mismatch_with_help(
        expected: impl Into<String>,
        actual: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        let mut failure = Self::mismatch(expected, actual);
        failure.help = Some(help.into());
        failure
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.rationale)
    }
}

impl std::error::Error for Failure {}

/// Signifies a check failure due to an item missing from a collection
/// (check50: `check50.Missing`) — typically a substring expected in
/// stdout. The rationale reads `Did not find "item" in "collection"`
/// (check50's exact wording).
pub struct Missing;

impl Missing {
    /// The missing-item failure.
    #[must_use]
    pub fn failure(item: impl Into<String>, collection: impl Into<String>) -> Failure {
        let (item, collection) = (item.into(), collection.into());
        Failure {
            rationale: format!("Did not find \"{item}\" in \"{collection}\""),
            help: None,
            expected: Some(item),
            actual: Some(collection),
        }
    }
}

/// The context a check runs in: its own working directory, the check
/// directory files can be included from, and the per-check log/data
/// buffers. Passed as `&mut CheckContext` to every check function.
pub struct CheckContext {
    /// The directory the check runs in (inherits the dependency's
    /// filesystem state).
    pub run_dir: PathBuf,
    /// The directory containing the check's own files (source of
    /// [`CheckContext::include`]).
    check_dir: PathBuf,
    log: Arc<Mutex<Vec<String>>>,
    data: Arc<Mutex<serde_json::Map<String, serde_json::Value>>>,
}

impl CheckContext {
    pub(crate) fn new(
        run_dir: PathBuf,
        check_dir: PathBuf,
        log: Arc<Mutex<Vec<String>>>,
        data: Arc<Mutex<serde_json::Map<String, serde_json::Value>>>,
    ) -> Self {
        Self {
            run_dir,
            check_dir,
            log,
            data,
        }
    }

    /// Adds a line to the student-visible check log (newlines escaped,
    /// check50 parity).
    pub fn log(&mut self, line: impl AsRef<str>) {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(line.as_ref().replace('\n', "\\n"));
    }

    /// Adds key/value pairs to the check's result payload (check50:
    /// `check50.data`).
    pub fn data(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.into(), value);
    }

    /// Asserts that every given path exists, or fails with
    /// `"<path> not found"` (check50 parity).
    ///
    /// # Errors
    /// Returns a [`Failure`] when any path does not exist.
    pub fn exists(
        &mut self,
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Result<(), Failure> {
        for path in paths {
            let path = path.as_ref();
            self.log(format!("checking that {} exists...", path.display()));
            if !self.resolve(path).exists() {
                return Err(Failure::new(format!("{} not found", path.display())));
            }
        }
        Ok(())
    }

    /// Resolves a check-relative path against the check's `run_dir`
    /// (checks run concurrently in threads of one process, so — unlike
    /// check50's per-process `os.chdir` — a relative path can never be
    /// resolved against the process's current directory).
    fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.run_dir.join(path)
        }
    }

    /// Copies files/directories from the check's own directory into the
    /// run directory (check50: `check50.include`).
    ///
    /// # Errors
    /// Returns an error when the copy fails.
    pub fn include(
        &mut self,
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> anyhow::Result<()> {
        for path in paths {
            let src = self.check_dir.join(path.as_ref());
            let dst = self.run_dir.join(path.as_ref());
            copy_tree(&src, &dst)
                .with_context(|| format!("could not include {}", path.as_ref().display()))?;
        }
        Ok(())
    }

    /// Hashes `file` with SHA-256 (check50: `check50.hash`).
    ///
    /// # Errors
    /// Returns a [`Failure`] when the file does not exist or cannot be
    /// read.
    pub fn hash(&mut self, file: impl AsRef<Path>) -> Result<String, Failure> {
        let path = file.as_ref();
        self.exists([path])?;
        self.log(format!("hashing {}...", path.display()));
        let bytes = std::fs::read(self.resolve(path))
            .map_err(|_| Failure::new(format!("{} not found", path.display())))?;
        Ok(sha256_hex(&bytes))
    }

    /// Runs a command in the check's run directory, returning the
    /// chainable [`Run`] builder (check50: `check50.run`).
    ///
    /// # Errors
    /// Returns a [`Failure`] when the command cannot be spawned.
    pub fn run(&mut self, command: impl AsRef<str>) -> Result<Run<'_>, Failure> {
        Run::spawn(self, command.as_ref())
    }
}

/// Computes the SHA-256 digest of `bytes` as lowercase hex.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// The result of spawning a command inside a check, with the same
/// chainable assertion methods as check50's `check50.run(...)` builder.
/// Dropping the builder kills the spawned program (check50's
/// good-practice note: never leave spawned programs running past the
/// check).
pub struct Run<'ctx> {
    ctx: &'ctx mut CheckContext,
    child: Child,
    stdin: Option<ChildStdin>,
    out_buf: Arc<Mutex<String>>,
    /// Match cursor: everything before this offset was already consumed
    /// by earlier `.stdout()` assertions.
    cursor: usize,
    exited: bool,
}

impl Run<'_> {
    fn spawn<'s>(ctx: &'s mut CheckContext, command: &str) -> Result<Run<'s>, Failure> {
        ctx.log(format!("running {command}..."));
        // bash -c, exactly like check50 (quoting + shell semantics).
        let mut child = bash_command()
            .arg("-c")
            .arg(command)
            .current_dir(&ctx.run_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Failure::new(format!("could not run {command}: {e}")))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        // A reader thread accumulates the program's output so every
        // assertion can poll the buffer without blocking the check.
        let out_buf = Arc::new(Mutex::new(String::new()));
        if let Some(mut stdout) = stdout {
            let buf = Arc::clone(&out_buf);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = stdout.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push_str(&String::from_utf8_lossy(&chunk[..n]));
                }
            });
        }
        Ok(Run {
            ctx,
            child,
            stdin,
            out_buf,
            cursor: 0,
            exited: false,
        })
    }

    /// Sends `input` to the program's stdin. With `prompt = true`, first
    /// absorbs output until something arrives (else
    /// `"expected prompt for input, found none"`), then keeps absorbing
    /// for up to `timeout` (check50's consume loop).
    /// # Errors
    /// Returns a [`Failure`] when no prompt is found (with `prompt`),
    /// stdin is closed, or the input cannot be sent.
    pub fn stdin(
        &mut self,
        input: impl Into<StdinInput>,
        prompt: bool,
        timeout: Duration,
    ) -> Result<&mut Self, Failure> {
        let input = input.into();
        match &input {
            StdinInput::Line(line) => self.ctx.log(format!("sending input {line}...")),
            StdinInput::Eof => self.ctx.log("sending EOF..."),
        }
        if prompt {
            let deadline = Instant::now() + timeout;
            let start = self.buffer_len();
            loop {
                if self.buffer_len() > start {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(Failure::new("expected prompt for input, found none"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let absorb = Instant::now() + timeout;
            while Instant::now() < absorb {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(Failure::new("stdin is closed"));
        };
        match input {
            StdinInput::Line(line) => {
                stdin
                    .write_all(line.as_bytes())
                    .and_then(|()| stdin.write_all(b"\n"))
                    .map_err(|_| Failure::new("could not send input to the program"))?;
                stdin
                    .flush()
                    .map_err(|_| Failure::new("could not send input to the program"))?;
            }
            StdinInput::Eof => drop(self.stdin.take()), // closing the pipe sends EOF
        }
        Ok(self)
    }

    /// Waits until the unconsumed output matches `pattern` (regex, or
    /// exact when `exact` is set; the EOF sentinel expects stdout to
    /// end), consuming through the match. On timeout,
    /// `Missing`-style/`Mismatch` failures carry the unconsumed output
    /// (check50 parity).
    /// # Errors
    /// Returns a [`Failure`] on timeout, invalid UTF-8, or invalid regex.
    /// Returns a `Mismatch`-style failure when the output does not match.
    pub fn stdout(
        &mut self,
        pattern: impl Into<MatchInput>,
        exact: bool,
        timeout: Duration,
    ) -> Result<&mut Self, Failure> {
        let pattern = pattern.into();
        let eof = matches!(pattern, MatchInput::Eof);
        let pattern = match pattern {
            MatchInput::Pattern(p) => p,
            MatchInput::Eof => String::new(),
        };
        if eof {
            self.ctx.log("checking for EOF...");
        } else {
            self.ctx
                .log(format!("checking for output \"{pattern}\"..."));
        }
        let matcher = if exact {
            Matcher::Exact(pattern.clone())
        } else {
            Matcher::Regex(regex::Regex::new(&pattern).map_err(|_| {
                Failure::new("could not verify output (pattern is not a valid regex)")
            })?)
        };
        let deadline = Instant::now() + timeout;
        loop {
            let buffer = self.buffer();
            let unconsumed = &buffer[self.cursor.min(buffer.len())..];
            if !eof && let Some(end) = matcher.find(unconsumed) {
                self.cursor += end;
                return Ok(self);
            }
            if self.exited() {
                // EOF with the expectation still unmet (check50 parity:
                // report a mismatch against whatever was left).
                if eof && unconsumed.is_empty() {
                    return Ok(self); // clean EOF
                }
                return Err(Failure::mismatch(
                    if eof {
                        "EOF".to_owned()
                    } else {
                        pattern.clone()
                    },
                    unconsumed.to_owned(),
                ));
            }
            if Instant::now() >= deadline {
                return Err(Failure::new(format!(
                    "timed out while waiting for output (waited {}s)",
                    timeout.as_secs()
                )));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Waits for the program to exit within `timeout` and returns all
    /// unconsumed output (CRLF normalized, leading newlines stripped;
    /// check50's `stdout(None)` form).
    /// # Errors
    /// Returns a [`Failure`] on timeout or SIGSEGV.
    pub fn stdout_text(&mut self, timeout: Duration) -> Result<String, Failure> {
        self.wait_for_exit(Instant::now() + timeout)?;
        let text = self.consume().replace("\r\n", "\n");
        Ok(text.trim_start_matches('\n').to_owned())
    }

    /// Asserts the program is still alive after `timeout` (i.e. it
    /// rejected the input instead of consuming it), check50 parity.
    /// # Errors
    /// Returns a [`Failure`] when the program exits before the timeout
    /// (i.e. it consumed the input instead of rejecting it).
    pub fn reject(&mut self, timeout: Duration) -> Result<&mut Self, Failure> {
        self.ctx.log("checking that input was rejected...");
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.try_exit() {
                return Err(Failure::new(
                    "expected program to reject input, but it did not",
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(self)
    }

    /// Waits for the program to exit within `timeout` and, when `code`
    /// is given, asserts the exit status; with `None`, returns the
    /// observed exit code (check50: `exit(None)`).
    ///
    /// # Errors
    /// Returns a [`Failure`] on timeout ("timed out while waiting for
    /// program to exit"), on SIGSEGV (check50 parity), or on an exit-code
    /// mismatch (`"expected exit code X, not Y"`).
    pub fn exit(&mut self, code: Option<i32>, timeout: Duration) -> Result<Option<i32>, Failure> {
        let Some(actual) = self.wait_for_exit(Instant::now() + timeout)? else {
            return Err(Failure::new("timed out while waiting for program to exit"));
        };
        let Some(expected) = code else {
            return Ok(Some(actual));
        };
        self.ctx.log(format!(
            "checking that program exited with status {expected}..."
        ));
        if actual != expected {
            return Err(Failure::new(format!(
                "expected exit code {expected}, not {actual}"
            )));
        }
        Ok(Some(actual))
    }

    /// Kills the program.
    pub fn kill(&mut self) -> &mut Self {
        let _ = self.child.kill();
        self.exited = true;
        self
    }

    fn buffer(&self) -> String {
        self.out_buf
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn buffer_len(&self) -> usize {
        self.out_buf
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn consume(&self) -> String {
        let buffer = self.buffer();
        buffer[self.cursor.min(buffer.len())..].to_owned()
    }

    fn exited(&mut self) -> bool {
        if self.exited {
            return true;
        }
        self.try_exit()
    }

    fn try_exit(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Waits until the program exits or `deadline` passes; on exit,
    /// records the status (SIGSEGV parity) and closes stdin. Returns the
    /// exit code (`None` when the program was killed by a signal).
    fn wait_for_exit(&mut self, deadline: Instant) -> Result<Option<i32>, Failure> {
        loop {
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|_| Failure::new("could not wait for the program"))?
            {
                self.exited = true;
                drop(self.stdin.take());
                if status.signal_of_segfault() {
                    return Err(Failure::new(
                        "failed to execute program due to segmentation fault",
                    ));
                }
                return Ok(status.code());
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                return Err(Failure::new("timed out while waiting for program to exit"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for Run<'_> {
    fn drop(&mut self) {
        // check50's good-practice note: never leave spawned programs
        // running past the check.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The observed exit status of a spawned program.
#[derive(Debug, Clone, Copy)]
pub struct ExitStatus {
    pub code: Option<i32>,
    pub segfault: bool,
}

/// Regex-or-exact matcher over the unconsumed output.
enum Matcher {
    Regex(regex::Regex),
    Exact(String),
}

impl Matcher {
    fn find(&self, text: &str) -> Option<usize> {
        match self {
            Self::Regex(re) => re.find(text).map(|m| m.end()),
            Self::Exact(pattern) => text.find(pattern).map(|start| start + pattern.len()),
        }
    }
}

/// Builds the exact-number regex (check50: `check50.regex.decimal`).
///
/// check50's Python original uses look-around (a negative lookbehind
/// for non-negative numbers, a negative lookahead always) to reject a
/// match embedded in a larger number (`"420"` must not match `42`).
/// The `regex` crate has no look-around support, so the port gets the
/// same rejection with consuming character-class boundaries instead:
/// the match includes one boundary character (or start/end of string)
/// on each side that is *not* part of a larger number, which is
/// functionally equivalent for [`Run::stdout`]'s use (only whether a
/// match exists, and where it ends, matters — not the captured text).
#[must_use]
pub fn decimal_regex(number: f64) -> String {
    let literal = regex::escape(&format!("{number}"));
    if number.is_sign_negative() {
        // The minus sign is already part of `literal`; only the
        // trailing boundary (not embedded in a larger/decimal number)
        // needs asserting.
        format!(r"{literal}(?:[^0-9.]|$)")
    } else {
        format!(r"(?:^|[^0-9.\-]){literal}(?:[^0-9.]|$)")
    }
}

/// Copies `src` to `dst`, recursively when `src` is a directory
/// (check50: `_copy`).
///
/// # Errors
/// Returns an error when any copy step fails.
pub fn copy_tree(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)
            .with_context(|| format!("could not create {}", dst.display()))?;
        for entry in
            std::fs::read_dir(src).with_context(|| format!("could not read {}", src.display()))?
        {
            let entry = entry?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)
            .with_context(|| format!("could not copy {} to {}", src.display(), dst.display()))?;
        Ok(())
    }
}

/// Resolves a real POSIX shell to run check commands through.
///
/// On Unix, `bash` on `PATH` is a real shell. On Windows, the first
/// `bash` found on `PATH` (including inside a GitHub Actions runner)
/// is frequently `%SystemRoot%\\System32\\bash.exe` — the WSL launcher
/// stub, which fails immediately when no WSL distribution is installed
/// rather than running the command. Git for Windows ships a real
/// `bash.exe` at a fixed location; prefer it explicitly so check
/// commands (which use POSIX shell syntax check50 checks rely on) run
/// the same way on every platform.
fn bash_command() -> Command {
    #[cfg(windows)]
    {
        for candidate in [
            "C:\\Program Files\\Git\\bin\\bash.exe",
            "C:\\Program Files\\Git\\usr\\bin\\bash.exe",
            "C:\\Program Files (x86)\\Git\\bin\\bash.exe",
        ] {
            if Path::new(candidate).is_file() {
                return Command::new(candidate);
            }
        }
    }
    Command::new("bash")
}

trait ExitStatusExt {
    fn signal_of_segfault(&self) -> bool;
}

#[cfg(unix)]
impl ExitStatusExt for std::process::ExitStatus {
    fn signal_of_segfault(&self) -> bool {
        use std::os::unix::process::ExitStatusExt as _;
        self.signal() == Some(11) // SIGSEGV
    }
}

#[cfg(windows)]
impl ExitStatusExt for std::process::ExitStatus {
    fn signal_of_segfault(&self) -> bool {
        // STATUS_ACCESS_VIOLATION
        self.code() == Some(0xC000_0005_u32 as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn context(dir: &std::path::Path) -> CheckContext {
        CheckContext::new(
            dir.to_path_buf(),
            dir.to_path_buf(),
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(serde_json::Map::new())),
        )
    }

    #[test]
    fn exists_passes_for_a_present_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("hello.txt"), "hi").expect("write");
        let mut ctx = context(dir.path());
        assert!(ctx.exists(["hello.txt"]).is_ok());
    }

    #[test]
    fn exists_fails_for_a_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = context(dir.path());
        let err = ctx.exists(["missing.txt"]).unwrap_err();
        assert!(err.rationale.contains("not found"));
    }

    #[test]
    fn run_stdout_and_exit_pass_for_matching_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = context(dir.path());
        let mut run = ctx.run("echo hello").expect("spawn");
        run.stdout("hello", false, Duration::from_secs(3))
            .expect("stdout matches");
        run.exit(Some(0), Duration::from_secs(3))
            .expect("exit code matches");
    }

    #[test]
    fn run_stdout_mismatch_carries_expected_and_actual() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = context(dir.path());
        let mut run = ctx.run("echo hello").expect("spawn");
        let Err(err) = run.stdout("goodbye", true, Duration::from_millis(500)) else {
            panic!("expected a stdout mismatch");
        };
        assert_eq!(err.expected.as_deref(), Some("goodbye"));
        assert_eq!(err.actual.as_deref(), Some("hello\n"));
    }

    #[test]
    fn run_exit_code_mismatch_reports_expected_and_actual_codes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = context(dir.path());
        let mut run = ctx.run("exit 7").expect("spawn");
        let err = run.exit(Some(0), Duration::from_secs(3)).unwrap_err();
        assert!(err.rationale.contains("expected exit code 0, not 7"));
    }

    #[test]
    fn run_stdin_prompt_then_echo_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = context(dir.path());
        let mut run = ctx
            .run("echo -n '> '; read line; echo \"$line\"")
            .expect("spawn");
        run.stdin("meow", true, Duration::from_secs(3))
            .expect("prompt absorbed and line sent");
        run.stdout("meow", true, Duration::from_secs(3))
            .expect("echoed back");
        run.exit(Some(0), Duration::from_secs(3)).expect("exits 0");
    }

    #[test]
    fn run_reject_fails_when_program_consumes_input() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = context(dir.path());
        let mut run = ctx.run("read line; exit 0").expect("spawn");
        run.stdin("meow", false, Duration::from_millis(200))
            .expect("stdin sent");
        let Err(err) = run.reject(Duration::from_secs(1)) else {
            panic!("expected reject to fail (program consumed input)");
        };
        assert!(err.rationale.contains("reject"));
    }

    #[test]
    fn decimal_regex_matches_the_exact_number_only() {
        let pattern = decimal_regex(42.0);
        let re = regex::Regex::new(&pattern).expect("valid regex");
        assert!(re.is_match("42"));
        assert!(!re.is_match("420"));
        assert!(!re.is_match("142"));
    }
}
