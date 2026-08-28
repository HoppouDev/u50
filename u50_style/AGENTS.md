# u50_style — style50 reimplementation

Rust rewrite of [style50](https://github.com/cs50/style50): checks code style and reports style violations.

**Read the original first**: consult the [style50 Python repo](https://github.com/cs50/style50) before assuming CLI flags, output format, or style-rule semantics. Record findings here (below) so later agents don't have to re-read the Python source.

## Status

Engine implemented, supporting the languages of the RELEASED style50 (2.10.4): `run(&Request) -> Result<Report>` uses `Cs50Formatter::from_env()` (impl of the `Formatter` trait, injectable via `run_with` for tests without the formatters installed), renders per-file diffs (`character` with inline char emphasis / `split` / `unified` / `json`) and prints them; the CLI maps `Report::clean()` to exit 0/1. Every language can be redirected to a custom formatter via `U50_STYLE_<LANG>` environment variables (see 'Formatter overrides', below).

API: `Language` (`detect_language` by extension for all supported languages; `Language::required_tool() -> Option<&'static str>` names the backing binary; `Language::env_var_key()` gives the override variable suffix), `Formatter` trait, `Cs50Formatter` (`Default` = built-in tools with no overrides; `with_overrides(HashMap<Language, Vec<String>>)`; `from_env()`), `Request { files, output, color }`, `FileResult { path, clean, rendered }`, `Report { results, errors }` with `clean()` and `has_errors()`. Pure renderers (`render_character`/`render_split`/`render_unified`/`json_document`) take `(source, formatted, ...)` and are unit-testable without the tools.

Per-file errors never abort the run: an unreadable file, unsupported extension, or formatter failure for one file records `(path, message)` in `Report.errors` and processing continues with the remaining files, so earlier results are preserved. `run()` prints rendered output for every processed file to stdout (stdout stays pure diff/JSON), then writes each error to stderr as `error: <path>: <message>`. Formatter-level failures (e.g. missing clang-format) are therefore per-file skips, not whole-run bails.

## Language support

Exactly the languages of the **released** style50 (2.10.4, per `style50 -E` → `[c, h, cpp, hpp, py, js, java]`). The style50 main branch adds CSS, SQL, and HTML, which u50 deliberately does **not** implement (removed to match the real released surface; recorded here in case they are added back later):

| Language | Extensions | Backend tool | Install hint |
|---|---|---|---|
| C | c, h | `clang-format` (>= 14) | distro package |
| C++ | cpp, hpp | `clang-format` (>= 14) | distro package |
| Java | java | `clang-format` (>= 14) | distro package |
| Python | py | `autopep8` | `pip install autopep8` |
| JavaScript | js | `js-beautify` | `pip install jsbeautifier` |

Per-tool options (mirroring the original's `languages.py` option values; flag names verified against the installed CLIs):

- C/C++/Java: `clang-format --assume-filename=<foo.c|foo.cpp|foo.java> -style=<CS50 config>` (config embedded in `CS50_CLANG_FORMAT_CONFIG`, below).
- Python: `autopep8 - --max-line-length=100 --ignore-local-config` (original: `autopep8.fix_code(..., options={'max_line_length': 100, 'ignore_local_config': True})`).
- JavaScript: `js-beautify --end-with-newline --operator-position preserve-newline -w 100 --brace-style collapse,preserve-inline --keep-array-indentation -` (original: `jsbeautifier.beautify(...)` with the same option values; the short `-w 100` form is required because the CLI declares the long `--wrap-line-length` as taking no argument, and the `-` stdin marker must come last).
The original calls the Python libraries (`autopep8`, `jsbeautifier`) directly; u50 shells out to the pip CLIs, which apply the same defaults. A missing binary produces a per-language error, e.g. "`autopep8` is required to check Python style (pip install autopep8)".

## Formatter overrides

Any language's formatter can be replaced per invocation via an environment variable — overriding only affects that language; all others keep their built-in tools.

| Variable | Language |
|---|---|
| `U50_STYLE_C` | C |
| `U50_STYLE_CPP` | C++ |
| `U50_STYLE_JAVA` | Java |
| `U50_STYLE_PYTHON` | Python |
| `U50_STYLE_JAVASCRIPT` | JavaScript |

Semantics:

- The variable value is the command line of the replacement formatter, **split on plain whitespace** (no quoting support — arguments cannot contain spaces). The file's source is **piped to the tool via stdin**; its stdout becomes the expected formatting.
- Empty or whitespace-only values are ignored; unknown `U50_STYLE_*` variables are ignored.
- Empty/whitespace-only source still short-circuits to "clean" before any override lookup, mirroring the built-in behavior.
- Exit handling for overrides is **strict**: exit 0 is the only success.
- Errors name the variable and the binary: spawn failure → "could not run `<binary>` (set via U50_STYLE_<LANG>): ..."; non-zero exit → "formatter `<command line>` failed: <stderr>".
- Overrides change what "clean" means for that language: a file is clean iff the override tool reproduces its bytes exactly.

Examples:

```sh
U50_STYLE_PYTHON="ruff format -" u50 style foo.py
U50_STYLE_JAVASCRIPT="biome format --stdin-file-path=stdin.js" u50 style foo.js
```

Programmatic use (no process env reads): `Cs50Formatter::with_overrides(HashMap<Language, Vec<String>>)` builds a formatter with explicit overrides; `Cs50Formatter::from_env()` reads the variables. The env parsing itself is a pure function over a lookup closure, so tests pass fake maps and never touch the real environment.

## Behavior notes

Findings recorded from the official docs: https://cs50.readthedocs.io/style50/

### Usage

- Usage: `style50 <file>` — checks file(s) against CS50's style guide.
- Languages: C, C++, Java, Python, JavaScript (the readthedocs page also lists HTML, CSS, SQL — main-branch only, absent from the released style50 2.10.4; u50 deliberately does not implement them, see 'Language support').
- Under the hood it shells out to per-language external formatters (see 'Language support' below; clang-format >= 14 required for C/C++/Java); any language can be redirected via `U50_STYLE_<LANG>` (see 'Formatter overrides').

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

clang-format >= 14 is required; when a formatter binary is missing the engine errors with a per-language message naming the tool and its install hint (e.g. "clang-format is required (>= 14) to check C/C++/Java style").

### Exit codes

- 0 — all files clean.
- 1 — style violations found in the processed files (CLI maps `!Report::clean()` to 1).
- 3 — any per-file error: unreadable file, unsupported extension, or formatter missing/failing (built-in or override). Files before the error are still checked and rendered (stdout); error lines go to stderr. Takes precedence: any error → 3 even if violations were also found.

### Not yet implemented (present in the original; future work)

- `-i` (in-place fix), `--ignore`, `--clang-format-style` (custom style override).
- `score` and `html` output modes (style50 v2 features).
- comment-count hints (style50's "But consider adding more comments!" suggestion).

## Verified against style50 2.10.4

Findings below were empirically verified live against the installed `/usr/bin/style50` (v2.10.4).

- **Formatter parity**: for C, C++, Java, Python, and JavaScript, u50's expected formatting is BYTE-IDENTICAL to style50's own (reconstructed from `style50 -o unified` diffs), and each tool accepts the other's output as clean in both directions. Python/JS output also byte-identical to the underlying CLIs (`autopep8`/`js-beautify`) run directly.
- Installed style50 2.10.4 `style50 -E` = `[c, h, cpp, hpp, py, js, java]` — it does NOT support css/sql/html (the 8-language support is newer/main-branch). u50 now matches the released language set exactly: CSS, SQL, and HTML support was removed (the `sqlformat` dependency was dropped with it), leaving no language u50 accepts that style50 2.10.4 rejects.
- **Exit codes**: style50 2.10.4 exits 0 even when the file has violations; u50 exits 1 (deliberate, documented in `u50_cli/AGENTS.md`).
- style50 skips unknown file types with a warning (rc=0); u50 errors with exit 3.
- **Presentation divergences (cosmetic)**: style50 prints a "Results generated by style50 vX" banner, "Looks good!", comment-count hints ("But consider adding more comments!" — a feature u50 lacks), and a "\n means insert a newline" legend in character mode; style50's character mode renders the original text with ins/del spans while u50 renders -/+ lines; style50's unified mode is not a patchable git diff (no `@@`/`---`/`+++`), u50's is.
- **JSON schema differs by design**: style50 emits `{files: [{name, score, comments, diff(html), warn_chars, loc}], score, version}`; u50 emits `{clean, files: [{path, clean, patch}]}`.
- style50 colors via `termcolor` (tty-aware); u50's `--color auto` honors `NO_COLOR` but has no tty check (colored output when piped).

Workspace-wide conventions (Rust edition 2024, workspace dependencies, clippy pedantic, CI gates) live in the root [AGENTS.md](../AGENTS.md) and are not repeated here.
