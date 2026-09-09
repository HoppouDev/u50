# u50_style Plugin Architecture Plan

## Goal

Make every language and every renderer **100% modular**: a language or
renderer is one self-contained module that implements a small plugin
trait and **registers itself** into a central registry — the registration
model minekube/gate uses for its proxy plugins (a plugin implements an
interface, is registered via `gate.AddPlugin(...)` before startup, and
the host dispatches through its registry; the host code never names
individual plugins). After this refactor, adding a language or a
renderer is **one new module + one registration line**; no core file
(match dispatch, enums, listing logic) changes.

Non-goals: no dynamic loading of external binaries/crates, no runtime
plugin discovery, no async — plugins are compiled in and registered
explicitly (gate's _embedded_ plugin model, where a host binary calls
`AddPlugin` itself). Byte-parity with style50 output is unaffected: the
registry changes _who_ is called, not _what_ is emitted.

## Current state (post module split)

The module split (see `REFACTOR_PLAN.md`) already isolates code per
language and per renderer. What remains hardcoded is the **dispatch**:

| Hardcoded dispatch today                                                                                     | Where                               |
| ------------------------------------------------------------------------------------------------------------ | ----------------------------------- |
| `Language` enum + `match` arms for `required_tool`, `pip_package`, `display_name`, `extensions`, `file_name` | `language/mod.rs`                   |
| `detect_language` extension `match`                                                                          | `language/mod.rs`                   |
| `count_comments` / `style50_count_lines` `match` to per-language counters                                    | `language/mod.rs`                   |
| `Cs50Formatter::format` `match` to `language::<lang>::format`                                                | `format/mod.rs`                     |
| `builtin_renderer` `match` on `Output` to the four renderer structs                                          | `rendering/renderer/mod.rs`         |
| `Language::ALL` array = listing order                                                                        | `language/mod.rs`                   |
| `tool_search_scope` / `ensure_backend` tool lookups                                                          | `language/mod.rs`, `format/tool.rs` |

Adding a language today = new module **+ touching `language/mod.rs` five
times + `format/mod.rs` + tests**. The registry removes every one of
those touch points.

## Target architecture

```text
u50_style/src/
├── registry.rs               the two registries + registration lists
│                             (the gate.AddPlugin analog — the ONLY file
│                             that names all plugins)
│
├── language/                 one module per language, each a plugin
│   ├── mod.rs                trait LanguagePlugin + shared hint
│   │                         arithmetic (COMMENT_MIN, comment_hint) +
│   │                         registry accessors (detect, get)
│   ├── c.rs                  CPlugin: metadata + counter + format
│   ├── python.rs             PythonPlugin
│   ├── ...                   (one plugin struct per language)
│   └── rust.rs               RustPlugin (+ toolchain resolution hook)
│
├── format/                   plumbing (unchanged shape)
│   ├── mod.rs                Formatter trait; Cs50Formatter becomes
│   │                         registry-driven (no match)
│   └── tool.rs               spawn/provision/cache (unchanged)
│
├── rendering/                shared rendering primitives (palette,
│   │                         line_diff, doc_flavor, character/split/
│   │                         unified/html_diff — unchanged)
│   └── renderer/
│       ├── mod.rs            trait RendererPlugin + registry accessor
│       │                     + Renderer trait (unchanged)
│       ├── console.rs        ConsolePlugin (serves character/split/
│       │                     unified via the shared diff renderers)
│       ├── json.rs           JsonPlugin + JsonRenderer
│       ├── html.rs           HtmlPlugin + HtmlRenderer
│       └── score.rs          ScorePlugin + ScoreRenderer
│
└── engine.rs, request.rs, listing.rs, diff.rs   (consume the registries)
```

### LanguagePlugin trait (`language/mod.rs`)

```rust
/// One language, fully self-contained. Implemented by a zero-sized
/// struct in the language's own module and registered in
/// `registry::languages()`.
pub(crate) trait LanguagePlugin: Sync {
    /// Stable machine id (`"rust"`), also the registry lookup key.
    fn id(&self) -> &'static str;
    /// Human-readable name for `--status` / listings (`"Rust"`).
    fn display_name(&self) -> &'static str;
    /// File extensions detected for this language.
    fn extensions(&self) -> &'static [&'static str];
    /// The backing binary; every language has exactly one.
    fn required_tool(&self) -> &'static str;
    /// The pip package provisioning the tool, or `None` when it cannot
    /// be pip-provisioned (rustfmt resolves from the Rust toolchain
    /// instead and is never auto-provisioned).
    fn pip_package(&self) -> Option<&'static str> { None }
    /// Canonical file name passed to tools that lex by filename
    /// (clang-format's `--assume-filename`); `None` = not applicable.
    fn assume_filename(&self) -> Option<&'static str> { None }
    /// Comment counter mirroring style50's per-language
    /// `count_comments`; `None` = never comment-hinted (HTML/CSS/SQL).
    fn count_comments(&self, code: &str) -> Option<u32> { None }
    /// The style-check line-count rule (Python counts all lines, per
    /// PEP 8; the default counts non-blank lines only).
    fn count_lines(&self, code: &str) -> usize {
        code.lines().filter(|l| !l.trim().is_empty()).count()
    }
    /// Formats normalized source with this language's backend.
    ///
    /// # Errors
    /// Returns an error when the backend is missing or fails.
    fn format(&self, source: &str) -> anyhow::Result<String>;
    /// Optional fallback resolution for tools that cannot be located
    /// cache-only (rustfmt -> the Rust toolchain). `None` = cache-only.
    fn resolve_tool(&self, tool: &str) -> Option<PathBuf> { None }
    /// Where the tool is searched, for the not-found error message.
    fn tool_search_scope(&self) -> &'static str { "the u50 style cache" }
}
```

