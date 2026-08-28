# u50_style — style50 reimplementation

Rust rewrite of [style50](https://github.com/cs50/style50): checks code style and reports style violations.

**Read the original first**: consult the [style50 Python repo](https://github.com/cs50/style50) before assuming CLI flags, output format, or style-rule semantics. Record findings here (below) so later agents don't have to re-read the Python source.

## Status

Stub. `src/lib.rs` contains only `#![warn(clippy::pedantic)]`; concrete behavior is future work.

## Behavior notes

Findings recorded from the official docs: https://cs50.readthedocs.io/style50/

### Usage

- Usage: `style50 <file>` — checks file(s) against CS50's style guide.
- Languages: C; also C++/Java via clang-format >= 14.
- Under the hood it uses astyle / clang-format.

### Output

- Shows a diff-style rendering: green highlights = characters to add, red = characters to delete.

### Output modes (`-o`/`--output`)

- `character` (default) — char-by-char comparison against expected styling.
- `split` — input and expected output side by side.
- `unified` — line-by-line diff, like `git diff` with `+`/`-` lines.

- No json mode exists in the original docs. (A machine-readable `json` mode is a **u50 addition** for tooling — see `u50_cli/AGENTS.md`.)

Workspace-wide conventions (Rust edition 2024, workspace dependencies, clippy pedantic, CI gates) live in the root [AGENTS.md](../AGENTS.md) and are not repeated here.
