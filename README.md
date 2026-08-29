# u50

[![CI](https://github.com/HoppouDev/u50/actions/workflows/rust.yml/badge.svg)](https://github.com/HoppouDev/u50/actions/workflows/rust.yml)

**u50** is a single Rust CLI binary that replaces Harvard CS50's separate
course tooling — [check50](https://github.com/cs50/check50),
[style50](https://github.com/cs50/style50), and
[submit50](https://github.com/cs50/submit50) — with one unified program.
The original Python tools remain the behavioral source of truth; u50
reimplements them as a Cargo workspace with one subcommand per tool.

## Status

| Subcommand | Crate | State |
| --- | --- | --- |
| `u50 style` | [`u50_style`](u50_style/) | **Fully implemented** — all 8 languages of style50 3.0.0, verified byte-identical against the original |
| `u50 check` | [`u50_check`](u50_check/) | Stub — request types defined, `run()` not implemented yet |
| `u50 submit` | [`u50_submit`](u50_submit/) | Stub — request types defined, `run()` not implemented yet |

The binary entry point lives in [`u50_cli`](u50_cli/).

## Quick start

```sh
cargo build

# One-time (optional): pre-download all formatter backends into the cache
cargo run --bin u50 -- style --setup

# Check style (auto-provisions any missing backend on first use)
u50 style src/
u50 style -o unified hello.c
u50 style -o json src/

# Fix in place (dry run first)
u50 style --fix --dry-run src/
u50 style --fix src/

# Show what is installed and from where
u50 style --list
```

## Headline feature: self-provisioning backends

u50 never needs a system `python3`, `pip`, or `uv`. All six formatter
backends (clang-format, autopep8, js-beautify, djhtml, css-beautify,
sqlformat) are downloaded on demand — via **uv library crates, in-process** —
into a managed venv under `~/.cache/u50/style50`. Tool resolution is
**cache-only**: the system `PATH` is never consulted, so a same-named binary
on your `PATH` can never be picked up. `--setup` pre-downloads everything
up front (CI, offline runs), but is not required — the first `u50 style`
invocation on a bare machine provisions exactly what it needs.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Clean (no violations, fix succeeded) |
| 1 | Violations found / dry run reported would-fix |
| 2 | Usage error (clap) |
| 3 | Per-file or infrastructure error (unreadable file, unsupported extension, missing/failing formatter, provisioning failure) |

## Development

```sh
cargo build
cargo test
U50_STYLE_GOLDEN=1 cargo test -p u50_style --test golden   # golden fixtures vs. style50 3.0.0

cargo clippy --workspace --all-targets -- -Dwarnings       # zero warnings (pedantic)
cargo fmt --all -- --check
```

CI (GitHub Actions, workflow name `Rust`) runs build, tests, fmt, clippy,
and the golden suite on every push/PR to `main`.

Optional git-hook integration via [lefthook](https://lefthook.dev): install
it (`brew install lefthook` or `go install github.com/evilmartians/lefthook@latest`),
then run `lefthook install` to enable the fmt + clippy pre-commit gates.

## Documentation

- [`AGENTS.md`](AGENTS.md) — repository conventions for humans and agents
  (start here)
- Per-crate docs: [`u50_cli/AGENTS.md`](u50_cli/AGENTS.md),
  [`u50_style/AGENTS.md`](u50_style/AGENTS.md),
  [`u50_check/AGENTS.md`](u50_check/AGENTS.md),
  [`u50_submit/AGENTS.md`](u50_submit/AGENTS.md)
- [`HARNESS_COMPLIANCE_PLAN.md`](HARNESS_COMPLIANCE_PLAN.md) — repo-meta
  improvement plan

## License

[GPLv3](LICENSE.md) — GNU General Public License, version 3.
