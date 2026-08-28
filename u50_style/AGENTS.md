# u50_style — style50 reimplementation

Rust rewrite of [style50](https://github.com/cs50/style50): checks code style and reports style violations.

**Read the original first**: consult the [style50 Python repo](https://github.com/cs50/style50) before assuming CLI flags, output format, or style-rule semantics. Record findings here (below) so later agents don't have to re-read the Python source.

## Status

Engine implemented, clang-format-backed (matching the original, which shells out to `clang-format -style=<config>`): `run(&Request) -> Result<Report>` uses `ClangFormat` (impl of the `Formatter` trait, injectable via `run_with` for tests without clang-format), renders per-file diffs (`character` with inline char emphasis / `split` / `unified` / `json`) and prints them; the CLI maps `Report::clean()` to exit 0/1.

API: `Language` (`detect_language` by extension: c/h -> C, cpp/hpp/cc/cxx -> Cpp, java -> Java), `Formatter` trait, `ClangFormat`, `Request { files, output, color }`, `FileResult { path, clean, rendered }`, `Report { results }` with `clean()`. Pure renderers (`render_character`/`render_split`/`render_unified`/`json_document`) take `(source, formatted, ...)` and are unit-testable without clang-format.

## Behavior notes

Findings recorded from the official docs: https://cs50.readthedocs.io/style50/

### Usage

- Usage: `style50 <file>` — checks file(s) against CS50's style guide.
- Languages: C; also C++/Java via clang-format >= 14.
- Under the hood it shells out to `clang-format` (>= 14 required for C++/Java).

### Output

- Shows a diff-style rendering: green highlights = characters to add, red = characters to delete.

### Output modes (`-o`/`--output`)

- `character` (default) — char-by-char comparison against expected styling.
- `split` — input and expected output side by side.
- `unified` — line-by-line diff, like `git diff` with `+`/`-` lines.

- `-o json` **exists in the original** (per its README; the readthedocs page was stale). u50's JSON schema:

  ```json
  { "clean": bool, "files": [ { "path": String, "clean": bool, "patch": string-or-null } ] }
  ```

  `patch` is the unified diff, null when the file is clean.

### Embedded style config (verbatim, from the original's source)

The original shells out to `clang-format -style=<config>`; u50 embeds the same config in `CS50_CLANG_FORMAT_CONFIG` and runs `clang-format --assume-filename=<foo.c|foo.cpp|foo.java> -style=<config>`:

```
{ AllowShortFunctionsOnASingleLine: Empty, BraceWrapping: { AfterCaseLabel: true, AfterControlStatement: true, AfterFunction: true, AfterStruct: true, BeforeElse: true, BeforeWhile: true }, BreakBeforeBraces: Custom, ColumnLimit: 100, IndentCaseLabels: true, IndentWidth: 4, SpaceAfterCStyleCast: true, TabWidth: 4 }
```

clang-format >= 14 is required; when the binary is missing the engine errors with "clang-format is required (>= 14) to check C/C++/Java style".

### Exit codes

- 0 — all files clean.
- 1 — style violations found (CLI maps `!Report::clean()` to 1).
- 3 — infrastructure error (clang-format missing/failing).

### Not yet implemented (present in the original; future work)

- `-i` (in-place fix), `--ignore`, `--clang-format-style` (custom style override).
- `score` and `html` output modes (style50 v2 features).

Workspace-wide conventions (Rust edition 2024, workspace dependencies, clippy pedantic, CI gates) live in the root [AGENTS.md](../AGENTS.md) and are not repeated here.
