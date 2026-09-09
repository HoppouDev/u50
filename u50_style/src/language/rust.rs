//! Rust: the rustfmt backend, and the Rust-toolchain tool resolution
//! (`rustfmt` is not pip-installable, so unlike the other backends it is
//! resolved from the user's Rust installation and never auto-provisioned).

use std::path::PathBuf;

use super::LanguagePlugin;
use super::count_c_family_comments;
use crate::format::run_tool;
use crate::format::tool::{is_executable_file, tool_file_name};

/// rustfmt invocation args pinning the edition (see [`format`]).
const EDITION_ARGS_2024: [&str; 4] = ["--edition", "2024", "--emit", "stdout"];
const EDITION_ARGS_2021: [&str; 4] = ["--edition", "2021", "--emit", "stdout"];

/// Formats Rust source with `rustfmt` (stdin in, stdout out; the edition
/// is pinned to 2024 so parsing does not depend on the invocation
/// directory).
///
/// Unlike the pip backends, rustfmt's own `rustfmt.toml` discovery
/// applies, as with `cargo fmt` — deliberate: Rust projects pin their
/// style via rustfmt.toml, and ignoring it would flag project-conformant
/// code as dirty.
///
/// Requires rustfmt >= 1.85 (the first release accepting
/// `--edition 2024`); older releases reject the flag before touching the
/// source, and the call falls back to edition 2021 so the file still
/// formats.
///
/// # Errors
/// Returns an error when `rustfmt` is missing or fails (e.g. a parse
/// error).
fn format_rustfmt(source: &str) -> anyhow::Result<String> {
    match run_tool("rustfmt", &EDITION_ARGS_2024, source) {
        // rustfmt < 1.85: clap rejects the `2024` value before any
        // parsing happens; 2021 still formats the file.
        Err(error) if error.to_string().contains("invalid value '2024'") => {
            run_tool("rustfmt", &EDITION_ARGS_2021, source)
        }
        other => other,
    }
}

/// Resolves a formatter tool from the user's Rust toolchain —
/// deterministic install locations, never `PATH` — so a Rust developer
/// does not need a duplicate cache copy of `rustfmt`. Only tools without
/// a pip backend (currently only `rustfmt`) resolve here; anything
/// pip-provisionable must come from the cache, and unknown tools never
/// resolve here. Returns `None` when no usable installation is found.
///
/// Resolution order: the rustup toolchain bin dirs under
/// `$RUSTUP_HOME/toolchains` (default `~/.rustup`), `stable*` first and
/// then the lexicographically greatest (likely newest) — direct binaries
/// that ignore `rust-toolchain.toml` — then the `$CARGO_HOME/bin` rustup
/// proxy (default `~/.cargo/bin`), which honors the user's default
/// toolchain but is sensitive to `rust-toolchain.toml` /
/// `RUSTUP_TOOLCHAIN` discovered from the invocation directory (it may
/// even trigger a toolchain download). The proxy is the fallback, not
/// the default, for exactly that reason.
///
/// Accepted stances (mirroring the pre-existing cache resolution): the
/// check-then-exec window between [`is_executable_file`] and spawn is
/// not closed (an attacker who can write the toolchain dirs already
/// controls the toolchain); symlinked toolchain directories are followed
/// (`RUSTUP_HOME` write access is already game over); on Windows a
/// rooted-but-prefix-less override (e.g. `\\evil`) passes the absolute
/// filter and resolves against the working directory's drive — it
/// requires control of the user's environment and is accepted.
fn toolchain_tool(tool: &str) -> Option<PathBuf> {
    let name = tool_file_name(tool);
    if let Some(rustup) = rustup_home() {
        let mut dirs: Vec<PathBuf> = match std::fs::read_dir(rustup.join("toolchains")) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect(),
            // No toolchains dir is not fatal: the cargo-bin proxy below
            // may still resolve.
            Err(_) => Vec::new(),
        };
        sort_toolchain_dirs(&mut dirs);
        if let Some(found) = dirs
            .iter()
            .map(|dir| dir.join("bin").join(&name))
            .find(|candidate| is_executable_file(candidate))
        {
            return Some(found);
        }
    }
    let cargo_bin = cargo_home()?.join("bin").join(&name);
    is_executable_file(&cargo_bin).then_some(cargo_bin)
}

/// Sorts toolchain directories into resolution order: `stable*` first,
/// then the lexicographically greatest (newest) within each group. Pure
/// so it is unit-testable.
pub(crate) fn sort_toolchain_dirs(dirs: &mut [PathBuf]) {
    dirs.sort_by_key(|path| {
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        (
            std::cmp::Reverse(name.starts_with("stable")),
            std::cmp::Reverse(name),
        )
    });
}

/// `$CARGO_HOME` (default `~/.cargo` on unix, `%USERPROFILE%\.cargo` on
/// Windows); absolute only, so a relative override cannot root the
/// search at the working directory.
fn cargo_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Some(home);
    }
    dirs::home_dir()
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".cargo"))
}

/// `$RUSTUP_HOME` (default `~/.rustup` on unix, `%USERPROFILE%\.rustup`
/// on Windows); absolute only, as with [`cargo_home`]. The default home
/// comes from the [`dirs`] crate.
fn rustup_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Some(home);
    }
    dirs::home_dir()
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".rustup"))
}

/// The Rust language plugin (a u50 addition — style50 has no Rust
/// support): the rustfmt backend and the Rust-toolchain tool
/// resolution, both owned entirely by this module.
pub(crate) struct RustPlugin;
pub(crate) static PLUGIN: RustPlugin = RustPlugin;

impl LanguagePlugin for RustPlugin {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn display_name(&self) -> &'static str {
        "Rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn required_tool(&self) -> &'static str {
        "rustfmt"
    }

    fn count_comments(&self, code: &str) -> Option<u32> {
        // Rust shares the C-family counter (line/block/doc comments;
        // nested block comments count once, raw strings are not
        // stripped — documented quirks).
        Some(count_c_family_comments(code))
    }

    // rustfmt is not pip-installable: it resolves from the Rust
    // toolchain (see `resolve_tool`) and is never auto-provisioned.
    fn missing_tool_message(&self) -> String {
        "`rustfmt` is required to check Rust style (install it with: `rustup component add rustfmt`)".to_owned()
    }

    fn format(&self, source: &str) -> anyhow::Result<String> {
        format_rustfmt(source)
    }

    fn resolve_tool(&self, tool: &str) -> Option<std::path::PathBuf> {
        if tool != self.required_tool() {
            return None;
        }
        toolchain_tool(tool)
    }

    fn tool_search_scope(&self) -> &'static str {
        "the u50 style cache and the Rust toolchain"
    }
}
