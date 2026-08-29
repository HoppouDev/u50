# u50_cli — unified binary (u50)

> **Status:** the CLI layer is implemented; `u50_style` has a working engine (clang-format-backed; style violations exit 1, and exit 3 now means any per-file error — unreadable file, unsupported type, or formatter failure — with the remaining files still checked and their output streamed). `u50_check` and `u50_submit` remain stubs returning "not implemented yet" errors (exit 3).

The `u50` binary replaces the separate check50/style50/submit50 command-line tools with a single program: clap 4 derive defines the interface, and each subcommand dispatches to the corresponding library crate (`u50_check`, `u50_style`, `u50_submit`).

## CLI design (modernized — deliberate divergence from originals)

### Design principles

The original tools are clunky by modern CLI standards: inconsistent flags, tool-specific semantics for the same flag letter, no unified machine-readable output, and mutually-exclusive boolean mode flags (`--local`/`--offline`/`--dev`). u50 normalizes on clap 4 conventions:

- **Consistent global flags** across all subcommands (logging, color, version).
- **Enums instead of boolean-flag clusters** — one `--mode` value, not four exclusive booleans.
- **Unified version/help/exit codes** for the whole binary.

## Layout

```rust
// u50_cli/src/main.rs
#[derive(Parser)]
#[command(name = "u50", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

enum Command {
    Check(CheckArgs),
    Style(StyleArgs),
    Submit(SubmitArgs),
}
```

## Subcommands

### `u50 check <SLUG> [flags]` → `u50_check`

- `--mode <online|local|offline|dev>` — default `online`; enum replaces the original's four mutually-exclusive boolean flags (`--local`, `--offline`, `--dev`).
- `--target <NAME>...` — repeatable; run only named checks plus their dependencies.
- `-o/--output <ansi|html|json>` — repeatable, default `ansi`.
- `--output-file <PATH>`.

### `u50 style <FILES>... [flags]` → `u50_style`

- FILE operands may be **directories**: each is expanded recursively like style50 3.0.0 (`os.walk`, symlinked dirs not followed) — only supported extensions are collected, hidden dirs included, results deduplicated and sorted; explicit file args keep per-file error semantics.
- `-o/--output <character|split|unified|json>` — default `character`; `json` also exists in the original (its README documents it; readthedocs was stale). Exit codes: 0 clean, 1 violations, 3 any per-file error (unreadable file, unsupported type, formatter failure) — other files are still checked and their output streamed.
- `--fix` — rewrite files in place with style50 formatting (mirrors the original's `-i`/`--in-place`; conflicts with `-o/--output`). Exit codes: 0 all files fixed or already clean, 3 any per-file error (incl. write failures). No exit 1: a plain fix either fixes or errors.
- `--dry-run` — report what would change without writing (requires `--fix`); prints `would fix: <path>` / `already clean: <path>` status lines, with no diff. Exit codes: 1 when at least one file would change (check-style convention), 0 when everything is already clean, 3 on errors.
- `--list` — print the language/extensions/binary/status table (see `u50_style/AGENTS.md`, 'Tool management'); ignores FILE operands and the other style flags; exits 0. Conflicts with `--setup`, `--fix`, `--dry-run`, `-o/--output`.
- `--setup` — install missing formatter backends into `~/.cache/u50/style50` (parallel pip downloads, one spinner per package), then print the `--list` table. Exit 0 on success, 3 when pip is unavailable or any package failed. Conflicts with `--list`, `--fix`, `--dry-run`, `-o/--output`.

### `u50 submit <SLUG> [flags]` → `u50_submit`

- `--yes` — skip confirmation (the original has no non-interactive mode).
- `--ssh` — force SSH transport.
- `--dry-run` — show what would be submitted, push nothing.
- `--logout`.

## Global flags (root level, consistent across subcommands)

- `-v/--verbose` — repeatable, maps to tracing levels.
- `--log-level <trace|debug|info|warn|error>` — overrides `-v`.
- `-q/--quiet`.
- `--color <auto|always|never>` — default `auto`; honors `NO_COLOR`.
- `-V/--version` on root (the original submit50 used `-V`; the others `--version`).

## Compatibility notes

- **Slug formats are kept identical to the originals** — they are server-side contracts with cs50's GitHub orgs; do not change them.
- JSON output should follow check50's documented results schema (`name`/`description`/`passed`/`log`/`cause`/`data`/`dependency`) — see `u50_check/AGENTS.md`.
- Mode names `ansi`/`html`/`character`/`split`/`unified` are kept for familiarity.
- Everything else (flag naming, mode enums, non-interactive flags) is intentionally modernized — **not bug-compatible** with the originals.

## Exit codes (unified)

| Code | Meaning |
| ---- | ------- |
| 0 | success (all checks passed / style clean / submitted / style fixed or already clean) |
| 1 | checks failed or style violations found; `style --fix --dry-run` with at least one would-fix |
| 2 | usage error |
| 3 | any per-file error (unreadable file, unsupported type, formatter/write failure) |
