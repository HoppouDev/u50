//! Formatter tool plumbing: cache paths, cache-only tool resolution,
//! process spawning with timeouts, and lazy backend provisioning. No
//! language-specific logic lives here — exception: `locate_tool`
//! delegates bare-name fallback resolution for tools their plugins
//! cannot pip-provision via [`LanguagePlugin::resolve_tool`].

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

use super::ToolOrigin;

/// The u50 style cache root: the absolute `$XDG_CACHE_HOME` override
/// when set (all platforms), else the platform cache base
/// of `dirs::cache_dir()`), then `u50/style50`.
///
/// # Errors
/// Returns an error when no cache base is determinable: no absolute
/// `$XDG_CACHE_HOME`, and no `$HOME` on unix or `%LOCALAPPDATA%` /
/// `%USERPROFILE%` on Windows. u50 never falls back to a relative
/// `.cache`, which would silently scatter the cache across working
/// directories.
pub(crate) fn cache_dir() -> anyhow::Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(xdg.join("u50").join("style50"));
    }
    let base = dirs::cache_dir().ok_or_else(|| {
        anyhow::anyhow!(if cfg!(windows) {
            "cannot determine the u50 style cache directory: set \
             %LOCALAPPDATA% or %USERPROFILE% (or an absolute \
             $XDG_CACHE_HOME)"
        } else {
            "cannot determine the u50 style cache directory: set \
             $HOME (or an absolute $XDG_CACHE_HOME)"
        })
    })?;
    Ok(base.join("u50").join("style50"))
}
/// The `bin` directory of a uv-managed venv: `Scripts` on Windows
/// (where console scripts are installed as `.exe` shims), `bin`
/// elsewhere (POSIX shebang scripts).
#[must_use]
pub(crate) fn venv_bin_dir(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    }
}

/// The file name the console script for `tool` is installed under in
/// the venv bin dir: `tool.exe` on Windows, `tool` elsewhere.
#[must_use]
pub(crate) fn tool_file_name(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_owned()
    }
}

/// The directory holding binaries installed by `u50 --setup` (the
/// uv-managed venv `<cache>/venv` puts console scripts in `bin/` —
/// `Scripts\` with `.exe` shims on Windows; see [`venv_bin_dir`]).
///
/// # Errors
/// Propagates [`cache_dir`] failures.
pub(crate) fn cache_bin_dir() -> anyhow::Result<PathBuf> {
    Ok(venv_bin_dir(&cache_dir()?.join("venv")))
}

/// Whether `path` is an existing regular file usable as a formatter
/// tool: on unix it must carry an execute bit; Windows has no exec-bit
/// model, so any existing regular file qualifies.
#[cfg(unix)]
pub(crate) fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
pub(crate) fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Whether `tool` names an explicit path rather than a bare tool name:
/// true when it contains either path separator, or its first path
/// component is not a plain name — a Windows drive/UNC prefix (which
/// `Path::join` would let replace the cache dir entirely), `..`, or
/// `.`. Bare tool names stay cache-only on all platforms, so a hostile
/// or unrelated same-named binary on `PATH` can never be picked up.
fn is_explicit_path(tool: &str) -> bool {
    if tool.contains('/') || tool.contains('\\') {
        return true;
    }
    match Path::new(tool).components().next() {
        Some(Component::Normal(_)) | None => false,
        Some(
            Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir,
        ) => true,
    }
}

/// Resolves `tool` to its location: an explicit path (see
/// [`is_explicit_path`]) is used as-is ([`ToolOrigin::Path`]); a bare
/// tool name is looked up in the u50 style cache bin dir (the
/// `u50 --setup` / lazy auto-provision install location, with the
/// platform console-script file name, see [`tool_file_name`]), then —
/// for tools their plugins cannot pip-provision — through
/// [`LanguagePlugin::resolve_tool`] ([`ToolOrigin::Toolchain`]). The
/// system `PATH` is never consulted. Returns `None` when the tool is
/// found nowhere (the caller may then auto-provision it; see
/// [`Cs50Formatter::format`](crate::format::Cs50Formatter::format)) or
/// when the cache directory cannot be determined ([`cache_dir`]).
#[must_use]
pub fn locate_tool(tool: &str) -> Option<(PathBuf, ToolOrigin)> {
    if is_explicit_path(tool) {
        return Some((PathBuf::from(tool), ToolOrigin::Path));
    }
    // The cache lookup must not short-circuit the toolchain fallback:
    // when the cache dir cannot even be determined (no `$HOME` etc.), a
    // toolchain-installed rustfmt must still resolve.
    if let Some(cached) = cache_bin_dir()
        .ok()
        .map(|dir| dir.join(tool_file_name(tool)))
        .filter(|path| is_executable_file(path))
    {
        return Some((cached, ToolOrigin::Cache));
    }
    // Tools their plugins cannot pip-provision resolve through the
    // plugin's own resolver — deterministic install locations, still
    // never `PATH`.
    if let Some(path) = crate::registry::languages()
        .iter()
        .find(|plugin| plugin.required_tool() == tool)
        .and_then(|plugin| plugin.resolve_tool(tool))
    {
        return Some((path, ToolOrigin::Toolchain));
    }
    None
}

