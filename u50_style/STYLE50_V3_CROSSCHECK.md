# style50 3.0.0 Cross-Check — u50 style vs. the original

- **Date**: 2026-09-06 (registers #1–#3 resolved 2026-09-06: pretty-printing fixed, `Auto` tty-gated, Python denominator fixed; see register)
- **Original**: `style50 3.0.0` (`/usr/bin/style50`, Python 3.14 site-packages)
- **u50**: `main` at `0933fc5` (the doc commit itself; the style engine analyzed is from `c02a6cc` — renderer abstraction + score mode, PR #2)
- **Cached backend versions** (u50 cache venv `~/.cache/u50/style50/venv`, identical to the pins in `tests/tool-versions.txt`): `clang-format 22.1.8`, `autopep8 2.3.2`, `jsbeautifier 2.0.3` (bin `js-beautify`), `cssbeautifier 2.0.3` (bin `css-beautify`), `djhtml 3.0.11`, `sqlparse 0.5.3` (bin `sqlformat`).
  - **djhtml note**: the u50 cache pins djhtml **3.0.11** (matching `tool-versions.txt`). The system `PATH` has no `djhtml` at all, so a stock `style50` run fails on `.html` files with `DependencyError: djhtml` (exit 1). For the HTML legs below, `style50` was re-run with the u50 cache venv's `bin/` prepended to `PATH` so both tools exercised the same djhtml 3.0.11.

## Methodology

Every scenario ran both tools on **byte-identical inputs** in fresh working directories during one session, with stdout/stderr and exit codes captured for each. Format-parity comparisons are `sha256` byte comparisons of the resulting files; fixpoint checks re-ran each tool on its own output. Samples quoted below are trimmed verbatim (ANSI stripped). u50 was `./target/debug/u50` built from `c02a6cc`.

Infra note (not a discrepancy, by design): u50 resolves formatter backends from its own cache only (`PATH` is never consulted); the original resolves via `PATH`. To make the comparison fair, style50 was given the u50 cache venv on `PATH`.

## 1. Language sets

```
$ style50 -E
["c", "h", "cpp", "hpp", "py", "js", "java", "html", "css", "sql"]

$ u50 --status          # same extension set, per-language backend table
C    c, h        clang-format   found (cache)
C++  cpp, hpp    clang-format   found (cache)
...  (all 8 languages, all backends found)
```

**Match** — identical 10-extension / 8-language set.

## 2. Format parity (`style50 -o format` vs `u50 style --fix`)

Inputs: `u50_style/tests/fixtures/<lang>/dirty.<ext>` copied twice; style50's stdout written to a file, u50's `--fix` rewriting in place; outputs byte-compared (sha256).

| Language | style50 exit | u50 exit | sha256 match | style50 fixpoint | u50 fixpoint |
|----------|--------------|----------|--------------|------------------|--------------|
| C        | 0 | 0 | PASS (`b6ade28e…`) | clean (`format(out)==out`) | `already clean`, exit 0 |
| C++      | 0 | 0 | PASS (`ef1e8dc1…`) | clean | `already clean`, exit 0 |
| CSS      | 0 | 0 | PASS (`2ed67560…`) | clean | `already clean`, exit 0 |
| HTML     | 0 (with djhtml on PATH; exit 1 `DependencyError` without) | 0 | PASS (`72cfe75c…`) | clean | `already clean`, exit 0 |
| Java     | 0 | 0 | PASS (`9ab8ca11…`) | clean | `already clean`, exit 0 |
| JS       | 0 | 0 | PASS (`465d6378…`) | clean | `already clean`, exit 0 |
| Python   | 0 | 0 | PASS (`f441da2f…`) | clean | `already clean`, exit 0 |
| SQL      | 0 | 0 | PASS (`45410747…`) | clean | `already clean`, exit 0 |

**8/8 byte-identical; both tools are fixpoint-stable in all 8 languages.**

## 3. Normalization & edge cases

Small Python and C inputs; style50 in default (character) mode, u50 in default mode.

| Case | style50 3.0.0 | u50 | Verdict |
|------|---------------|-----|---------|
| Trailing whitespace per line (py, c) | reformats & re-renders styled snippet, hint lines; **exit 0** | `+/-` diff of the same changes; **exit 1** | same formatting engine, different report/exit (by design) |
| CRLF line endings | normalizes silently, formats; exit 0 | normalizes silently, formats (diff only where style actually differs); exit 1 | same engine behavior |
| Missing final newline | formats, adds newline; exit 0 | same formatting result; exit 1 | same engine behavior |
| Whitespace-only file | `file is empty` printed **to stdout**, exit 0 | `error: test.py: file is empty` **to stderr**, exit 3 | divergence (see register #4) |
| Empty file | same as above: stdout message, exit 0 | stderr error, exit 3 | divergence (register #4) |
| Unsupported extension (`.txt` with content) | `unknown file type "test.txt", skipping...` on stdout, **skips**, exit 0 | `error: test.txt: unsupported file type…` on stderr, **fatal for that file**, exit 3 | divergence (register #4) |
| Nonexistent file | `file "nope.py" not found` on stdout, exit 0 | `error: nope.py: could not read…` on stderr, exit 3 | divergence (register #4) |
| Directory argument | **walks recursively**, per-file `:::::::::::::: name ::::::::::::::` blocks, exit 0 | **walks real directories recursively**, renders diffs, exit 1 (dirty) | both walk; presentation/exit differ |
| Symlinked directory argument | follows the symlink, formats targets, exit 0 | `error: link: could not read \`link\`: Is a directory (os error 21)`, exit 3 | **gap** (register #5) |
| Two files, first broken (syntax error) | per-file block `failed to parse code, check for syntax errors!`, **continues** to second file, exit 0 | reports first file's parse error (stderr), **continues** to second file, exit 1/3 | both continue past broken files |

## 4. Exit codes

| Scenario | style50 3.0.0 | u50 |
|----------|---------------|-----|
| All files clean | 0 | 0 |
| Style violations found | 0 | 1 |
| Per-file error (empty/whitespace-only, missing, unsupported ext, unreadable, formatter failure) | 0 (message on stdout; tool continues) | 3 (error on stderr; earlier files still checked) |
| Usage error (no operands, unknown flag) | 2 | 2 |
| Uncaught exception (e.g. `DependencyError`) | 1 | n/a (u50 pre-flights dependencies; missing backend → 3) |

style50 3.0.0 effectively has only 0 / 1(exception) / 2(usage); u50's 0/1/2/3 scheme is a documented by-design divergence (`u50_style/AGENTS.md`, "Exit codes").

## 5. Output modes

Samples: character/split use a tiny C file (`int main(void){return 0;}`); unified/json/score use `dirty.py` = `x=1` (single-line file).

### character (default)

style50 (banner + re-rendered styled code + hints; **not** a diff):

```
Results generated by style50 v3.0.0

int main(void)\n
{\n
    return 0;\n
}

\n means that you should insert a newline.
And consider adding more comments!
```

u50 (style50-parity presentation — banner, char-level colors; u50-branded banner, no comments hint):

```
Results generated by u50 v0.1.0

int main(void)\n
{\n
    return 0;\n
}

\n means that you should insert a newline.
```

Clean file: style50 prints `Results generated by style50 v3.0.0` / `Looks good!` / `But consider adding more comments!`; u50 prints `Results generated by u50 v0.1.0` / `Looks good!` (no hint). **Resolved** (register #6): presentation now matches except the banner branding and the hint.

### split

style50 aligns original (left) and styled (right) columns with wide spacing; u50 uses `<orig> | <styled>` with a pipe separator. Layout-only difference.

### unified

style50's `-o unified` is **not a patchable unified diff**: it emits the banner plus context/`-`/`+` lines with a leading space and no `---`/`+++`/`@@` headers:

```
Results generated by style50 v3.0.0

- x=1
+ x = 1
```

u50 emits a real, `git apply`-able patch:

```
--- dirty.py
+++ dirty.py
@@ -1 +1 @@
-x=1
+x = 1
```

By-design divergence (register #7).

### json

style50 (indent-4 **pretty**-printed; HTML-escaped styled diff; score; version):

```json
{
    "files": [
        {
            "name": "dirty.py",
            "score": 0.0,
            "comments": true,
            "diff": "<pre>\nx<ins> </ins>=<ins> </ins>1\n</pre>",
            "warn_chars": [],
            "loc": 1
        }
    ],
    "score": 0.0,
    "version": "3.0.0"
}
```

u50 (also indent-4 pretty-printed since register #1's pretty-printing fix; unified patch, no score/loc/version — schema by design):

```json
{
    "clean": false,
    "files": [
        {
            "clean": false,
            "patch": "--- dirty.py\n+++ dirty.py\n@@ -1 +1 @@\n-x=1\n+x = 1\n",
            "path": "dirty.py"
        }
    ]
}
```

Verified live: **pretty-printing now matches** (register #1, resolved); the schema itself is a documented [by-design] divergence (register #1, schema half). Also: style50 exits 0 with JSON for dirty files; u50 exits 1.

### score

Both print one bare line, Python `str(float)`-formatted. Re-verified byte-parity:

| Input | style50 | u50 |
|-------|---------|-----|
| `fixtures/c/dirty.c` (cJSON) | `0.5036334275333064` | `0.5036334275333064` — identical |
| clean `.c` / clean `.py` | `1.0` | `1.0` |
| partially dirty `.c` (1 of 5 lines dirty) | `0.5` | `0.5` |
| tiny dirty `.py` (`x=1`) | `0.0` | `0.0` |
| `fixtures/py/dirty.py` (Werkzeug) | `0.9814814814814815` | `0.9807692307692307` — **differs** |

The Python-fixture difference is fully explained: both computed **diffs = 15.0** (identical diff), but style50's `Python.count_lines` counts **all** lines of the styled text (810; "blank lines are relevant to style per pep8", `languages.py`), while u50 counted **non-blank** lines (780). `1 − 15/810` vs `1 − 15/780`. Every other language uses the same non-blank count, which is why all C cases agree exactly. **Resolved** (register #2): u50 now counts all lines for `.py` files only; re-verified — u50 prints `0.9814814814814815` byte-identical to style50, clean files still `1.0`, `c/dirty.c` still `0.5036334275333064`.

Exit codes again differ: score mode with dirty files → style50 0, u50 1 (by design).

**Resolution (walk warning):** style50 warns `unknown file type "<path>", skipping...` when its directory walk hits unsupported files; u50's score mode now prints the same per-file warning on stdout before the score (style50-parity; text/fix modes print it on stderr instead, JSON stays silent — it never affects exit codes in u50).

### html

Not implemented in u50 (see register #8).

## 6. Color behavior

Piped (non-TTY) runs, ANSI escape counts on the same dirty C input:

| Tool | Piped output |
|------|--------------|
| style50 | **no color codes at all** — but still emits bare resets (11790 × `\x1b[0m`); no `\x1b[3x` SGRs |
| u50 | **full ANSI color even when piped**: `\x1b[32m` (green adds) ×2457, `\x1b[31m` (red dels) ×9, `\x1b[1m` (bold) ×7, resets ×2466 |

- u50's `resolve_ansi` (`u50_cli/src/main.rs`) had **no `is_terminal` gate**, so `auto` colored piped output. **Resolved** (register #3): `Auto` now requires stdout to be a tty — re-verified: piped run emits **0** ANSI escapes; `NO_COLOR=1` and `--color never` still suppress; `--color always` still forces color.
- style50 strips color when piped but leaks the per-line resets.
- u50's `-o score` emits no ANSI when piped (score line is never colored; error lines are only colored when color is on), matching the original's behavior in score mode.

**Resolved** (register #3): piped u50 output is no longer colored.

## 7. CLI surface

| style50 3.0.0 | u50 equivalent | Status |
|---------------|----------------|--------|
| `-o character` (default) | `-o character` (default) | implemented-verified (different presentation, by design) |
| `-o split` / `-y --side-by-side` | `-o split` | implemented-verified (`-y` shorthand not implemented; layout differs) |
| `-o unified` | `-o unified` | implemented-verified (u50 emits real patch — by-design divergence) |
| `-o json` | `-o json` | implemented-verified (pretty-printing matches; schema by-design — register #1) |
| `-o score` | `-o score` | implemented-verified (full parity — register #2 resolved) |
| `-o format` | `--fix` (in-place) / `--fix --dry-run` | implemented-verified (byte parity 8/8) |
| `-o html` | — | not-implemented |
| `-i --in-place` | `--fix` | implemented-verified (renamed) |
| `-V --version` | `-V --version` (root; `u50 0.1.0`) | implemented-verified (version strings differ) |
| `-E --extensions` | root `--status` (per-language backend table) | different-semantics (same 10-extension set verified) |
| `-v --verbose` | `-v --verbose` (+ `-q`, `--log-level`) | implemented-verified |
| `--ignore PATTERN` | — | not-implemented |
| `--clang-format-style STYLE` | — | not-implemented |
| — (no equivalent) | root `--setup` (pre-provision backends into cache) | u50-only, by design |
| — (no equivalent) | `--dry-run`, `--color auto/always/never`, `-q` | u50-only |
| exit 0/1/2 | exit 0/1/2/3 | different-semantics (by design; see §4) |

## Discrepancy register

1. **[by-design (schema) / resolved (pretty-printing)] JSON mode.** style50: indent-4, fields `files[]{name, score, comments, diff(HTML), warn_chars, loc}`, `score`, `version`. u50: fields `clean`, `files[]{path, clean, patch}` — a deliberately leaner schema, kept by design (score/loc/version/HTML diff are not part of u50's contract). Pretty-printing was a [gap] and is **fixed**: u50 now serializes indent-4 like style50. (§5 json)
2. **[resolved] Python score denominator.** style50's `Python.count_lines` counts all lines (PEP8: blank lines matter); u50 counted non-blank lines for every language, so identical diffs (15.0) produced `0.9807692307692307` instead of `0.9814814814814815` on `py/dirty.py`. **Fixed**: `.py` (and only `.py`) now counts all lines in `ScoreRenderer`; non-Python denominators unchanged. (§5 score)
3. **[resolved] Piped ANSI colors.** u50 emitted full color when stdout was not a TTY (`resolve_ansi` had no `is_terminal` gate, `u50_cli/src/main.rs`); style50 emits no color when piped (leaks bare resets only). **Fixed**: `Auto` is now tty-gated; `NO_COLOR`/`--color never` still suppress, `--color always` still forces. (§6)
4. **[by-design] Error reporting channel + exits.** style50 reports per-file problems (`file is empty`, `file not found`, `unknown file type … skipping`) on stdout and always exits 0; u50 reports on stderr (`error: <file>: …`) and exits 3, per its documented 0/1/2/3 scheme. (§3, §4)
5. **[gap] Symlinked directory operands.** style50 follows a symlinked directory argument and formats its contents; u50 fails with `Is a directory (os error 21)`, exit 3. Real directories are walked by both. (§3)
6. **[resolved] Character-mode presentation.** u50 used to show a bare `+/-` diff with no banner/`Looks good!`; it now reproduces style50 3.0.0's `to_ansi` presentation: banner, cyan per-file headers (multi-file runs), re-rendered original text with char-level `on_red`/`on_green` highlighting, `\n`/`\t` markers, legend lines, green `Looks good!` on clean. Remaining divergences (by design/roadmap): the banner is u50-branded (`Results generated by u50 v<version>`), and the comments hint is not implemented. (§5 character)
7. **[by-design] Unified mode is a real patch.** style50's `-o unified` output is not `patch`/`git apply`-able (no `@@`/`---`/`+++`, banner present); u50 emits a standard unified diff. (§5 unified)
8. **[by-design] Missing flags.** `--ignore`, `--clang-format-style`, `-o html`, `-E` (as such) are not implemented; `--status`/`--setup`/`--dry-run`/`--color` are u50 additions. (§7)
9. **[by-design] Version string.** `style50 3.0.0` vs `u50 0.1.0`; the JSON `version` field would similarly differ if/when json mode converges.
