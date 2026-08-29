<div align="center">

<img src="assets/logo.png" alt="u50 logo" width="128">

# u50

*One binary for CS50 — check, style, and submit.*

[![CI](https://github.com/HoppouDev/u50/actions/workflows/rust.yml/badge.svg)](https://github.com/HoppouDev/u50/actions/workflows/rust.yml)
[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue.svg)](LICENSE.md)

[Features](#features) • [Quick start](#quick-start) • [Usage](#usage) • [Exit codes](#exit-codes) • [Development](#development)

</div>

u50 unifies Harvard CS50's three command-line tools — [check50](https://github.com/cs50/check50), [style50](https://github.com/cs50/style50), and [submit50](https://github.com/cs50/submit50) — into a single Rust binary. The `style` engine is fully implemented and verified byte-identical against style50 3.0.0; `check` and `submit` are on the roadmap.

## Features

- **Self-provisioning** — missing formatter backends are downloaded and cached on first use. No system `python3`, `pip`, or `uv` required.
- **8 languages** — C, C++, Java, Python, JavaScript, HTML, CSS, SQL: the full style50 3.0.0 language set.
- **Cache-only resolution** — tools are resolved strictly from u50's own cache; nothing on your `PATH` can shadow or hijack them.
- **In-place fix** — `--fix` rewrites files with style50 formatting; `--dry-run` previews what would change.
- **Four output modes** — character (default), split, unified, and JSON for tooling.
- **Reproducible** — backend versions are pinned, and CI verifies output byte-identical to the original tool.

## Quick start

> [!NOTE]
> There is nothing to install besides Rust. The first `u50 style` run provisions
> exactly the backends it needs into `~/.cache/u50/style50` — a managed venv
> built in-process via [uv's library crates](https://github.com/astral-sh/uv).

```sh
git clone https://github.com/HoppouDev/u50 && cd u50
cargo build
```

## Usage

```sh
# Check style (auto-provisions any missing backend on first use)
u50 style src/
u50 style -o unified hello.c
u50 style -o json src/ > report.json

# Fix in place (preview first)
u50 style --fix --dry-run src/
u50 style --fix src/

# Pre-download all six backends up front (CI, offline use)
u50 style --setup

# Show what's installed, per language
u50 style --list
```

Example output (`-o unified`):

```diff
- int main(){printf("hello\n");return 0;}
+ int main(void)
+ {
+     printf("hello\n");
+     return 0;
+ }
```

> [!TIP]
> Any language's formatter can be swapped per invocation via `U50_STYLE_<LANG>` —
> for example: `U50_STYLE_PYTHON="ruff format -" u50 style foo.py`

## Status

| Subcommand | Crate | State |
| --- | --- | --- |
| `u50 style` | [`u50_style`](u50_style/) | **Fully implemented** — all 8 languages of style50 3.0.0, verified byte-identical against the original |
| `u50 check` | [`u50_check`](u50_check/) | Stub — request types defined, engine on the roadmap |
| `u50 submit` | [`u50_submit`](u50_submit/) | Stub — request types defined, engine on the roadmap |

The binary entry point lives in [`u50_cli`](u50_cli/).

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Clean (no violations, fix succeeded) |
| 1 | Violations found / dry run reported would-fix |
| 2 | Usage error |
| 3 | Per-file or infrastructure error (unreadable file, unsupported extension, missing or failing formatter, provisioning failure) |

## Development

```sh
cargo build
cargo test
U50_STYLE_GOLDEN=1 cargo test -p u50_style --test golden   # golden fixtures vs. style50 3.0.0

cargo clippy --workspace --all-targets -- -Dwarnings       # zero warnings (pedantic)
cargo fmt --all -- --check
```

CI (GitHub Actions, workflow name `Rust`) runs build, tests, format check, clippy, the golden suite, and a harness-score ratchet on every push/PR to `main`.

## Documentation

- [`AGENTS.md`](AGENTS.md) — repository conventions for humans and agents (start here)
- Per-crate docs: [`u50_cli/AGENTS.md`](u50_cli/AGENTS.md) · [`u50_style/AGENTS.md`](u50_style/AGENTS.md) · [`u50_check/AGENTS.md`](u50_check/AGENTS.md) · [`u50_submit/AGENTS.md`](u50_submit/AGENTS.md)