/// Resolves `tool` cache-only and spawns it with `args`, feeding `source`
/// on stdin (written from a separate thread so a child that fills its
/// stdout pipe cannot deadlock against us still writing its stdin), and
/// waits for it to exit.
///
/// Cache-only spawn guard for BUILT-IN tools: never let the OS resolve a
/// bare formatter tool through `PATH`. `locate_tool` runs exactly once
/// here and the resolved path is handed straight to the spawn (no second
/// lookup). No env fixup is needed: the venv console scripts installed by
/// `--setup` carry absolute shebangs and are self-contained.
///
/// # Errors
/// Returns an error when the tool is not in the cache (with the standard
/// missing-tool message) and any error while attaching stdin or waiting
/// on the child.
fn spawn_tool(tool: &str, args: &[&str], source: &str) -> anyhow::Result<std::process::Output> {
    // The owning plugin provides the missing-tool message and the search
    // scope; tools unknown to the registry keep the generic defaults.
    let owner = crate::registry::languages()
        .iter()
        .copied()
        .find(|&plugin| plugin.required_tool() == tool);
    let missing = owner.map_or_else(
        || format!("`{tool}` is required"),
        super::super::language::LanguagePlugin::missing_tool_message,
    );
    let scope = owner
        .map_or("the u50 style cache", |plugin| plugin.tool_search_scope())
        .to_owned();
    let resolved = locate_tool(tool)
        .map(|(path, _)| path)
        .ok_or_else(|| anyhow::anyhow!("{missing} (not found in {scope})"))?;
    let mut command = Command::new(&resolved);
    // The venv console scripts are self-contained, but the interpreter
    // they launch still honors inherited Python env vars: a user's
    // PYTHONHOME breaks site initialization and a stray PYTHONPATH can
    // shadow the pinned transitive deps. Strip them (see AGENTS.md,
    // "self-contained").
    // Strip every inherited `PYTHON*` variable (not just the four
    // classic ones): the venv scripts are self-contained, and any of
    // PYTHONHOME/PYTHONPATH/PYTHONUSERBASE/PYTHONSAFEPATH/... can alter
    // the interpreter's import resolution.
    for (var, _) in std::env::vars_os() {
        if var.to_string_lossy().starts_with("PYTHON") {
            command.env_remove(var);
        }
    }
    // Own process group: on timeout a whole backend process tree dies
    // instead of surviving grandchildren holding the pipes open.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("{missing}: {e}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("could not attach stdin to `{tool}`"))?;
    let source = source.to_owned();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(source.as_bytes());
    });
    // Drain both pipes on separate threads and wait under a deadline: a
    // hung backend must not hang the whole (rayon-parallel) run.
    // style50 waits forever — documented divergence.
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("could not attach stdout to `{tool}`"))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("could not attach stderr to `{tool}`"))?;
    let stdout_reader = std::thread::spawn(move || read_capped(stdout_pipe));
    let stderr_reader = std::thread::spawn(move || read_capped(stderr_pipe));
    let started = std::time::Instant::now();
    let deadline = tool_timeout();
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if started.elapsed() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                // The dead child closed its pipe ends; give the readers
                // a short grace period to observe EOF. A straggler
                // grandchild still holding a pipe open (see the process
                // group above) must not hang the run — the reader
                // thread is then abandoned (a bounded leak per
                // timeout).
                let _ = join_bounded(writer);
                let _ = join_bounded(stdout_reader);
                let _ = join_bounded(stderr_reader);
                anyhow::bail!("`{tool}` timed out after {}s", deadline.as_secs());
            }
            None => std::thread::sleep(std::time::Duration::from_millis(25)),
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader task for `{tool}` failed"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader task for `{tool}` failed"))?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

