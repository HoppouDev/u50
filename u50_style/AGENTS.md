# u50_style — style50 reimplementation

Rust rewrite of [style50](https://github.com/cs50/style50): checks code style and reports style violations.

**Read the original first**: consult the [style50 Python repo](https://github.com/cs50/style50) before assuming CLI flags, output format, or style-rule semantics. Record findings here (below) so later agents don't have to re-read the Python source.

## Status

Engine implemented, supporting all 8 languages of style50 3.0.0. Directory arguments are expanded recursively (`expand_paths`, style50 3.0.0's `os.walk` expansion): `run(&Request) -> Result<Report>` uses `Cs50Formatter::default()` (impl of the `Formatter` trait, injectable via `run_with` for tests without the formatters installed), renders per-file diffs (`character` with inline char emphasis / `split` / `unified` / `json`) and prints them; the CLI maps `Report::clean()` to exit 0/1.

API: `Language` (`detect_language` by extension for all supported languages; `Language::required_tool() -> Option<&'static str>` names the backing binary), `Formatter` trait, `Cs50Formatter` (`Default` = built-in tools resolved cache-only), `Request { files, output, color }`, `FileResult { path, clean, rendered, formatted }`, `Report { results, errors }` with `clean()` and `has_errors()`. Pure renderers (`render_character`/`render_split`/`render_unified`/`json_document`) take `(source, formatted, ...)` and are unit-testable without the tools.

Per-file errors never abort the run: an unreadable file, unsupported extension, or formatter failure for one file records `(path, message)` in `Report.errors` and processing continues with the remaining files, so earlier results are preserved.

## Directory arguments (style50 3.0.0 parity)

Both `run_with` and `fix_with` call `expand_paths(&req.files)` before processing, mirroring style50 3.0.0's `FILE [FILE ...] — file or directory to lint` (`os.walk` expansion with `followlinks=false`):

- A **directory** argument is walked recursively; only **regular files** whose `detect_language` is `Some` are collected (the extension filtering style50 applies while walking; FIFOs/devices and symlinked entries inside the tree are skipped — no blocking opens). **Hidden directories are included** — in the original, exclusion is `--ignore`'s job (e.g. skipping `node_modules`), which u50 has not implemented yet.
- **Symlinked directories are not followed** (top-level `symlink_metadata` reads a link as non-directory, and links inside a walked tree are neither descended into nor collected) — matches `os.walk`'s default.
- Anything else (file, symlink to file, missing path) is kept **unchanged**, so explicit file arguments keep their per-file error semantics (unsupported extension → exit 3, missing → `could not read`).
- A directory with zero supported files contributes nothing (no error — style50 likewise skips unknown types); **unreadable directories are skipped silently** (also matches `os.walk`'s ignored-error default).
- The final file list is **deduplicated** (a directory and a file inside it may both be named) and **sorted** for deterministic output, applied identically to check and `--fix`. `run()` prints rendered output for every processed file to stdout (stdout stays pure diff/JSON), then writes each error to stderr as `error: <path>: <message>`. Formatter-level failures (e.g. missing clang-format) are therefore per-file skips, not whole-run bails.

## In-place fix

`fix(&Request, dry_run: bool) -> Report` (and the injectable `fix_with(&Request, &dyn Formatter, dry_run) -> Report`) rewrites dirty files in place, mirroring the original style50's `-i`/`--in-place`. It reuses the exact per-file machinery of the style check (a shared `process_file` helper), so clean/dirty and error semantics (normalization, empty-file error, per-file error continuation) are identical.

- `FileResult.formatted: Option<String>` — the style50-styled content for every successfully processed file (the normalized input when clean); `None` only for files that could not be processed. Fix mode writes this back byte-for-byte (the styled content, never the rendered diff).
- Write policy: a file is written only when it is dirty, `dry_run` is false, and no error occurred. Clean files and dry runs never touch the filesystem; write failures are recorded in `Report.errors` as `could not write \`<path>\`: ...` (no bail).
- Printing policy (in `fix`, engine-side like `run()`): plain fix prints per file `fixed: <path>` or `already clean: <path>` to stdout; dry run prints `would fix: <path>` or `already clean: <path>` instead (nothing is written, no diff is rendered, and the exit code 1 signals what would have changed). Errors always go to stderr as `error: <path>: <message>`.
- Exit-code contract (implemented in u50_cli): **0** — plain fix succeeded (every file fixed or already clean); **1** — dry run with at least one would-fix (check-style convention); **3** — any per-file error (unreadable/unsupported/empty/formatter/write failure), taking precedence.
- The CLI exposes this as `u50 style --fix [--dry-run]`; `--dry-run` requires `--fix`, and `--fix` conflicts with `-o/--output` (fix output is the fixed/already-clean lines or the dry-run diff, not a chosen render mode).
- **Adaptive diff strategy** (`render::select_algorithm`): diff rendering defaults to Myers but engages the Lcs algorithm for large, low-overlap pairs — inputs where the larger side has ≥ 1024 lines **and** the distinct shared lines number fewer than `max_lines / 1000` (a `HashSet` intersection probe, linear). Measured (release, unified render, `examples/bench_diff.rs`):

  | input | Myers | Lcs |
  | --- | --- | --- |
  | golden 2.5k real dirty→expected (26 distinct common) | 11.5ms | 0.57ms |
  | 7.5k wholly-dirty (0 common) | 509.6ms | 205.7ms |
  | 60k wholly-dirty (0 common) | 32.21s | 13.14s |
  | 60k, 28 distinct common (earlier build) | 32.5s | 12.9s |
  | 7.5k, 8 distinct common (earlier build) | ~1s | ~3s (collapse) |

  The 1000x density multiplier is a **measured heuristic**, not a constant with first-principles meaning: the Lcs→Myers crossover lies between the 8-common@7.5k collapse and the 28-common@60k win, and `cargo run --release -p u50_style --example bench_diff` records the full matrix behind it. Patience was also measured and is worse than both on wholly-dirty input (1.09s @7.5k, 69.2s @60k). Note: outputs of `bench_diff` revisions printed before this note had the `patience`/`lcs` header labels swapped relative to the measured cell order `[Myers, Lcs, Patience]` (fixed in the harness; the algorithm attribution above is verified by a one-off probe: 7.5k wholly-dirty → Myers 518ms / Lcs 186ms / Patience 1.35s). The strategy is **display-only**: formatter results and clean/dirty decisions are unaffected; student-scale files and the committed goldens render sub-second either way.

## Language support

All 8 languages of **style50 3.0.0** (per `style50 -E` → `[c, h, cpp, hpp, py, js, java, html, css, sql]`):

| Language | Extensions | Backend tool | Install hint |
| --- | --- | --- | --- |
| C | c, h | `clang-format` (>= 14) | auto-provisioned by u50 on first use |
| C++ | cpp, hpp | `clang-format` (>= 14) | auto-provisioned by u50 on first use |
| Java | java | `clang-format` (>= 14) | auto-provisioned by u50 on first use |
| Python | py | `autopep8` | auto-provisioned by u50 on first use |
| JavaScript | js | `js-beautify` | auto-provisioned by u50 on first use |
| HTML | html | `djhtml` | auto-provisioned by u50 on first use |
| CSS | css | `css-beautify` | auto-provisioned by u50 on first use |
| SQL | sql | `sqlformat` | auto-provisioned by u50 on first use |

Per-tool options (mirroring the original's `languages.py` option values; flag names verified against the installed CLIs):

- C/C++/Java: `clang-format --assume-filename=<foo.c|foo.cpp|foo.java> -style=<CS50 config>` (config embedded in `CS50_CLANG_FORMAT_CONFIG`, below).
- Python: `autopep8 - --max-line-length=100 --ignore-local-config` (original: `autopep8.fix_code(..., options={'max_line_length': 100, 'ignore_local_config': True})`).
- JavaScript: `js-beautify --end-with-newline --operator-position preserve-newline -w 100 --brace-style collapse,preserve-inline --keep-array-indentation -` (original: `jsbeautifier.beautify(...)` with the same option values; the short `-w 100` form is required because the CLI declares the long `--wrap-line-length` as taking no argument, and the `-` stdin marker must come last).
- HTML: `djhtml -` via the **lenient** runner: exit 0 is success, and exit 1 with non-empty stdout is also treated as success (older djhtml releases followed the diff/black "exit 1 = reformatted" convention, which is what `languages.py`'s `exit=None` accommodates). Observation: the installed djhtml (3.0.6; also the pinned 3.0.11) **always exits 0**, even when it reformats — the source comment is stale for those versions; the lenient runner covers both conventions.
- CSS: `css-beautify --indent-size 4 --end-with-newline -` (original: `cssbeautifier.beautify(...)` with `indent_size = 4, end_with_newline = True`; verified byte-identical to the library call).
- SQL: `sqlformat -k upper -r --indent_width 4 -`, with a `\n` appended when the output lacks one (matching the original's `Sql.style` fix-up). `sqlformat` is the CLI of the same `sqlparse` library the original calls; verified byte-identical to `sqlparse.format(code, reindent=True, keyword_case='upper', indent_width=4)` + trailing-newline append.

The original calls the Python libraries (`autopep8`, `jsbeautifier`, `cssbeautifier`, `sqlparse`) directly; u50 shells out to the pip CLIs, which apply the same defaults. All backends are auto-provisioned by u50 into its cache on first use (see 'Tool management'); a system-wide `pip install` still works but is not required. When provisioning fails (or is disabled via `U50_STYLE_NO_PROVISION=1`) and the binary is absent, a per-language error is produced, e.g. "`autopep8` is required to check Python style (pip install autopep8)".

## Tool management (`--status` / `--setup`)

The 6 backing formatters are all pip-installable: `clang-format` (standalone
binary wheel), `autopep8`, `jsbeautifier` (bin `js-beautify`), `djhtml`,
`cssbeautifier` (bin `css-beautify`), and `sqlparse` (bin `sqlformat`).
Mapping per language: C/C++/Java → clang-format, Python → autopep8,
JavaScript → js-beautify, HTML → djhtml, CSS → css-beautify, SQL → sqlformat
(`Language::pip_package`). u50 installs them **itself** into a uv-managed
cache — `$XDG_CACHE_HOME`/`~/.cache` → `u50/style50` (paths built by
`cache_dir()`; binaries in `cache_bin_dir()` = `<cache>/venv/bin`) — with no
system pip, no distro packages, and no PATH reliance. Tool resolution is
**cache-only** (see below), so a missing backend is simply one not yet in
the cache, and such backends are auto-provisioned lazily on first use (see
'Lazy auto-provisioning', below). That makes `--setup` the
**bulk/explicit** path — for pre-downloading everything up front (CI,
ahead of offline runs) — but never a prerequisite for formatting.

### `u50 --status`

Exposed as a **root-level** flag (`u50 --status`), not a style subcommand
flag; the library entry point stays `list_languages()` (see
`u50_cli/AGENTS.md`).

Prints an aligned table of languages, extensions, backing binary, and
status. Because bare tool names resolve cache-only, the status is
`found (cache)` when the binary is in the u50 cache and `missing` when it
is not — the system `PATH` is never consulted and never reported. `--status`
**never provisions**; it purely reports:

```text
Language    Extensions  Binary        Status
----------  ----------  ------------  -------------
C           c, h        clang-format  found (cache)
C++         cpp, hpp    clang-format  found (cache)
Java        java        clang-format  found (cache)
Python      py          autopep8      found (cache)
JavaScript  js          js-beautify   found (cache)
HTML        html        djhtml        found (cache)
CSS         css         css-beautify  found (cache)
SQL         sql         sqlformat     found (cache)
```

Always exits 0. Combined with `--setup` or a subcommand it is a usage
error (exit 2) — enforced by the dispatcher, like `--setup` (see
`u50_cli/AGENTS.md`).

### `u50 --setup` (root flag)

Exposed as a **root-level** flag (`u50 --setup`), not a style subcommand
flag; the library entry point stays `setup_missing()`. Combined with a
subcommand it is a usage error (exit 2) — see `u50_cli/AGENTS.md`.

Installs missing formatter backends into u50's cache in-process (uv library
calls — no pip subprocesses, no system Python required). Same in-process
flow as lazy provisioning, but as the **bulk/explicit** path: single tools
are also provisioned automatically on first use (see 'Lazy
auto-provisioning', below), so `--setup` is not needed anymore on a bare
machine — it exists to pre-download everything up front and to report
per-package outcomes:

1. Determines the distinct pip packages of tools not resolvable from the
   cache; if none, prints `all formatter backends are already available`.
2. Prints `installing N package(s) into <cache dir>`.
3. Provisions a uv-managed CPython 3.14 and a venv at `<cache>/venv` when
   absent. The venv's console scripts carry absolute shebangs and are
   self-contained, so no environment fixup is needed at spawn time.
4. **Parallel downloads**: one task and one spinner per package (indicatif
   `MultiProgress`), each fetching the package's best wheel via the PyPI
   JSON API. Backend versions are pinned (`PINNED_VERSIONS`, matching
   [`tests/tool-versions.txt`](tests/tool-versions.txt) — the golden-fixture
   source of truth); each backend's hardcoded transitive runtime
   dependencies (`TRANSITIVE_DEPS`) are fetched alongside it, unpinned
   (latest release). Previously fetched wheels are reused across runs.
5. Installs every fetched wheel into the venv in a single `uv-installer`
   call.
6. Prints per-package `installed: <pkg> (<tool>)` or `failed: <pkg>: <reason>`
   (a package counts as installed only when its tool is resolvable from the
   cache bin dir afterwards), then prints the status table. Any failure
   exits 3.

### Cache-only tool resolution

BUILT-IN formatter tools resolve via `locate_tool(tool)`, which resolves
**bare tool names from `<cache>/venv/bin` ONLY — the system `PATH` is
never consulted**, so a hostile or unrelated same-named binary on `PATH`
can never be picked up. A bare built-in tool absent from the cache is
never spawned by name (which would let `Command::new` fall back to the OS
`PATH`): the call sites check `locate_tool` once and emit the standard
missing-tool error instead of spawning. Cache hits spawn by resolved path.

### Lazy auto-provisioning

When a language's backend is missing from the cache at format time, u50 auto-provisions
it on first use: the tool is mapped to its pip package and installed via
the same install core as `--setup` (uv library path — managed CPython 3.14
if needed, venv, pinned wheel + transitive deps), deduplicated per process
(`ensure_backend_once`: the first missing-tool occurrence per run triggers
provisioning; later files in the same run skip straight to the error when
the first attempt failed). Provisioning failures degrade to the standard
per-file missing-tool error (exit 3). Set `U50_STYLE_NO_PROVISION=1` to
disable auto-provisioning entirely (hermetic tests / CI).

Verified end-to-end: a fake same-named binary planted on `PATH` is never
used (cache-only resolution ignores `PATH` entirely), and on an empty cache
u50 auto-provisions the needed backend on first format and then formats
correctly via the cache (`found (cache)` in `--status`), exercising both the
managed CPython + venv provisioning and the standalone `clang-format`
binary wheel.

## Input normalization (style50 3.0.0 semantics)

Before formatting and comparison, u50 normalizes the file's source exactly as style50 3.0.0's `_api.py` does:

1. **rstrip every line** (trailing whitespace, including `\r`, removed),
2. **join with `\n`**,
3. **ensure a trailing `\n`** (append one if missing).

Formatting and the clean/dirty comparison both operate on the normalized text, so:

- **trailing whitespace is never flagged** (a file that only differs by trailing whitespace is clean),
- **CRLF input is normalized to LF**,
- a missing final newline is not flagged.

An empty or whitespace-only file normalizes to `""` and is reported as a **per-file error** `file is empty` (style50 3.0.0 raises the same error when `count_lines` is 0; its default `count_lines` ignores blank lines). This replaces the earlier behavior of reporting such files clean.

## Behavior notes

Findings recorded from the official docs: <https://cs50.readthedocs.io/style50/>

### Usage

- Usage: `style50 <file>` — checks file(s) against CS50's style guide.
- Languages: C, C++, Java, Python, JavaScript, HTML, CSS, SQL (style50 3.0.0; see 'Language support').
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
- 1 — style violations found in the processed files (CLI maps `!Report::clean()` to 1).
- 3 — any per-file error: unreadable file, unsupported extension, or formatter missing/failing. Files before the error are still checked and rendered (stdout); error lines go to stderr. Takes precedence: any error → 3 even if violations were also found.

### Not yet implemented (present in the original; future work)

- `--ignore` (the mechanism for excluding directories such as `node_modules` from the directory walk), `--clang-format-style` (custom style override).
- `score` and `html` output modes (style50 v2 features).
- comment-count hints (style50's "But consider adding more comments!" suggestion).

## Golden fixture tests

`tests/golden.rs` compares u50's formatter output against **ground truth generated by style50 3.0.0 itself**: for each language, `tests/fixtures/<lang>/dirty.<ext>` is a deliberately badly formatted input and `tests/fixtures/<lang>/expected.<ext>` is the output of `style50 -o format dirty.<ext>`, which was then verified clean by style50 (`style50 -o unified expected.<ext>` shows no diff). The test asserts u50's `Cs50Formatter` output on the normalized dirty fixture is byte-identical to `expected.<ext>`, plus one test re-checking every expected fixture is clean per u50's own engine (`run_with`). Both dirty and expected fixtures are committed.

Run:

```sh
U50_STYLE_GOLDEN=1 cargo test --test golden
```

**Gating/skip behavior**: each language's golden test runs only when `U50_STYLE_GOLDEN=1` is set AND the language's backing tool is found in the u50 style cache (cache-only resolution — the system `PATH` is never consulted); otherwise the test prints a `skip` line and returns. The ground truth is only byte-stable for a given set of tool versions, so backend versions are pinned in [`tests/tool-versions.txt`](tests/tool-versions.txt) (a pip constraints file — the single source of truth). CI provisions exactly those versions via `u50 --setup` (dogfooding u50's own installer) and runs the golden suite as a dedicated step; locally, run `u50 --setup` once, then `U50_STYLE_GOLDEN=1 cargo test --test golden` (`u50 --status` shows what is in your cache). When a backend is upgraded, refresh this file, `tool-versions.txt`, and the goldens together.

### Large real-world fixtures and provenance

The fixtures are **large, real-world, minified sources** (50–90KB dirty / 60–200KB
expected per language), not toy snippets: c=cJSON, cpp=JsonCpp, java=Guava
`ImmutableList`, py=Werkzeug `routing/map.py`, js=official jQuery 3.7.1
`jquery.min.js`, css=normalize.css, html=Bootstrap 5.3 dashboard example,
sql=collapsed Supabase migrations. Per-language provenance (project, upstream
source file, license SPDX/copyright, and the exact minification applied) is
documented in [`tests/fixtures/NOTICE.md`](tests/fixtures/NOTICE.md) — update
that file whenever a fixture's provenance changes.

### Fixed-point generation procedure

Golden equality requires BOTH `format(dirty) == expected` AND `format(expected)
== expected` (byte-clean per style50). The fixtures therefore satisfy both
invariants **by construction**:

1. Start from the minified upstream source `dirty0`.
2. Iterate `X_{i+1} := style50 -o format X_i` (up to 25 passes) until `X_{i+1}
   == X_i` byte-equal. The converged `F := X_i` becomes `expected` — a fixed
   point, so `format(expected) == expected` holds by definition.
3. Set `dirty := X_{n-1}`, the iterate immediately before convergence, so
   `format(dirty) == F == expected` holds by construction. For languages
   converging on the first pass, use `dirty := dirty0` (the minified upstream
   source) instead — `format(dirty0) == F` directly, giving a true
   transformation test rather than an idempotency-only check. A vacuous
   `dirty == expected` fixture fails the golden test's `assert_ne!` guard.

   Fixture classification: c, cpp, js, sql use `X_{n-1}` (style50 needs 2-3
   passes on their large minified inputs); py and html use `dirty0`; java is
   `X_{n-1}` with a 1-byte diff (borderline — the X0->X1 step for java, and
   every language's first transformation step, is separately covered by the
   byte-parity verification against `style50 -o format` during generation).

If a language does not converge in 25 passes, shrink/replace that language's
source (e.g. truncate the minified lib at a top-level statement boundary that
still parses) and restart its iteration.

Regenerate (after adjusting a dirty fixture) — `style50 -o format` is the oracle, and the second command must show **no diff**:

```sh
cd u50_style/tests/fixtures
cp <lang>/dirty.<ext> /tmp/dirty && style50 -o format /tmp/dirty.<ext> > <lang>/expected.<ext>
style50 -o unified <lang>/expected.<ext>   # must show no +/- lines
```

## Verified against style50 3.0.0

Findings verified live against the installed style50 **3.0.0** (`/usr/lib/python3.14/site-packages/style50/`):

- `style50 -E` = `[c, h, cpp, hpp, py, js, java, html, css, sql]` — all 8 languages, which u50 now matches.
- **Oracle**: `style50 -o format <file>` prints the styled code directly; used as ground truth to byte-compare u50's formatter output for every language — u50's output is byte-identical to style50's, and each side accepts the other's output as clean (per-language PASS table in the session log; all 8 PASS).
- **djhtml exit code**: `languages.py` comments that djhtml returns exit 1 when it reformats (hence its `exit=None`), but the installed djhtml (3.0.6; also the pinned 3.0.11) **exits 0** even when reformatting — the comment is stale for those versions. u50 uses a lenient runner that accepts both conventions.
- **CLI-vs-library byte-identity**: `css-beautify --indent-size 4 --end-with-newline -` is byte-identical to the `cssbeautifier.beautify(...)` call style50 makes; `sqlformat -k upper -r --indent_width 4 -` (plus a missing trailing-newline append) is byte-identical to `sqlparse.format(code, reindent=True, keyword_case='upper', indent_width=4)` + fix-up.
- **Input normalization**: `_api.py` rstrips every line, joins with `\n`, and ensures a trailing `\n` before `check()`; empty/whitespace-only files raise `Error("file is empty")` (`count_lines` ignores blank lines → `ZeroDivisionError`). u50 implements the same semantics.

## Verified against style50 2.10.4

Findings below were empirically verified live against the installed `/usr/bin/style50` (v2.10.4).

- **Formatter parity**: for C, C++, Java, Python, and JavaScript, u50's expected formatting is BYTE-IDENTICAL to style50's own (reconstructed from `style50 -o unified` diffs), and each tool accepts the other's output as clean in both directions. Python/JS output also byte-identical to the underlying CLIs (`autopep8`/`js-beautify`) run directly.
- Installed style50 2.10.4 `style50 -E` = `[c, h, cpp, hpp, py, js, java]` — it does NOT support css/sql/html (the 8-language support is newer/main-branch). u50 now matches the released language set exactly: CSS, SQL, and HTML support was removed (superseded: HTML/CSS/SQL support was restored with the style50 3.0.0 alignment; see Language support above) — the `sqlformat` dependency was dropped with it — leaving no language u50 accepts that style50 2.10.4 rejects.
- **Exit codes**: style50 2.10.4 exits 0 even when the file has violations; u50 exits 1 (deliberate, documented in `u50_cli/AGENTS.md`).
- style50 skips unknown file types with a warning (rc=0); u50 errors with exit 3.
- **Presentation divergences (cosmetic)**: style50 prints a "Results generated by style50 vX" banner, "Looks good!", comment-count hints ("But consider adding more comments!" — a feature u50 lacks), and a "\n means insert a newline" legend in character mode; style50's character mode renders the original text with ins/del spans while u50 renders -/+ lines; style50's unified mode is not a patchable git diff (no `@@`/`---`/`+++`), u50's is.
- **JSON schema differs by design**: style50 emits `{files: [{name, score, comments, diff(html), warn_chars, loc}], score, version}`; u50 emits `{clean, files: [{path, clean, patch}]}`.
- style50 colors via `termcolor` (tty-aware); u50's `--color auto` honors `NO_COLOR` but has no tty check (colored output when piped).

Workspace-wide conventions (Rust edition 2024, workspace dependencies, clippy pedantic, CI gates) live in the root [AGENTS.md](../AGENTS.md) and are not repeated here.
