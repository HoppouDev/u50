# u50_style Modularization Plan

## Current state

```text
u50_style/src/
├── lib.rs           606 B   crate root (mod + pub use)
├── diff.rs           37 KB  CPython difflib port (cohesive — NOT split)
├── engine.rs         23 KB  style-check driver (walk, normalize, format, report)
├── formatter.rs      20 KB  Formatter trait + tool resolution + Cs50Formatter
│                            (match dispatch to per-language CLIs)
├── language.rs       21 KB  Language enum + detect + comment counters
│                            (C/JS/Python tokenizers inline)
├── render.rs         23 KB  palette + diff renderers (character/split/unified/html)
├── renderer.rs       26 KB  Renderer trait + Console/Json/Html/Score renderers
├── request.rs       2.5 KB  Output enum + Request/Report types
├── listing.rs       2.8 KB  --status table
├── tests.rs          64 KB  all tests
└── setup/           42 KB   provisioning (already modularized)
```

## Target architecture

The organizing principle: **one module per language**. Everything that is
language-specific — its comment tokenizer, its formatter invocation, and its
formatting configuration — lives in that language's single file, isolated
from every other language and from the shared plumbing. Changing how (say)
Python is tokenized or formatted touches exactly one file.

```text
u50_style/src/
├── lib.rs                    crate root
├── diff.rs                   CPython difflib port (UNCHANGED — cohesive port)
├── engine.rs                 style-check driver (UNCHANGED)
├── request.rs                Output enum + Request/Report (UNCHANGED)
├── listing.rs                --status table (UNCHANGED)
│
├── language/                 ← was language.rs + the per-language arms of formatter.rs
│   ├── mod.rs                Language enum, detect_language, missing_tool_message,
│   │                         style50_count_lines, comment_hint, count_comments,
│   │                         COMMENT_MIN, dispatch to per-language counters
│   ├── c.rs                  C/C++/Java family: count_c_comments, c_strip_strings,
│   │                         find_star_slash, CS50_CLANG_FORMAT_CONFIG,
│   │                         format() → clang-format invocation
│   ├── python.rs             PyTok, PyStringUnit, python_comments,
│   │                         single_quote_end, close_triple, string_start,
│   │                         format() → autopep8 invocation
│   ├── javascript.rs         js_strip_strings,
│   │                         format() → js-beautify invocation
│   ├── html.rs               format() → djhtml invocation (lenient runner)
│   ├── css.rs                 format() → css-beautify invocation
│   └── sql.rs                format() → sqlformat invocation
│                              + trailing-newline fix-up
│
├── format/                   ← was formatter.rs — tool plumbing ONLY, no language logic
│   ├── mod.rs                Formatter trait, ToolOrigin, Cs50Formatter
│   │                         (dispatches to language::<lang>::format)
│   └── tool.rs               cache_dir, cache_base, venv_bin_dir, tool_file_name,
│                             cache_bin_dir, is_executable_file, is_explicit_path,
│                             locate_tool, spawn_tool, tool_timeout, run_tool,
│                             run_tool_lenient, tool_failure, ensure_backends,
│                             ensure_backend
│
├── rendering/                ← was render.rs + renderer.rs (49 KB combined)
│   ├── mod.rs                shared markers (NEWLINE_MARKER/TAB_MARKER),
│   │                         re-exports for engine.rs (Renderer,
│   │                         builtin_renderer, json_document)
│   ├── palette.rs            crossterm-backed ANSI palette (pinned, red, green, …)
│   ├── character.rs          render_character + push_transition + strip_ansi
│   ├── split.rs              render_split + flush_split_rows + fit_column
│   │                         + split_row + SPLIT_WIDTH/TAB_STOP
│   ├── unified.rs            render_unified + patch
│   ├── html_diff.rs          render_html_diff + html_transition
│   ├── line_diff.rs          select_algorithm + line_diff + line_diff_score
│   │                         + ALL_IN_ONE_GROUP + trim_line + ADAPTIVE_MIN_LINES
│   ├── doc_flavor.rs         html_escape + markup_escape
│   └── renderer/             ← was renderer.rs — Renderer-trait-based report
│       │                       renderers, one module per output format
│       ├── mod.rs            Renderer trait + builtin_renderer dispatch
│       ├── console.rs        ConsoleRenderer + HEADER_RULE + file_hints
│       │                     (drives character/split/unified)
│       ├── json.rs           JsonRenderer + json_document + json_pretty
│       ├── html.rs           HtmlRenderer + HtmlFile + HTML_STYLE
│       │                     + render_fragment + ws + html_file_chunk
│       │                     + html_document
│       └── score.rs          ScoreRenderer + py_str_f64
│
└── setup/                    (already modularized — unchanged)
```

### Language-module contract

Every language file exposes the same one-function surface so the dispatch in
`format/mod.rs` stays trivial and each language stays self-contained:

