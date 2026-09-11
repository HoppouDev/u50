# u50_check Python Checks Plan (embedded RustPython)

Runs the legacy Python check files — the `__init__.py` / `check.py` authoring
model that the real check sets in [cs50/problems](https://github.com/cs50/problems)
actually use — inside `u50_check`, by embedding [RustPython](https://github.com/RustPython/RustPython)
and exposing a native `check50` module that bridges to the existing Rust check API.
This closes the port's largest documented divergence: today
`u50_check/src/lib.rs` bails with _"no registered check set matches … (Python
checks are not executable)"_ for the model that most shipped check sets use
(see `docs/CHECK50_PORT_NOTES.md` §7.1 and `docs/U50_CHECK_PLUGIN_PLAN.md`).

## Goal and non-goals

**Goal**: a check package whose `.cs50.yaml` says `checks: __init__.py` (or any
Python checks file) runs its `@check50.check()` functions through the same
engine — declaration order, dependency graph, filesystem inheritance,
timeouts, skip cascade, ansi/json rendering — that YAML "simple" checks and
native Rust plugins already use (per `docs/U50_CHECK_PLUGIN_PLAN.md`).

**Goal (this revision)**: the three capabilities previously listed as
divergences — pip `dependencies:`, `check50.flask`, and `check50.c.valgrind` —
are now **planned phases** (6-8 below). They land on shared foundations,
not style-local or check-local code:

- **`u50_tools`** (Phase 5): the uv provisioning pipeline, the tool
  resolver **plugin system**, and the plugin scaffolding **extracted from
  `u50_style` into a shared workspace crate** — every tool declares _which_
  resolver plugin provisions it (`uv`, `toolchain`, `system`, binary
  `download`, or one added later), so provisioning mechanisms are
  application-wide, pluggable assets rather than style50 internals.
- **Capability plugins** (Phases 7-8): flask and valgrind become **their own
  plugins** — self-contained modules registered in a capability registry,
  each declaring its tool/dependency needs (uv-provisioned stack vs
  resolver-discovered binary) and exposing its surface to _both_ the Python
  bridge (`check50.flask`, `check50.c.valgrind`) and the native Rust
  check API. Adding a future capability is one module + one registration
  line — the same rule the language and check-set registries follow.

**Non-goals (permanent divergences):**

- gettext translations.
- Remote mode (`lib50.push`, submit.cs50.io polling).
- `import_checks` across GitHub repos (local sibling paths only).
- C-extension packages under the embedded interpreter (RustPython has no
  CPython C API) — declared/filtered with clear errors, never silently wrong.

## Why embedded RustPython (alternatives considered)

| Option                        | Verdict                                                                                                                                                                                                                         |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **RustPython (embedded)**     | Pure Rust, no system Python, no libpython. Aligns with u50's self-contained principle (cf. style50's in-process uv provisioning). Crates: `rustpython-vm` / `rustpython-derive` (stdlib via `rustpython-pylib`/freeze feature). |
| Subprocess `python3`          | Violates the workspace principle that resolution never consults `PATH` (see `u50_style/AGENTS.md` tool management) and breaks the "one binary" promise. Documented as a fallback escape hatch only.                             |
| PyO3 + CPython                | Requires a libpython at runtime — not self-contained.                                                                                                                                                                           |
| Transpile check files to Rust | The authoring API is dynamic (decorators, closures, arbitrary Python) — not transpilation territory.                                                                                                                            |

Trust model: check files come from cs50's GitHub orgs, the same trust level as
the compiled-in native plugins. Embedding is _not_ a sandbox promise; the
timeout/deadline machinery (below) is about hangs, not malice.

## What must be emulated (from `docs/CHECK50_PORT_NOTES.md`)

The Python check surface that shipped `__init__.py` files actually touch:

1. **`@check50.check(dependency=None, *, timeout=..., hidden=...)`** — appends
   the function to a module-global registry in **declaration order**; the
   function's **docstring is the user-visible description**; the first
   positional arg is the dependency (function or name).
2. **Return-value passing** — a passing dependency's **return value is passed
   as the first argument of its dependents** (check50's runner submits
   dependents "with the dependency's returned state"). The Rust engine
   currently only inherits the dependency's _filesystem_ (`run_dir`); this is
   a required scheduler extension (Phase 3).
