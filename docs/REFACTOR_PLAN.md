# u50_style Modularization Plan

## Current state

```
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

```
u50_style/src/
├── lib.rs                    crate root
├── diff.rs                   CPython difflib port (UNCHANGED — cohesive port)
├── engine.rs                 style-check driver (UNCHANGED)
├── request.rs                Output enum + Request/Report (UNCHANGED)
├── listing.rs                --status table (UNCHANGED)
│
├── language/                 ← was language.rs (21 KB)
│   ├── mod.rs                Language enum, detect_language, missing_tool_message,
│   │                         style50_count_lines, comment_hint, count_comments,
│   │                         COMMENT_MIN, dispatch to per-language counters
│   ├── c.rs                  c_strip_strings, count_c_comments, find_star_slash
│   ├── python.rs             PyTok, PyStringUnit, python_comments,
│   │                         single_quote_end, close_triple, string_start
│   └── javascript.rs         js_strip_strings
│
├── format/                   ← was formatter.rs (20 KB) — formatter abstraction
│   ├── mod.rs                Formatter trait, ToolOrigin, Cs50Formatter dispatch,
│   │                         CS50_CLANG_FORMAT_CONFIG
│   ├── tool.rs               cache_dir, cache_base, venv_bin_dir, tool_file_name,
│   │                         cache_bin_dir, is_executable_file, is_explicit_path,
│   │                         locate_tool, spawn_tool, tool_timeout, run_tool,
│   │                         tool_failure, run_tool_lenient, ensure_backends,
│   │                         ensure_backend
│   ├── c.rs                  C/C++/Java clang-format invocation
│   ├── python.rs             autopep8 invocation
│   ├── javascript.rs         js-beautify invocation
│   ├── html.rs               djhtml invocation (lenient runner)
│   ├── css.rs                css-beautify invocation
│   └── sql.rs                sqlformat invocation + trailing-newline fix-up
│
├── rendering/                ← was render.rs + renderer.rs (49 KB combined)
│   ├── mod.rs                Renderer trait, builtin_renderer, shared types,
│   │                         re-exports, HEADER_RULE
│   ├── palette.rs            crossterm-backed ANSI palette (pinned, red, green, …)
│   ├── console.rs            ConsoleRenderer (character/split/unified output)
│   ├── json.rs               JsonRenderer + json_document + json_pretty
│   ├── html.rs               HtmlRenderer + html_file_chunk + html_document
│   │                         + HTML_STYLE + render_fragment + ws
│   ├── score.rs              ScoreRenderer + py_str_f64
│   ├── character.rs          render_character + push_transition
│   │                         + NEWLINE_MARKER/TAB_MARKER
│   ├── split.rs              render_split + flush_split_rows + fit_column
│   │                         + split_row + SPLIT_WIDTH/TAB_STOP
│   ├── unified.rs            render_unified + patch
│   ├── html_diff.rs          render_html_diff + html_escape + markup_escape
│   │                         + html_transition
│   ├── line_diff.rs          select_algorithm + line_diff + line_diff_score
│   │                         + ALL_IN_ONE_GROUP + trim_line + ADAPTIVE_MIN_LINES
│   │                         + strip_ansi
│   └── doc_flavor.rs         html_escape + markup_escape
│
└── setup/                    (already modularized — unchanged)
```

## Per-language formatter abstraction (item 3)

Currently `Cs50Formatter::format` is a single match statement dispatching to
per-language CLI invocations. The refactored design replaces the match with
a `LanguageFormatter` trait that each language implements:

```rust
/// Per-language formatting: implemented by each language module.
pub(crate) trait LanguageFormatter {
    /// The CLI tool and arguments for this language.
    fn tool_and_args(source: &str, language: Language) -> (String, Vec<String>);
}
```

Actually — the simpler and more idiomatic Rust approach (given that each
"formatter" is just a CLI invocation with fixed args) is to keep the
`Formatter` trait as-is but move the per-language match arms into the
`format/` submodule files as free functions:

```rust
// format/c.rs
pub(crate) fn format(source: &str, language: Language) -> anyhow::Result<String> {
    let assume = format!("--assume-filename={}", language.file_name());
    let style = format!("-style={CS50_CLANG_FORMAT_CONFIG}");
    run_tool("clang-format", &[assume.as_str(), style.as_str()], source)
}
```

And `Cs50Formatter::format` becomes:

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

This achieves 100% modularization: each language's formatting logic is in
its own file, isolated from the others, while the `Formatter` trait and
`Cs50Formatter` dispatch remain in `format/mod.rs`.

## Implementation phases

### Phase 1: language/ module (low risk, mechanical)

1. Create `language/` directory
2. Move C tokenizer to `language/c.rs`
3. Move Python tokenizer to `language/python.rs`
4. Move JS string stripper to `language/javascript.rs`
5. Keep Language enum + detect_language + dispatchers in `language/mod.rs`
6. Fix visibility (pub(crate) on shared items)
7. Fix imports

### Phase 2: format/ module (medium risk — touches the Formatter trait)

1. Create `format/` directory
2. Move tool resolution to `format/tool.rs`
3. Create per-language formatter files with the match-arm bodies
4. Keep `Formatter` trait + `Cs50Formatter` dispatch in `format/mod.rs`
5. Move ensure_backends/ensure_backend to `format/tool.rs`
6. Fix imports + visibility

### Phase 3: rendering/ module (medium risk — touches Renderer trait)

1. Create `rendering/` directory
2. Move palette to `rendering/palette.rs`
3. Move each renderer to its own file
4. Move the Renderer trait + builtin_renderer to `rendering/mod.rs`
5. Move diff renderers (character/split/unified/html_diff) to their own files
6. Fix imports + visibility

### Phase 4: cleanup

1. Delete the old single-file versions
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