```rust
// language/c.rs — everything C/C++/Java-specific in one file
pub(crate) const CS50_CLANG_FORMAT_CONFIG: &str = "{ ... }";

pub(crate) fn format(source: &str, language: Language) -> anyhow::Result<String> {
    let assume = format!("--assume-filename={}", language.file_name());
    let style = format!("-style={CS50_CLANG_FORMAT_CONFIG}");
    format::tool::run_tool("clang-format", &[assume.as_str(), style.as_str()], source)
}
```

```rust
// language/html.rs — no tokenizer, formatter only
pub(crate) fn format(source: &str, _language: Language) -> anyhow::Result<String> {
    format::tool::run_tool_lenient("djhtml", &["-"], source)
}
```

`Cs50Formatter::format` in `format/mod.rs` keeps the empty-source short-circuit
and lazy auto-provisioning, then delegates per language:

```rust
fn format(&self, source: &str, language: Language) -> anyhow::Result<String> {
    if source.trim().is_empty() { return Ok(source.to_owned()); }
    // lazy auto-provisioning (unchanged)
    ...
    match language {
        Language::C | Language::Cpp | Language::Java => c::format(source, language),
        Language::Python => python::format(source, language),
        Language::JavaScript => javascript::format(source, language),
        Language::Html => html::format(source, language),
        Language::Css => css::format(source, language),
        Language::Sql => sql::format(source, language),
    }
}
```

### Why this shape

- **Isolation**: each language's tokenizer, config, and CLI invocation sit in
  one file. The C-family config (`CS50_CLANG_FORMAT_CONFIG`) moves out of the
  shared formatter into `language/c.rs`, where it belongs.
- **No language logic in `format/`**: that module is pure plumbing — tool
  resolution, cache paths, venv provisioning, process spawning, timeouts.
  It never needs to change when a language changes.
- **Dependency direction**: `language/*` → `format/tool` (one-way). No
  language module imports another, and `format/mod.rs` only imports the
  language modules' `format` functions.
- **Uniform surface**: one `format(source, language)` per language file —
  simpler than a `LanguageFormatter` trait with one impl per language, since
  each formatter is just a fixed CLI invocation.

### Renderer-module contract

Same principle as `language/`: one module per renderer, each owning
everything specific to it, with shared plumbing extracted — not duplicated.

- **Report renderers** live in `rendering/renderer/` — one module per
  `Output`, each owning its `Renderer` impl plus every helper only it uses,
  including its output serialization:

```rust
// rendering/renderer/score.rs — score-specific float formatting lives with
// its renderer
pub(crate) fn py_str_f64(value: f64) -> String { ... }
```

```rust
// rendering/renderer/json.rs — the whole JSON output path is one module
pub(crate) fn json_document(report: &Report) -> serde_json::Value { ... }
pub(crate) fn json_pretty(document: &serde_json::Value) -> Vec<u8> { ... }
```

`renderer/mod.rs` holds the `Renderer` trait and `builtin_renderer`
(the `Output`-based constructor); nothing else is shared between them.

- **Diff renderers** (`character`/`split`/`unified`/`html_diff`) are free
  functions with a uniform `render_*` surface, consumed by `ConsoleRenderer`
  (character/split/unified, dispatched on `Output`) and `HtmlRenderer`
  (html_diff, for the diff embedded in HTML reports).
- **Shared plumbing** — used by two or more renderers — gets its own file:
  - `palette.rs` — ANSI colors (character, split, html_diff, console)
  - `line_diff.rs` — difflib diff + algorithm choice (split, unified, score)
  - `doc_flavor.rs` — `html_escape` (html_diff) + `markup_escape` (html)
  - `mod.rs` — `NEWLINE_MARKER`/`TAB_MARKER` shared by character + html_diff
- **Dependency direction** (one-way): `rendering/renderer/*` → the
  `rendering/`-level helpers: `console` → `character`/`split`/`unified`
  → `palette` + `line_diff` + the `mod.rs` markers; `html` →
  `html_diff` + `doc_flavor`; no renderer imports a sibling renderer.
  `engine.rs` touches only the `rendering/` re-exports (`Renderer`,
  `builtin_renderer`, `json_document`) — the `renderer/` submodule is
  invisible from outside `rendering/`.

Corrections to the earlier draft, verified against the current source:

- `strip_ansi` is character-specific (only `render_character` calls it) —
  it was mis-listed under `line_diff.rs`; it belongs in `character.rs`
- `html_escape`/`markup_escape` were listed twice (`html_diff.rs` and
  `doc_flavor.rs`); they live once, in `doc_flavor.rs`