A plugin struct per language module (zero-sized, `pub(crate) static
PLUGIN: CPlugin = CPlugin;`), so the module owns its metadata, its
tokenizer/counter, and its formatter invocation together:

```rust
// language/c.rs
pub(crate) static PLUGIN: CPlugin = CPlugin;
impl LanguagePlugin for CPlugin {
    fn id(&self) -> &'static str { "c" }
    fn extensions(&self) -> &'static [&'static str] { &["c", "h"] }
    fn required_tool(&self) -> &'static str { "clang-format" }
    fn pip_package(&self) -> Option<&'static str> { Some("clang-format") }
    fn assume_filename(&self) -> Option<&'static str> { Some("foo.c") }
    fn count_comments(&self, code: &str) -> Option<u32> {
        Some(count_c_comments(&c_strip_strings(code)))
    }
    fn format(&self, source: &str) -> anyhow::Result<String> { /* clang-format */ }
}
```

Note: `Cpp`/`Java` become plugins too (they share C's counter and
clang-format backend via `language/c.rs` helpers — one family, three
plugin structs, zero shared mutable state).

### RendererPlugin trait (`rendering/renderer/mod.rs`)

```rust
/// One output format, self-contained: builds the [`Renderer`] serving
/// its `Output`. Registered in `registry::renderers()`.
pub(crate) trait RendererPlugin: Sync {
    /// The output format this plugin serves (one plugin per `Output`,
    /// except console which serves the three text modes).
    fn outputs(&self) -> &'static [Output];
    /// Diagnostics name (`"console"`, `"json"`, ...).
    fn name(&self) -> &'static str;
    /// Builds the renderer for one run.
    fn create(&self, output: Output, color: bool, out: Box<dyn Write>)
        -> Box<dyn Renderer>;
}
```

`builtin_renderer` becomes a registry lookup (no match):

```rust
pub fn builtin_renderer(output: Output, color: bool, out: Box<dyn Write>)
    -> Box<dyn Renderer>
{
    renderers()
        .iter()
        .find(|plugin| plugin.outputs().contains(&output))
        .unwrap_or_else(|| panic!("no renderer plugin for {output:?}"))
        .create(output, color, out)
}
```

### Registration (`registry.rs`) — the gate analog

```rust
/// The compiled-in language plugins, in listing order (`--status`, the
/// extension-detection precedence and `Language::ALL` all derive from
/// this list). Registering a plugin here is the ONLY core change adding
/// a language requires — the gate.AddPlugin analog.
pub(crate) fn languages() -> &'static [&'static dyn LanguagePlugin] {
    &[
        &c::PLUGIN,
        &cpp::PLUGIN,
        &java::PLUGIN,
        &python::PLUGIN,
        &javascript::PLUGIN,
        &html::PLUGIN,
        &css::PLUGIN,
        &sql::PLUGIN,
        &rust::PLUGIN,
    ]
}

/// The compiled-in renderer plugins.
pub(crate) fn renderers() -> &'static [&'static dyn RendererPlugin] {
    &[&console::PLUGIN, &json::PLUGIN, &html::PLUGIN, &score::PLUGIN]
}
```

