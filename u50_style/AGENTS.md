# u50_style — style50 reimplementation

Rust rewrite of [style50](https://github.com/cs50/style50): checks code style and reports style violations.

**Read the original first**: consult the [style50 Python repo](https://github.com/cs50/style50) before assuming CLI flags, output format, or style-rule semantics. Record findings here (below) so later agents don't have to re-read the Python source.

## Status

Engine implemented, supporting all 8 original languages via external formatter binaries: `run(&Request) -> Result<Report>` uses `Cs50Formatter` (impl of the `Formatter` trait, injectable via `run_with` for tests without the formatters installed), renders per-file diffs (`character` with inline char emphasis / `split` / `unified` / `json`) and prints them; the CLI maps `Report::clean()` to exit 0/1.

API: `Language` (`detect_language` by extension for all 8 languages; `Language::required_tool()` names the backing binary), `Formatter` trait, `Cs50Formatter`, `Request { files, output, color }`, `FileResult { path, clean, rendered }`, `Report { results }` with `clean()`. Pure renderers (`render_character`/`render_split`/`render_unified`/`json_document`) take `(source, formatted, ...)` and are unit-testable without the tools.

## Language support

All 8 languages of the original `style50/languages.py`:

| Language | Extensions | Backend tool | Install hint |
|---|---|---|---|
| C | c, h | `clang-format` (>= 14) | distro package |
| C++ | cpp, hpp, cc, cxx | `clang-format` (>= 14) | distro package |
| Java | java | `clang-format` (>= 14) | distro package |
| Python | py | `autopep8` | `pip install autopep8` |
| JavaScript | js | `js-beautify` | `pip install jsbeautifier` |
| HTML | html | `djhtml` | `pip install djhtml` |
| CSS | css | `css-beautify` | `pip install cssbeautifier` |
| SQL | sql | `sqlformat` | `pip install sqlparse` |

Per-tool options (mirroring the original's `languages.py` option values; flag names verified against the installed CLIs):

- C/C++/Java: `clang-format --assume-filename=<foo.c|foo.cpp|foo.java> -style=<CS50 config>` (config embedded in `CS50_CLANG_FORMAT_CONFIG`, below).
- Python: `autopep8 - --max-line-length=100 --ignore-local-config` (original: `autopep8.fix_code(..., options={'max_line_length': 100, 'ignore_local_config': True})`).
- JavaScript: `js-beautify --end-with-newline --operator-position preserve-newline -w 100 --brace-style collapse,preserve-inline --keep-array-indentation -` (original: `jsbeautifier.beautify(...)` with the same option values; the short `-w 100` form is required because the CLI declares the long `--wrap-line-length` as taking no argument, and the `-` stdin marker must come last).
- HTML: `djhtml -` — **`djhtml` exits 1 when it reformats** (the `diff`/`black` convention); the engine treats exit 0, or exit 1 with non-empty stdout, as success; exit > 1, or exit 1 with empty stdout, is an error.
- CSS: `css-beautify --indent-size 4 --end-with-newline -` (original: `cssbeautifier.beautify(..., options={'indent_size': 4, 'end_with_newline': True})`; `-` stdin marker last, as with js-beautify).
- SQL: `sqlformat --reindent --keywords upper --indent_width 4 -` (original: `sqlparse.format(..., reindent=True, keyword_case='upper', indent_width=4)`). The CLI takes a single FILE positional (`-` = stdin) and writes to stdout, spells the option `--indent_width` with an underscore, and does not end output with a newline — the engine appends one when missing, matching the original's SQL class.

The original calls the Python libraries (`autopep8`, `jsbeautifier`, `cssbeautifier`, `sqlparse`) directly and only `djhtml` as a process; u50 uniformly shells out to the pip CLIs, which apply the same defaults. A missing binary produces a per-language error, e.g. "`autopep8` is required to check Python style (pip install autopep8)".

## Behavior notes

Findings recorded from the official docs: https://cs50.readthedocs.io/style50/

### Usage

- Usage: `style50 <file>` — checks file(s) against CS50's style guide.
- Languages: C, C++, Java, Python, JavaScript, HTML, CSS, SQL.
- Under the hood it shells out to per-language external formatters (see 'Language support' below; clang-format >= 14 required for C/C++/Java).

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
- 1 — style violations found (CLI maps `!Report::clean()` to 1).
- 3 — infrastructure error (clang-format missing/failing).

### Not yet implemented (present in the original; future work)

- `-i` (in-place fix), `--ignore`, `--clang-format-style` (custom style override).
- `score` and `html` output modes (style50 v2 features).

Workspace-wide conventions (Rust edition 2024, workspace dependencies, clippy pedantic, CI gates) live in the root [AGENTS.md](../AGENTS.md) and are not repeated here.