3. **Core API** — `run(cmd, env)` returning the chainable
   `stdin`/`stdout`/`reject`/`exit`/`kill` builder; `exists`, `include`,
   `log`, `data`, `hash`; the `Failure`/`Mismatch`/`Missing` exceptions;
   `EOF` sentinel; `regex.decimal`; `hidden(rationale)`.
4. **`check50.c.compile(file, ...)`** — clang invocation (the runner already
   spawns child processes; compile is just another spawn).
5. **`import_checks("../less")`** — import another check module from a
   sibling directory so check sets can extend each other.
6. **`check50.py`** helpers (`append_code`, `import_`, `compile`) — thin;
   implement where cheap, else document.
7. **`check50.flask`** and **`check50.c.valgrind`** — now planned as
   capability plugins (Phases 7 and 8).

## Target architecture

```text
Cargo.toml                    [workspace.dependencies] gains rustpython-vm
                              (pinned, optional); style50's uv-* crates are
                              already pinned workspace-wide
u50_tools/                    NEW shared crate — application-wide, not
                              style-specific (Phase 5):
├── uv/                       the provisioning pipeline, extracted from
│                             u50_style/src/setup (pipeline.rs, venv.rs,
│                             wheels.rs, pins.rs): uv-managed CPython, venv
│                             at a fixed cache path, PyPI wheel fetch with
│                             platform-tag ranking, pinned versions,
│                             parallel downloads with stderr spinners,
│                             cross-process advisory lock — in-process,
│                             no pip/python binaries, no PATH
├── resolver/                 the resolver plugin system: a ResolverPlugin
│                             trait + resolver registry — every tool
│                             declares which resolver provisions it;
│                             plugins: uv (the extracted pipeline, for pip
│                             packages), toolchain (rustfmt from the Rust
│                             toolchain), system (discover-only, e.g.
│                             valgrind), download (pinned standalone
│                             binaries + SHA-256, platform-mapped); cache
│                             paths parameterized by domain (`u50/style50`,
│                             `u50/check50`)
├── registry.rs               plugin-scaffolding helpers shared by every
│                             registry (unique-id/name validation, path-safe
│                             names, declaration-order iteration); the
│                             domain traits stay in their crates
└── fs.rs / proc.rs           shared OS plumbing: symlink-safe recursive
                              copy, process-group spawn/kill (from
                              u50_check/src/api.rs), shell resolution
                              (bash_command), execute-bit handling
u50_check/src/
├── capabilities/              NEW capability-plugin layer (see below)
│   ├── mod.rs                 CapabilityPlugin trait + registry (one module
│                             + one registration line per capability)
│   ├── flask.rs               FlaskCapability (Uv strategy: pinned Flask
│                             stack venv, lazily provisioned once)
│   └── valgrind.rs            ValgrindCapability (System strategy:
│                             discover-only, skip-with-guidance when
│                             absent)
├── python.rs                  feature-gated module root (mod python when built)
│   ├── interp.rs              interpreter lifecycle: one VM per run, warm pool
│                             of scopes, sys.path wiring (check dir, run dir,
│                             provisioned site-packages)
│   ├── api.rs                 the injected `check50` package: run/stdin/stdout/
│                             exit/kill, exists/include/log/data/hash, EOF,
│                             Failure/Mismatch/Missing exceptions — each call
│                             bridges to the Rust `CheckContext`/`Run` API;
│                             submodule namespaces (check50.c/check50.flask)
│                             delegate to registered capabilities
│   ├── registry.rs            `@check50.check` decorator: records (name,
│                             docstring, dependency, timeout, hidden) into
│                             the module registry, in declaration order
│   ├── c.rs                   check50.c.compile (spawn clang); valgrind
│                             bridges to ValgrindCapability
│   ├── flask.rs               check50.flask bridges to FlaskCapability
│   └── deps.rs                pip `dependencies:` provisioning via u50_tools
│                             (Phase 6)
├── plugin.rs                  new RunKind::Python { module, check } variant
└── runner.rs                  per-thread VM scope handling; expired-flag
                              checks inside Python API calls
```