Why an explicit list (gate's model) instead of the `inventory`/`linkme`
auto-registration crates: ordered listing is a user-visible contract,
static typing stays fully checkable, no linker sections or ctor magic,
and the registry is greppable — exactly gate's embedded-plugin story.
Duplicate `id`/`extension` collisions are caught by a debug assertion +
unit test walking the registry (mirrors today's `ALL` uniqueness test).

### What happens to `Language`

The enum is **retired as a dispatch mechanism** and reduced to a cheap
handle: `Language` becomes `#[derive(Copy, Clone)] pub struct
Language(&'static dyn LanguagePlugin)` with `PartialEq` by `id()`.
Everything that matched on variants becomes a method call through the
handle — `detect_language` finds the plugin by extension and returns the
handle; `engine`, `renderers`, and `listing` carry the handle and call
methods; **no core file names a specific language again**. Public API
stays source-compatible for u50_cli (`Language` still exists,
`detect_language` still returns it); `required_tool`/`pip_package` move
from inherent methods to plugin methods behind the handle.

### Call-site migration map

| Today                                   | After                                                                           |
| --------------------------------------- | ------------------------------------------------------------------------------- |
| `detect_language` match                 | iterate `registry::languages()` extensions                                      |
| `language.required_tool()` match        | `plugin.required_tool()`                                                        |
| `language.pip_package()` match          | `plugin.pip_package()` (default impl)                                           |
| `count_comments` match                  | `plugin.count_comments(code)`                                                   |
| `style50_count_lines` match             | `plugin.count_lines(code)`                                                      |
| `Cs50Formatter::format` match           | `plugin.format(source)` (after empty-source + provisioning, which stay in core) |
| `tool_search_scope` string match        | `plugin.tool_search_scope()` (default impl)                                     |
| `locate_tool` -> `rust::toolchain_tool` | `plugin.resolve_tool(tool)`                                                     |
| `builtin_renderer` match                | registry lookup                                                                 |
| `Language::ALL`                         | `registry::languages()`                                                         |

## Dependency direction (unchanged rule, stronger form)

`plugins -> core plumbing` only. `language/*` plugins use
`format::tool::{run_tool, run_tool_lenient}`; renderer plugins use the
shared `rendering` primitives. The registries are the single place that
knows every plugin; core files (engine, listing, format, request) import
only the trait + registry accessors and never a concrete plugin.

## Implementation phases

### Phase 1: language plugin trait + registry (largest, mechanical)

1. Add `language/mod.rs::LanguagePlugin` (trait above) and
   `registry.rs` with the language list; `Language` becomes the plugin
   handle (enum variants deleted in the same commit — the compiler
   enumerates every call site)
2. Migrate the 9 languages: each module gains its `PLUGIN` struct;
   `c.rs` hosts the shared C-family helpers used by the C/Cpp/Java
   plugins
3. Rewire `detect_language`, `count_comments`/`count_lines` callers,
   `Cs50Formatter`, `tool_search_scope`, `ensure_backend`, `locate_tool`
   to the registry
4. Move `missing_tool_message` into per-plugin defaults? No — it stays a
   core table keyed by tool name (it is tool UX, not language logic)
5. Update tests: registry-walk tests replace the per-variant tables
   (`required_tool_maps_every_language` -> registry assertions)

### Phase 2: renderer plugin trait + registry

1. Add `RendererPlugin` + `registry::renderers()`; migrate the four
   renderer modules to `PLUGIN` structs
2. `builtin_renderer` becomes the registry lookup; `Output` stays the
   CLI-facing enum
3. `engine.rs`'s direct `json_document` use is decided: keep it a shared
   schema builder re-exported from `rendering` (it is output-mode
   independent plumbing for `fix --dry-run`)

### Phase 3: cleanup

1. Delete retired match arms and the `Language::ALL` array
2. `cargo fmt`, full gate ladder, goldens byte-parity
3. Update `AGENTS.md` (root + crate) and this plan's checklist

## Performance invariants

- Registry lookups happen **once per file** (language) and **once per
  run** (renderer), never inside the per-character/per-line hot loops
- Plugin methods are static dispatch through `&'static dyn` where the
  vtable cost is one call per file — negligible against process spawn
- `diff.rs`, the ndiff walk, and the renderer hot paths are untouched

## What is NOT changing

- `diff.rs` (CPython difflib port), `request.rs`, the engine's walk and
  report model, the CLI surface and exit codes
- The goldens: every output byte must be identical; the golden suite is
  the acceptance harness for each phase
- The shared rendering primitives (palette, line_diff, doc_flavor, the
  diff renderers) — they are the plugins' toolkit, not plugins
- Cache/toolchain resolution semantics, provisioning, exit codes

## Verification checklist

- [ ] `cargo test --workspace` — all pass (registry uniqueness + mapping tests included)
- [ ] `U50_STYLE_GOLDEN=1 cargo test -p u50_style --test golden` — byte-parity
- [ ] `cargo clippy --workspace --all-targets -- -Dwarnings` — clean
- [ ] Adding a throwaway test language = 1 module + 1 registry line (spike commit, then reverted)
- [ ] `grep Language::` in engine.rs/listing.rs/rendering returns only the handle type, no variant names
- [ ] Registry debug-assert rejects duplicate ids and duplicate extensions
- [ ] `--status` output byte-identical (order = registry order)
- [ ] CI green (both OS legs)