- `HEADER_RULE` is console-specific → `renderer/console.rs`, not `mod.rs`
- `py_str_f64` is score-specific → `renderer/score.rs`
- the `Renderer` trait + `builtin_renderer` belong to the report-renderer
  abstraction → `renderer/mod.rs`, not `rendering/mod.rs` (which keeps
  only the shared markers and the external re-exports)

## Implementation phases

### Phase 1: format/ tool plumbing (low risk, mechanical)

1. Create `format/` directory
2. Move all tool resolution/provisioning out of `formatter.rs` into
   `format/tool.rs` (cache paths, locate/spawn/run, timeouts, failures,
   ensure_backends/ensure_backend)
3. Widen visibility to `pub(crate)` on the pieces language modules need:
   `run_tool`, `run_tool_lenient`
4. Keep `Formatter` trait + `ToolOrigin` + `Cs50Formatter` in `format/mod.rs`
   (per-language match arms still inline at this point)
5. Fix imports

### Phase 2: language/ module (medium risk — moves formatter match arms)

1. Create `language/` directory
2. Move C tokenizers + `CS50_CLANG_FORMAT_CONFIG` + the clang-format arm to
   `language/c.rs`
3. Move Python tokenizer + the autopep8 arm to `language/python.rs`
4. Move JS string stripper + the js-beautify arm to `language/javascript.rs`
5. Create `language/html.rs`, `language/css.rs`, `language/sql.rs` from the
   remaining match arms (sql keeps its trailing-newline fix-up)
6. Each file exposes `pub(crate) fn format(source, language)`; tokenizers
   stay `pub(crate)` for the `language/mod.rs` counters dispatch
7. Keep Language enum + detect_language + counter dispatch in `language/mod.rs`
8. Replace the inline match arms in `Cs50Formatter::format` with delegation
   to the language modules
9. Fix imports + visibility

### Phase 3: rendering/ module (medium risk — touches Renderer trait)

1. Create `rendering/` directory
2. Move palette to `rendering/palette.rs`
3. Move shared plumbing: `line_diff.rs` (select_algorithm, line_diff,
   line_diff_score, trim_line, ALL_IN_ONE_GROUP, ADAPTIVE_MIN_LINES) and
   `doc_flavor.rs` (html_escape, markup_escape)
4. Move diff renderers: `character.rs` (incl. push_transition + strip_ansi),
   `split.rs`, `unified.rs` (incl. patch), `html_diff.rs` (incl.
   html_transition); keep NEWLINE_MARKER/TAB_MARKER in `rendering/mod.rs`
5. Create `rendering/renderer/`: move the Renderer trait +
   builtin_renderer to `renderer/mod.rs`, then one file per report
   renderer with ALL of its specific logic — `console.rs` (incl.
   HEADER_RULE + file_hints), `json.rs` (incl. json_document +
   json_pretty), `html.rs` (incl. HtmlFile, HTML_STYLE, render_fragment,
   ws, html_file_chunk, html_document), `score.rs` (incl. py_str_f64)
6. Keep `rendering/mod.rs` for shared markers (NEWLINE_MARKER/TAB_MARKER)
   and re-exports for engine.rs (Renderer, builtin_renderer, json_document)
7. Fix imports + visibility

### Phase 4: cleanup

1. Delete the old single-file versions (`language.rs`, `formatter.rs`,
   `render.rs`, `renderer.rs`)
2. `cargo fmt --all`
3. Full gate ladder
4. Commit + push + CI

## What is NOT split

- `diff.rs` — the CPython difflib port; cohesion is intentional
- `engine.rs` — already focused (walk + normalize + format + report)
- `request.rs` — already tiny (2.5 KB)
- `listing.rs` — already tiny (2.8 KB)
- `tests.rs` — the unified_request helper collapsed the worst duplication;
  further splitting is cosmetic

## Verification checklist

- [ ] cargo build --workspace — zero errors
- [ ] cargo fmt --all -- --check — clean
- [ ] cargo clippy --workspace --all-targets -- -Dwarnings — clean
- [ ] cargo test --workspace — all pass (101 lib + 26 cli + goldens)
- [ ] Character output byte-parity on dirty.c (12× perf target preserved)
- [ ] HTML output byte-parity on all 8 fixtures
- [ ] CI green (both OS legs)
- [ ] No language-specific logic remains in `format/` (grep for tool names
      and configs outside `language/`)
- [ ] Each `language/<lang>.rs` compiles without importing any sibling
      language module (isolation check)
- [ ] No renderer-specific logic remains in `rendering/mod.rs` (only
      shared markers + re-exports) or in `rendering/renderer/mod.rs`
      (only the trait + builtin_renderer)
- [ ] No `rendering/*.rs` imports a sibling renderer module — shared
      plumbing only (isolation check)
- [ ] No `rendering/renderer/<x>.rs` imports a sibling report renderer
      (isolation check)
- [ ] `engine.rs` references only the `rendering/` re-exports and never
      `rendering::renderer::…` paths directly
