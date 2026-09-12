//! Shared process plumbing: spawn with process-group semantics and
//! kill the whole group on timeout (POSIX) or the direct child
//! (Windows).

use std::process::{Child, Command, Stdio};

/// Builds a `bash -c` command, resolving a real POSIX shell (mirrors
/// the Rust `bash_command` resolution: Git for Windows' bash preferred
/// over the WSL launcher stub).
#[must_use]
pub fn bash_command() -> Command {
    let mut command_builder = Command::new(resolve_bash_program());
    command_builder.stderr(Stdio::null());
    command_builder
}

fn resolve_bash_program() -> String {
    if cfg!(windows) {
        for candidate in [
            "C:\\Program Files\\Git\\bin\\bash.exe",
            "C:\\Program Files\\Git\\usr\\bin\\bash.exe",
            "C:\\Program Files (x86)\\Git\\bin\\bash.exe",
        ] {
            if std::path::Path::new(candidate).is_file() {
                return candidate.to_owned();
            }
        }
        tracing::warn!(
            "no Git for Windows bash found; falling back to `bash` on PATH, which may be the WSL launcher stub"
        );
    }
    "bash".to_owned()
}

/// Configures a command to run in its own process group (Unix), so
/// killing it takes down any grandchildren.
pub fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

/// Kills a spawned process, including its process group on Unix.
pub fn kill_child(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = libc::pid_t::try_from(child.id()) {
            // SAFETY: killpg only sends a signal to a process group; any
            // failure (e.g. the group is already gone) is ignored.
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

/// Kills and reaps every tracked child of a check (shared by the
/// scheduler's timeout path).
pub fn kill_tracked_children(
    children: &std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<std::sync::Mutex<Child>>>>>,
) {
    let drained = children
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain(..)
        .collect::<Vec<_>>();
    for child in drained {
        if let Ok(mut child) = child.lock() {
            kill_child(&mut child);
            let _ = child.wait();
        }
    }
}
