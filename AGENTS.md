# AGENTS.md for u50

## Purpose

u50 is a single Rust CLI binary intended to replace the separate Harvard CS50 command-line tools — check50, style50, submit50 — with one unified program. It is a Cargo workspace: `u50_cli` is the binary entry point (clap-based), and each original tool gets its own library crate that the CLI dispatches to as a subcommand:

- **u50_check** — reimplementation of [check50](https://github.com/cs50/check50): runs checks against student code.
- **u50_style** — reimplementation of [style50](https://github.com/cs50/style50): style checking.
- **u50_submit** — reimplementation of [submit50](https://github.com/cs50/submit50): submits work; uses `git2` (with SSH support) for git operations.

## Original tools (reference)

u50 reimplements these Python tools from CS50. They are the source of truth for CLI flags, output formats, check semantics, and submission behavior — consult them before making assumptions about how a feature should work:

- [cs50/check50](https://github.com/cs50/check50) — checks the correctness of programs against per-problem check specifications. → `u50_check`
- [cs50/style50](https://github.com/cs50/style50) — checks code style and reports style violations. → `u50_style`
- [cs50/submit50](https://github.com/cs50/submit50) — submits problem sets to GitHub via git. → `u50_submit`

These are Python codebases; u50 is a Rust rewrite intended to produce one binary with equivalent behavior. Where the originals' behavior is unclear, read their source in the linked repos rather than guessing.

## Documentation layout

Each program crate has its own `AGENTS.md` in its directory — `u50_cli/AGENTS.md`, `u50_check/AGENTS.md`, `u50_style/AGENTS.md`, `u50_submit/AGENTS.md` — holding program-specific details (CLI flags, output formats, findings from the original repo). `u50_cli/AGENTS.md` defines the CLI design (subcommands, flags, exit codes). Read ONLY this root AGENTS.md plus the AGENTS.md of the crate you are modifying; skip the others to save context tokens.

## Codebase layout

```
Cargo.toml            # workspace root
u50_cli/              # binary crate: [[bin]] name = "u50"
  src/main.rs
u50_check/            # library crate (check50 equivalent)
  src/lib.rs
u50_style/            # library crate (style50 equivalent)
  src/lib.rs
u50_submit/           # library crate (submit50 equivalent)
  src/lib.rs
.github/workflows/rust.yml
```

- **Workspace root `Cargo.toml`**: `resolver = "3"`, all dependency versions in `[workspace.dependencies]`, release profile tuned for size (`opt-level = "z"`, `strip`, `lto`, `codegen-units = 1`). Members: `u50_check`, `u50_cli`, `u50_style`, `u50_submit`.
- **u50_cli/**: the binary crate, `[[bin]] name = "u50"`. Implemented CLI: clap subcommands `check`/`style`/`submit` with global flags, dispatching to the library crates; its `AGENTS.md` defines the CLI design.
- **u50_check/, u50_style/, u50_submit/**: library crates, one per original cs50 tool. `u50_style`'s engine is implemented (the style50 3.0.0 language set — C/C++/Java via clang-format, Python via autopep8, JavaScript via js-beautify, HTML via djhtml, CSS via css-beautify, SQL via sqlformat; unified/character/split/json output; in-place fix (`--fix`) with dry-run; directory arguments; `--list` status table and `--setup`/lazy auto-provisioning of backends into a uv-managed venv in `~/.cache/u50/style50` (in-process via uv library crates — no system python3/pip/uv binary needed; cache-only tool resolution, `PATH` never consulted); `u50_check` and `u50_submit` remain stubs: `run()` bails with a "not implemented yet" error; request types (Request/enums) are defined and dispatched from the CLI. Each has its own `AGENTS.md` with program-specific details.

## Conventions

- Rust **edition 2024** in every crate.
- Dependency versions come from `[workspace.dependencies]` — crates must reference them as `x = { workspace = true }`, never with inline version numbers.
- Async runtime: **tokio** (full features). Errors: **anyhow**. Logging: **tracing** / **tracing-subscriber**.
- Every crate starts with `#![warn(clippy::pedantic)]`.

## CI / quality gates

GitHub Actions workflow `.github/workflows/rust.yml` (name: `Rust`) runs on push/PR to `main`:

- `cargo build`
- `cargo test`
- `cargo fmt --all -- --check` — code must be rustfmt-clean.
- `cargo clippy --verbose -- -Dwarnings` — **zero warnings allowed**; with pedantic enabled, this is strict. Linter configuration lives in the root `clippy.toml`.

## Common commands

```sh
cargo build
cargo test
cargo run
cargo fmt --all
cargo clippy --workspace -- -Dwarnings
cargo run --bin u50 -- --help   # clap help output
```