**Compile feature**: `u50_check/Cargo.toml` gains
`rustpython-vm = { workspace = true, optional = true }` +
`[features] python = ["dep:rustpython-vm"]`; the workspace pins the exact
version (like the uv crates). Default builds (and the binary's size budget)
are unchanged unless `--features python`. The CLI surface does not change:
`u50 check <pkg> --mode local/offline/dev` just stops bailing when
`.cs50.yaml` names a Python checks file — and reports a clean error
("rebuild with `--features python`") when the feature is off.

## Execution and isolation model

- **One interpreter, one scope per check.** check50 gives each check a fresh
  process; the embedded equivalent is a fresh module namespace (a rustpython
  scope) per check, executing the _same pre-parsed module code_ — import once,
  call once per check. The runner's existing per-check thread stays; the
  expired flag is polled at every `check50.*` binding call, so a check blocked
  in our `Run` waits still aborts on deadline.
- **Pure-Python infinite loops cannot be interrupted** mid-bytecode today.
  Mitigations, in order: (1) rustpython's instruction-count hook / trace
  callback if available in the pinned version, else (2) document the
  limitation and rely on the fact that real check sets spend their time in
  `check50.run(...)` spawns, which the existing child-process kill already
  bounds. Record the outcome of the Phase 0 spike here.
- **State**: the dependency graph stays in Rust. Python checks are bridged as
  `RunKind::Python` specs; pass/fail/skip semantics, log capture (the
  `check50.log` binding appends to the check's existing log Arc), and the
  skip cascade are unchanged.
- **Stderr/stdout of the _interpreter_** never leaks into rendered output;
  only check API calls produce log lines (check50 parity).

## The capability-plugin layer

flask and valgrind are not check sets and not languages — they are
**capabilities**: optional API surfaces a check set may reach for, each with
its own tool/dependency needs. They become plugins under
`u50_check/src/capabilities/`, following the same registry pattern as
`LanguagePlugin` (style50) and `CheckSetPlugin` (check50):

```rust
pub trait CapabilityPlugin: Sync {
    /// Stable id ("flask", "valgrind"); also the registered Python
    /// submodule surface it backs.
    fn id(&self) -> &'static str;
    /// Availability through u50_tools: the capability's tools are resolved
/// by their declared resolver plugins (flask -> the `uv` resolver: is
/// the pinned stack provisioned? valgrind -> the `system` resolver:
/// discovered binary?). Reported by `u50 --status`.
    fn availability(&self) -> Availability;   // Available | NeedsProvision | Missing { guidance }

    fn availability(&self) -> Availability;   // Available | NeedsProvision | Missing { guidance }
    /// Provision when supported by the strategy (flask: yes, once per
    /// machine; valgrind: never — prints guidance instead).
    fn ensure(&self) -> anyhow::Result<()>;
    /// The native Rust surface (usable from native check sets, not just
    /// Python checks) — e.g. `ctx.flask().get(...)`, `ctx.c_valgrind(..)`.
    /// The Python bridge (`check50.flask`, `check50.c.valgrind`) binds to
    /// the same objects, so there is exactly one implementation.
    // (per-capability concrete methods; not part of the shared trait)
}
```

Consequences of the shape:

- **One implementation, two surfaces**: the Python bindings in
  `python/flask.rs` / `python/c.rs` are thin bridges; the logic lives in the
  capability module. Native Rust check sets can use the same capability
  without Python.
- **Provisioning is declarative**: each capability names its strategy and
  package/binary needs; `u50 --status` reports every capability's
  availability, `u50 --setup` bulk-provisions the provisionable ones (and
  prints guidance for the discover-only ones) — one table, all domains.
- **Skip, never bail**: an unavailable capability yields check-level skips
  with guidance (check50 `Cause`-shaped), never a whole-run failure.
- **Extensibility**: a future capability (e.g. `check50.py` extras, or a
  clang-tidy helper) is one module + one registration line, provisioned by
  an existing strategy.

## Phases

### Phase 0 — Spike (feasibility + budget)

- Add `rustpython-vm` behind the `python` feature; boot a VM; exec a module
  that imports a native Rust extension module and defines a decorated
  function; call it; raise/catch a custom exception across the boundary.
- Measure: build time delta (debug + release `opt-level="z"`), binary size
  delta, VM boot latency, MSRV compatibility (`msrv = "1.96"` in clippy.toml).
- Gate: both OS legs compile; boot+exec < 50 ms cold; size delta documented.
  If rustpython is unusable at this pin, stop and record the fallback plan
  (subprocess python3, PATH-free via explicit discovery like rustfmt's
  toolchain resolution).

### Phase 1 — The `check50` module surface

- `python/api.rs`: native module + exceptions (`Failure`, `Mismatch` with the
  repr-truncated `expected`/`actual` payload, `Missing`), `EOF`, and the
  chainable run builder object (methods delegate to `CheckContext::run` and
  the existing `Run` API — prompt absorption, regex/exact matching, reject,
  exit-code assert, SIGSEGV semantics come for free).
- `exists`, `include`, `log`, `data`, `hash` (streaming), all resolving via
  `CheckContext::resolve` (run_dir containment, per the review fixes).
- Unit tests drive the API from Python fixtures (`#[cfg(feature = "python")]`):
  echo/stdin prompt, EOF, reject, mismatch payload, Failure propagation.
- Gate: fixture parity with the equivalent YAML-check expectations already
  in `cross_check.rs`.

### Phase 2 — Decorator bridge (the end-to-end pipeline)

- `python/registry.rs`: the decorator records the registry; `lib.rs` routes
  `checks: <file>.py` to: read file → exec module once → build
  `Vec<CheckSpec>` in declaration order (docstring → description;
  dependency fn → name; `timeout=`, `hidden=` kwargs honored) → existing
  runner. No new rendering work.
- New `RunKind::Python { module_id, check_name }` in `plugin.rs`; the
  runner's per-check thread creates the check's scope and calls the
  function; raised `check50.Failure` → `Failure`, other Python exceptions →
  `Cause::Error` (panic-path parity).
- Gate: a hello-world `__init__.py` (exists → compiles → prints hello, the
  canonical cs50 example) produces the same results JSON shape as the
  YAML/native equivalents.

### Phase 3 — Parity work the legacy checks actually need

- **Dependency return-value passing**: the scheduler stores each check's
  return value keyed by name (`CheckState` extension); Python dependents
  receive it as their first argument. Native Rust checks get the same
  capability (an API evolution, not Python-only — real cs50 chains pass
  state this way).
- `check50.c.compile`: clang via the existing spawn machinery
  (`bash_command`-style resolution; PATH-free clang discovery like
  rustfmt's).
- `check50.regex.decimal` → the existing `decimal_regex()`.
- `import_checks` for sibling check directories.
- `check50.py` helpers where cheap.

### Phase 4 — Goldens, docs, CI

- Golden fixtures: captured check50 3.4.0 results for 2-3 real
  `__init__.py` check sets from cs50/problems (hello world; a chain that
  uses `c.compile`; one that exercises return-value passing), byte-shaped
  against the documented JSON spec — same pattern as `cross_check.rs`,
  including the gated live cross-check when check50 is installed.
- Feature-gated CI job/step so the `python` build is exercised on both OS
  legs (`cargo build -p u50_check --features python`, `cargo test -p
u50_check --features python`).
- Docs: `u50_check/AGENTS.md` (remove the divergence note, document the
  feature flag and its limits), root `AGENTS.md` status line, this file
  updated with spike measurements.

### Phase 5 — `u50_tools`: shared uv pipeline, resolver plugins, plugin scaffolding

The extraction that makes everything below application-wide instead of
style-local. Two halves:

**(a) Extraction inventory** (from the current code):

| Moves to `u50_tools`                                                                                                                                             | From                                                                            | Notes                                                                                                                                                                                                                                                                   |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| uv provisioning pipeline (1,135 lines: `pipeline.rs`, `venv.rs`, `wheels.rs`, `pins.rs`, `setup/mod.rs`)                                                         | `u50_style/src/setup/`                                                          | Becomes the `uv` resolver plugin, parameterized by cache subdomain + package list; style50 pins stay with style50 (they are style's package table), u50_check adds its own.                                                                                             |
| cache paths + resolution (`cache_dir`, `cache_bin_dir`, `venv_bin_dir`, `tool_file_name`, `is_executable_file`, `is_explicit_path`, `locate_tool`, `ToolOrigin`) | `u50_style/src/format/tool.rs`, `format/mod.rs`                                 | Cache path parameterized: `u50/<domain>` (`u50/style50` today, `u50/check50` for check-side venvs). Resolution is generalized into the resolver plugin system (below); the `Toolchain` discovery in `language/rust.rs` becomes the `toolchain` plugin's implementation. |
| registry scaffolding (unique-id/name validation, path-safe names, duplicate detection)                                                                           | today duplicated: `u50_style` registry tests, `u50_check` `Graph::new`/registry | A tiny generic helper; domain traits stay in their crates.                                                                                                                                                                                                              |
| shared OS plumbing (symlink-safe `copy_tree`, process-group spawn/kill, `bash_command` shell resolution, execute-bit checks)                                     | `u50_check/src/api.rs` (+ equivalents in style50's spawn path)                  | Both crates consume; the review-hardened versions are the ones that move.                                                                                                                                                                                               |
| advisory cache lock + stderr-progress conventions                                                                                                                | `u50_style/src/setup`                                                           | Used identically by check-side provisioning.                                                                                                                                                                                                                            |

**(b) The resolver plugin system.** Tool provisioning is not a hardcoded
pipeline — the resolver itself is a plugin registry. Every tool declares
_which_ resolver plugin provisions it; resolver plugins register in their
own registry, so adding a provisioning mechanism (a new package ecosystem,
a new binary channel) is one module + one registration line — the same
rule as the language / check-set / capability registries:

```rust
/// One provisioning mechanism. Registered in `u50_tools`' resolver
/// registry; tools reference their resolver by id.
pub trait ResolverPlugin: Sync {
    /// Stable id ("uv", "toolchain", "system", "download").
    fn id(&self) -> &'static str;
    /// Can this resolver serve the tool (config shape + platform)?
    fn supports(&self, spec: &ToolSpec) -> bool;
    /// Locate an existing instance, cache-first. Returns the binary
    /// path and the user-facing origin ("found (cache)", "found
    /// (system)", "found (toolchain)").
    fn resolve(&self, spec: &ToolSpec) -> Option<Resolved>;
    /// Provision on demand. Resolvers that cannot provision (e.g.
    /// `system`) return Err carrying the actionable guidance instead.
    fn provision(&self, spec: &ToolSpec) -> anyhow::Result<()>;
}

pub struct ToolSpec {
    /// Unique tool name ("clang-format", "rustfmt", "valgrind").
    pub name: &'static str;
    /// The resolver plugin this tool prefers.
    pub resolver: &'static str;
    /// Optional fallback resolvers, tried in order (e.g. a tool whose
    /// primary is `download` may fall back to `system`).
    pub fallback: &'static [&'static str],
    /// Resolver-specific configuration, interpreted by the plugin:
    /// uv -> pip package + version pin; toolchain -> binary name;
    /// system -> search locations + minimum version; download ->
    /// pinned URL template + SHA-256 + platform mapping.
    pub config: ResolverConfig,
}
```

- **Initial resolver plugins** (the existing three behaviors, plus one new):
  - `uv` — wraps the extracted provisioning pipeline (pip packages, pinned
    versions, venv, parallel wheel fetch); serves style50's formatters and
    check-side venvs (Phase 6).
  - `toolchain` — resolves rustfmt from the Rust toolchain; provisioning
    unsupported by design (the rustfmt precedent).
  - `system` — discover-only (valgrind): bounded standard-location lookup,
    `found (system)`/`missing`; `provision` returns guidance (and, behind
    the Phase 8 opt-in consent flag, the package-manager bridge).
  - `download` — pinned standalone-binary downloads into the cache
    (`<cache>/u50/<domain>/bin/<tool>/<version>/`) with pinned SHA-256
    checksums and an explicit platform-mapping table. Implemented and
    unit-tested in Phase 5 (against a synthetic local file server — no
    network in tests); reserved for future tools that ship standalone
    binaries rather than pip wheels. Never the default for
    pip-installable tools — `uv` stays preferred wherever a wheel exists.
- **Policy hooks**: each resolver declares whether `provision` is
  supported. `u50 --status` walks every registered tool → its declared
  resolver → `resolve()` → status line. `u50 --setup` calls `provision()`
  for tools whose resolver supports it and prints each resolver's
  guidance for the rest. The opt-in system-mutation bridge (Phase 8) is
  simply `system`'s provision behavior gated behind the consent flag —
  not special-cased plumbing.
- **Security**: the `download` resolver pins exact versions + SHA-256 and
  is cache-first; `uv` pins versions and reuses the shared cache offline;
  `system` only reads. Every resolver's config is validated through the
  shared registry scaffolding (unique ids, non-empty names).
- **Stability**: `u50_style` re-exports its public API (`Formatter`,
  `ToolOrigin`, `locate_tool`, `setup_missing`, …) from `u50_tools`, so the
  CLI and any external callers see no change. style50's golden fixtures and
  tool tests are the regression net; the docs (`u50_style/AGENTS.md`) point
  at the new home.
- **What stays domain-specific**: `LanguagePlugin` + per-language modules,
  formatters, diff/renderers (style50); `CheckSetPlugin` + `checks/`, the
  runner, graph, render (check50). Shared scaffolding never grows domain
  knowledge.
- Gate: `cargo test --workspace` green (style50 goldens byte-identical,
  tool-resolution tests pass through the re-exports), no behavior change,
  CI both legs **plus** resolver-registry tests: adding a throwaway
  resolver plugin = 1 module + 1 registration line, picked up by
  `--status`/`--setup` generically.

### Phase 6 — pip `dependencies:` via `u50_tools::uv`

Solves the _"embedded interpreter does not install packages"_ limitation
using the extracted pipeline:

- **Cache layout**: requirement sets provision into
  `<cache>/u50/check50/deps/<hash(requirements sorted)>` — one venv per
  distinct requirement set (cheap; the shared uv cache reuses downloaded
  wheels across sets), so concurrent check sets cannot poison each other.
  A small manifest records the resolved package versions (reproducibility;
  the cache key changes when the declaration changes).
- **Wiring**: `python/deps.rs` reads `.cs50.yaml`'s `dependencies:`
  (already parsed in `yaml.rs` as an ignored `serde_yaml::Value`; pip
  requirement-string or list, check50 parity), provisions/verifies the venv
  lazily on first use (bulk via `u50 --setup`, per-problem), and appends the
  venv's `site-packages` (`<venv>/lib/python3.x/site-packages`,
  `<venv>\Lib\site-packages` on Windows) to the embedded interpreter's
  `sys.path` for that check set's runs.
- **Pure-Python policy**: RustPython cannot load C extensions. Before
  install, each selected wheel is classified (`py3-none-any` vs platform
  wheel — the extracted `wheels.rs` already knows the tags): non-pure
  packages are _installed but flagged_, and an import of a flagged
  package's C parts raises a clear `check50.Failure`-shaped error ("needs
  CPython; not supported by the embedded interpreter") instead of a cryptic
  import error. The check author sees the exact package to drop or replace.
- **Offline/flags**: `--offline` uses only the shared uv cache (clear error
  if a wheel is not cached); `--local`/`--dev` may fetch; the advisory lock
  serializes concurrent first-use provisioning exactly as style50 does.
- Gate: a check set declaring `dependencies: [pyyaml]` (pure) runs; one
  declaring a C-extension package imports with the explained error; both
  legs in CI with a warm cache; cold-cache provisioning shown on stderr
  only.

### Phase 7 — flask: a capability plugin (Uv strategy)

`FlaskCapability` in `capabilities/flask.rs`, unblocked by Phases 5-6:
Flask + Werkzeug + Jinja2 + click + itsdangerous + markupsafe are all pure
Python, so the capability's need is "a pinned Python package set" — exactly
what the Uv strategy provisions:

- **Lazy, pinned provisioning**: first `import check50.flask` (or first
  native use) provisions a pinned Flask stack venv at
  `<cache>/u50/check50/flask` — exact versions pinned like style50's
  `PINNED_VERSIONS` with a version-fixture test — once per machine, reused
  by all subsequent runs. `ensure()` is a no-op when provisioned.
- **`python/flask.rs`** stays a thin bridge: it binds `check50.flask`'s
  surface (`get`/`post`/data asserts with check50's exact names and payload
  shapes — pinned by the Phase-0/3 study of `flask.py`; the local source
  snapshot in `tmp/check50-src` was a stale 503 download and must be
  re-fetched) onto `FlaskCapability`'s native objects, which drive the
  student's app through Werkzeug's test client inside the embedded
  interpreter.
- **Gate**: one golden from a flask-using cs50 check set (or a synthetic
  fixture if no canonical one exists) byte-shaped against captured check50
  output; cold-cache provisioning on stderr only; `u50 --status` shows the
  capability.

### Phase 8 — valgrind: a capability plugin (the `system` resolver)

`ValgrindCapability` in `capabilities/valgrind.rs`, backed by the `system`
resolver plugin — the resolver-plugin system in action for a tool that no
provisioning resolver can install:

- **Discovery, not installation**: valgrind is a system C tool; the
  capability reports `found (cache)` / `found (system)` / `missing`.
  `ensure()` never installs: a missing valgrind prints actionable guidance
  (`apt/dnf/brew install valgrind`) as a non-fatal line. A check set that
  actually uses valgrind **skips with guidance** (check50 `Cause`-shaped)
  when the tool is absent — never a whole-run bail.
- **`python/c.rs`** bridges `check50.c.valgrind`'s surface (decorate a
  check so its spawned programs run under valgrind and valgrind-clean
  output is asserted — exact semantics pinned by re-fetching `c.py`, the
  snapshot was stale) onto the capability's native implementation.
- **Timeout interplay**: valgrind is 10-50× slower; valgrind-decorated
  checks get a documented multiplier (e.g. ×25, capped) applied to the
  effective deadline — the author's explicit `timeout=` always wins over
  the multiplier.
- **`u50 --status` / `--setup`**: the root-level table gains the
  capability/tools section (`valgrind  found (system) / missing`);
  `--setup` bulk-provisions the Uv-strategy capabilities (flask, any
  `dependencies:`) and prints the valgrind guidance line.
- **Why valgrind cannot be cache-provisioned like the formatters** (verified
  against the live indexes, not assumed):
  - The uv pipeline installs _wheels from PyPI_; PyPI's only `valgrind`
    package is version **0.0.0** — a ctypes helper for controlling callgrind
    instrumentation from _inside_ a process already running under valgrind.
    It does not ship the valgrind binary, so there is nothing to provision.
  - Third-party prebuilt channels do exist (conda-forge packages valgrind)
    but are a compatibility trap: valgrind is coupled to the host
    kernel/libc, and conda-forge's builds cover `linux-64`, `linux-aarch64`,
    `linux-ppc64le`, and `osx-64` only — **no Apple Silicon, no Windows**
    (upstream valgrind does not support them). A frozen cache binary would
    break subtly on mismatched kernels, and the strategy could never apply
    to a whole platform u50 supports. The formatters are cache-installable
    precisely because they are pure-Python/any-wheel packages; valgrind is
    the same class of exception as rustfmt (hence `System`, not `Uv`).
- **Optional, explicit system-bridge** (the safe version of "on-demand"):
  `u50 --setup` detects the available package manager (apt/dnf/pacman/brew)
  and — only with an explicit opt-in flag (e.g. `u50 --setup
--install-system-tools`) and its consent prompt — runs the install
  command for missing System-strategy tools. Never the default, never
  silent, never from a check run: mutating the host system is outside the
  cache-only philosophy, so it stays behind a flag that says exactly what
  it does. Without the flag, behavior is the planned guidance + skip.
- **Gate**: golden for a valgrind-shaped check (captured JSON), the
  missing-tool skip path, status-table output, and both legs (the guidance
  path is exercised where valgrind is absent, e.g. CI Windows).

## Risks and mitigations

| Risk                                                                                     | Mitigation                                                                                                                                                                                                                              |
| ---------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| rustpython stdlib gaps (a check imports a module the VM lacks)                           | Inventory imports across cs50/problems check sets during Phase 0; stub-with-clear-error for missing modules rather than a segfault; document coverage.                                                                                  |
| No C extensions (numpy etc.)                                                             | Phase 6 flags non-pure wheels at install and raises a clear, named error at import; check authors drop or replace the package.                                                                                                          |
| Supply-chain exposure from `dependencies:` fetches                                       | In-process uv pipeline only (no script execution beyond wheels), resolved versions recorded in the venv manifest, versions change only when the declaration changes, shared cache reused offline thereafter; `--offline` never fetches. |
| Hang in pure-Python loop ignores the deadline                                            | Instruction-count/trace hook if the pinned rustpython supports it; else documented limitation (spawn-bound real checks are already bounded).                                                                                            |
| GIL + per-check threads serialize                                                        | Real check sets are mostly spawn-wait time; document measured concurrency delta from Phase 0.                                                                                                                                           |
| Binary size / build time blowup                                                          | Behind the `python` feature; measure in Phase 0; workspace pin exactly (like the uv crates).                                                                                                                                            |
| rustpython API churn between versions                                                    | Exact workspace pin; Phase 0 records the tested version.                                                                                                                                                                                |
| MSRV conflict                                                                            | Gate in Phase 0 against `msrv = "1.96.0"`.                                                                                                                                                                                              |
| Windows differences (paths, PTY-free pipes, venv layout)                                 | The Rust `Run` layer already handles both legs; `deps.rs` uses the platform site-packages layout; Python bindings only bridge.                                                                                                          |
| A resolver plugin misdeclares its config (stale pin, wrong checksum, wrong platform map) | Resolver configs are validated through the shared registry scaffolding; each resolver ships version-fixture tests; `download` requires pinned SHA-256 and never floats to latest.                                                       |
| `u50_tools` extraction regresses style50                                                 | Re-exports keep the public API stable; style50's golden fixtures + tool tests are the regression net; the extraction is behavior-preserving by construction (moves + parameterization, no rewrites).                                    |
| Capability plugins drift from upstream `flask.py`/`c.py` semantics                       | Semantics pinned by re-fetching the source (the `tmp/check50-src` snapshot is stale) and by golden fixtures against captured check50 output; the capability trait keeps each surface small enough to verify in isolation.               |
| Capability registry bloat (every helper wants to be a capability)                        | A capability earns registration by owning a tool/dependency strategy and a dual (native + Python) surface; pure helpers stay in `u50_tools`. Documented admission rule.                                                                 |
| valgrind absent on the host                                                              | `System` strategy reports `missing`; affected checks skip with guidance; `--status` makes the gap visible before a run.                                                                                                                 |

## Verification checklist

- [ ] `cargo build/test -p u50_check --features python` green on both OS legs
- [ ] Default build (no feature) unchanged: size/build time within noise
- [ ] Phase-1 API fixtures: Python-driven `run/stdin/stdout/exit/reject`
      parity with the YAML cross-check expectations
- [ ] hello-world `__init__.py` results JSON matches the YAML/native shape
- [ ] Golden fixtures for 2-3 real cs50/problems check sets (captured JSON,
      gated live check50 cross-check)
- [ ] Return-value passing covered by a chain test
- [ ] `checks: __init__.py` without the feature errors cleanly
      ("rebuild with --features python")
- [ ] No plugin/check-set names outside `checks/` + `registry.rs` (grep)
      still holds; python.rs is core-generic (no per-problem knowledge)
- [ ] CI: feature-gated python job on both legs
- [ ] Resolver plugins: adding a throwaway resolver = 1 module + 1
      registration line; a tool can switch resolvers by editing its
      declaration; `--status`/`--setup` walk tools through their declared
      resolvers generically
- [ ] Phase 5: `u50_tools` extraction keeps style50 goldens byte-identical
      and its tool tests green through the re-exports; `u50_check` consumes
      the shared crate for resolver + provisioning
- [ ] Phase 6: pure-python dependency set provisions lazily; C-extension
      dependency imports with the explained error; `--offline` never
      fetches; warm-cache runs touch no network
- [ ] Phase 7: pinned Flask stack provisions once (`ensure()` idempotent);
      a flask check fixture matches captured check50 JSON; capability
      visible in `u50 --status`
- [ ] Phase 8: valgrind `found (system)`/`missing` in `u50 --status`;
      missing-valgrind check skips with guidance; valgrind timeout
      multiplier documented and tested
- [ ] Phase 8 (opt-in bridge): `--install-system-tools` prompts before
      mutating the system; default behavior remains guidance + skip
- [ ] Capability registry: adding a throwaway capability = 1 module +
      1 registration line; `--status`/`--setup` pick it up generically