/// The per-tool wall-clock deadline (default: one minute, generous for
/// even the large golden fixtures). Override with
/// `U50_STYLE_TOOL_TIMEOUT_SECS`.
fn tool_timeout() -> std::time::Duration {
    const DEFAULT: std::time::Duration = std::time::Duration::from_mins(1);
    std::env::var_os("U50_STYLE_TOOL_TIMEOUT_SECS")
        .and_then(|v| v.into_string().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(DEFAULT, std::time::Duration::from_secs)
}

/// Upper bound on how much backend output is buffered: a backend that
/// echoes pathological input must not exhaust memory. Output past the
/// budget is still drained (so the child never blocks on a full pipe)
/// but dropped — a run that large fails anyway.
const MAX_PIPE_BYTES: usize = 64 * 1024 * 1024;

fn read_capped(mut pipe: impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => return buf,
            Ok(n) => {
                if buf.len() + n <= MAX_PIPE_BYTES {
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
    }
}

/// Joins a spawned thread with a bounded grace period; a thread still
/// running after the deadline is abandoned (leaked) rather than waited
/// on indefinitely.
fn join_bounded<T>(handle: std::thread::JoinHandle<T>) -> Option<T> {
    const GRACE: std::time::Duration = std::time::Duration::from_secs(2);
    let deadline = std::time::Instant::now() + GRACE;
    while !handle.is_finished() {
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    handle.join().ok()
}

pub(crate) fn run_tool(tool: &str, args: &[&str], source: &str) -> anyhow::Result<String> {
    let output = spawn_tool(tool, args, source)?;
    if !output.status.success() {
        return Err(tool_failure(tool, output.status, &output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Formats a failed-tool error: always carries the exit status (several
/// backends can exit nonzero with empty stderr — a bare message with a
/// trailing colon helps nobody), plus the stderr text when present.
fn tool_failure(tool: &str, status: std::process::ExitStatus, stderr: &[u8]) -> anyhow::Error {
    let code = status
        .code()
        .map_or_else(|| "signal".to_owned(), |code| code.to_string());
    let lossy = String::from_utf8_lossy(stderr);
    let detail = lossy.trim();
    if detail.is_empty() {
        anyhow::anyhow!("`{tool}` failed with exit code {code} (no error output)")
    } else {
        anyhow::anyhow!("`{tool}` failed with exit code {code}: {detail}")
    }
}

/// Runs `tool` with `args`, feeding `source` on stdin, tolerating the
/// "exit 1 means reformatted" convention some backends document (see the
/// calling plugin module for which backend uses this and why): exit 0 is
/// success, and exit 1 with non-empty stdout is also treated as success;
/// anything else is an error.
pub(crate) fn run_tool_lenient(tool: &str, args: &[&str], source: &str) -> anyhow::Result<String> {
    let output = spawn_tool(tool, args, source)?;
    let reformatted_on_exit_1 = output.status.code() == Some(1) && !output.stdout.is_empty();
    if !output.status.success() && !reformatted_on_exit_1 {
        return Err(tool_failure(tool, output.status, &output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Tools whose auto-provisioning was already attempted in this
/// process (see [`ensure_backends`]). Each missing tool of a run is
/// attempted once; later files needing the same tool skip straight to
/// the missing-tool error when the attempt failed. When an attempt
/// succeeded, [`locate_tool`] finds the tool and the dedupe never
/// matters.
static PROVISION_ATTEMPTED: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Attempts to lazily auto-provision the backends for `missing` — the
/// `(pip package, tool)` pairs collected after the walk — exactly once
/// per process: fetches every package **in parallel** via the same uv
/// library path `u50 --setup` uses (never through the formatter, so
/// there is no recursion) and lets the caller's subsequent spawn fail
/// naturally when provisioning did not help. Set
/// `U50_STYLE_NO_PROVISION` in the environment to disable (used by
/// hermetic tests).
pub(crate) fn ensure_backends(missing: &[(String, String)]) {
    if std::env::var_os("U50_STYLE_NO_PROVISION").is_some() {
        return;
    }
    let mut attempted = PROVISION_ATTEMPTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let pending: Vec<(String, String)> = missing
        .iter()
        .filter(|(_, tool)| attempted.insert(tool.clone()))
        .cloned()
        .collect();
    drop(attempted);
    if pending.is_empty() {
        return;
    }
    let tools: Vec<&str> = pending.iter().map(|(_, tool)| tool.as_str()).collect();
    tracing::info!(
        ?tools,
        "formatter backends missing from the cache; auto-provisioning"
    );
    if let Err(e) = crate::setup::install_backends(&pending, crate::setup::ProgressTarget::Stderr) {
        tracing::warn!(?tools, error = %e, "auto-provisioning failed");
    }
}

/// Attempts to lazily auto-provision the single backend `tool` (the
/// per-file fallback in
/// [`Cs50Formatter::format`](crate::format::Cs50Formatter::format)) by mapping it to its
/// pip package and delegating to [`ensure_backends`].
pub(crate) fn ensure_backend(tool: &str) {
    let Some(package) = crate::registry::languages()
        .iter()
        .find(|plugin| plugin.required_tool() == tool)
        .and_then(|plugin| plugin.pip_package())
    else {
        tracing::warn!(tool, "no known pip package provides this tool");
        return;
    };
    ensure_backends(&[(package.to_owned(), tool.to_owned())]);
}
