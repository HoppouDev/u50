# u50_check Python Checks Plan (provisioned CPython)

Runs the legacy Python check files — the `__init__.py` / `check.py` authoring
model that the real check sets in [cs50/problems](https://github.com/cs50/problems)
actually use — inside `u50_check`, on a **CPython interpreter that u50
itself provisions** with the same in-process uv pipeline style50 already
uses. A revision of this plan replaced an embedded [RustPython](https://github.com/RustPython/RustPython)
interpreter with the provisioned CPython: real-CPython compatibility
(complete stdlib, working C extensions, current syntax) beats an embedded
VM on every axis that matters, and the interpreter is resolved from u50's
own cache — never from `PATH`.

This plan supersedes the RustPython revision in full (git history retains
it). The structure below keeps what survived: the resolver plugin system,
the capability plugin layer, and the Phases 6-8 work for pip
`dependencies:`, flask, and valgrind.

## Goal and non-goals

**Goal**: a check package whose `.cs50.yaml` says `checks: __init__.py` (or any
Python checks file) runs its `@check50.check()` functions through the same
engine — declaration order, dependency graph, filesystem inheritance,
timeouts, skip cascade, ansi/json rendering — that YAML "simple" checks and
native Rust plugins already use (per `docs/U50_CHECK_PLUGIN_PLAN.md`). The
checks execute on u50's provisioned CPython (a uv-managed interpreter +
venv at `<cache>/u50/check50/venv`), driven by a `check50` Python package
shipped by u50.

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
  each declaring its tool/dependency needs (uv-provisioned stack vs pinned
  downloaded binary) and exposing its surface to _both_ the Python
  `check50` package (`check50.flask`, `check50.c.valgrind`) and the native
  Rust check API. Adding a future capability is one module + one
  registration line — the same rule the language and check-set registries
  follow.

**Non-goals (permanent divergences):**

- gettext translations.
- Remote mode (`lib50.push`, submit.cs50.io polling).
- `import_checks` across GitHub repos (local sibling paths only).

Note: the earlier "no C extensions" divergence **dies with this revision** —
the provisioned interpreter is a real CPython build (uv-managed), so
C-extension wheels for the host platform import and run like in any
CPython environment.

## Why the provisioned CPython (alternatives considered)

| Option                             | Verdict                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Provisioned CPython subprocess** | Real CPython (uv-managed build), complete stdlib, C extensions work, current syntax — resolved from u50's own cache, never `PATH`. One subprocess per check mirrors check50's own `ProcessPoolExecutor` architecture, giving OS-level isolation that the existing process-group timeout/kill machinery already enforces. No new Rust dependencies; the interpreter is the same asset style50's pipeline already downloads. |
| Embedded RustPython                | Rejected (the plan's previous revision): stdlib gaps, no C extensions, VM maturity risk, binary-size/build-time cost, GIL-in-one-process, MSRV and API-churn risk — all eliminated by using a real CPython.                                                                                                                                                                                                                |
| Subprocess `python3` from `PATH`   | Violates the workspace principle that resolution never consults `PATH` (see `u50_style/AGENTS.md` tool management) and is not reproducible. The provisioned venv is the PATH-free equivalent.                                                                                                                                                                                                                              |
| PyO3 + CPython embedded            | Requires a libpython at runtime — not self-contained; also embeds the GIL into u50's process (a hung interpreter blocks the runner).                                                                                                                                                                                                                                                                                       |
| Transpile check files to Rust      | The authoring API is dynamic (decorators, closures, arbitrary Python) — not transpilation territory.                                                                                                                                                                                                                                                                                                                       |

**Venv ownership**: the check venv lives at `<cache>/u50/check50/venv` —
_not_ inside `u50/style50` — so style formatter reinstalls/upgrades can
never break checks and each domain pins independently. The downloaded
uv-managed CPython itself is shared (uv's interpreter cache), so there is
no duplicate interpreter download — only a small per-domain venv.

Trust model: check files come from cs50's GitHub orgs, the same trust level as
the compiled-in native plugins. Subprocess execution is _not_ a sandbox promise;
the timeout/deadline machinery (below) is about hangs, not malice.

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
4. **`check50.c.compile(file, ...)`** — clang invocation.
5. **`import_checks("../less")`** — import another check module from a
   sibling directory so check sets can extend each other.
6. **`check50.py`** helpers (`append_code`, `import_`, `compile`) — thin;
   implement where cheap, else document.
7. **`check50.flask`** and **`check50.c.valgrind`** — now planned as
   capability plugins (Phases 7 and 8).

## Target architecture

```text
Cargo.toml                    no new Rust dependencies for the interpreter;
                              style50's uv-* crates are already pinned
                              workspace-wide and gain a new consumer
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
│                             toolchain), system (discover-only), download
│                             (pinned standalone binaries + SHA-256,
│                             platform-mapped — first consumer: valgrind);
│                             cache paths parameterized by domain
│                             (`u50/style50`, `u50/check50`)
├── registry.rs               plugin-scaffolding helpers shared by every
│                             registry (unique-id/name validation, path-safe
│                             names, declaration-order iteration); the
│                             domain traits stay in their crates
└── fs.rs / proc.rs           shared OS plumbing: symlink-safe recursive
                              copy, process-group spawn/kill (from
                              u50_check/src/api.rs), shell resolution
                              (bash_command), execute-bit handling
u50_check/src/
├── python/                   the Python-check support (always compiled in —
│                             no optional dependency)
│   ├── bridge.rs             the process bridge: discovery pass (import the
│                             module once, emit the check registry as JSON)
│                             and per-check invocation (spawn the venv
│                             python in the check's run_dir with its own
│                             process group; feed it the dependency state;
│                             collect the result); wired into RunKind::Python
│   └── check50/              the `check50` Python package shipped by u50
│                             (real Python files, installed into the check
│                             venv during provisioning): the authoring
│                             surface — decorator registry, run/stdin/stdout/
│                             reject/exit/kill builder, exists/include/log/
│                             data/hash, EOF, Failure/Mismatch/Missing,
│                             regex.decimal, c.compile, import_checks, and
│                             (via capabilities) flask/valgrind surfaces
├── capabilities/              capability-plugin layer (see below)
│   ├── mod.rs                 CapabilityPlugin trait + registry (one module
│                             + one registration line per capability)
│   ├── flask.rs               FlaskCapability (uv resolver: pinned Flask
│                             stack installed into the check venv)
│   └── valgrind.rs            ValgrindCapability (download resolver:
│                             pinned prebuilt binary, no system reliance,
│                             skip-with-guidance on unsupported platforms)
├── plugin.rs                  new RunKind::Python { module, check } variant
└── runner.rs                  unchanged scheduling; RunKind::Python dispatches
                              through python/bridge.rs

# cache layout
<cache>/u50/check50/venv/         the check interpreter + shipped check50
                                  package (+ pinned capability stacks on
                                  demand)
<cache>/u50/check50/deps/<hash>/  one venv per distinct `dependencies:` set
                                  (Phase 6)
<cache>/u50/check50/valgrind/<v>/ extracted valgrind prefix (Phase 8)
```

## Execution and isolation model

check50 runs every check in a **separate process** (`ProcessPoolExecutor`);
the port keeps that shape, which maps directly onto the engine's existing
isolation and timeout machinery:

- **Discovery pass** — one subprocess imports the checks module once and
  emits the registry as JSON: check names in declaration order, docstrings
  (descriptions), dependencies, `timeout=`, `hidden=`. The Rust side turns
  that into `Vec<CheckSpec>` and feeds the existing graph/runner. All the
  engine guarantees (declaration-order results, skip cascade,
  filesystem inheritance, rendering) are unchanged.
- **Per-check invocation** — the runner spawns one `venv/bin/python`
  subprocess per check, cwd = the check's `run_dir`, own process group;
  the subprocess imports the module and calls the named function. The
  dependency's return value is passed via a pickled state file in the run
  root (Phase 3; check50 pickles dependency state the same way).
- **Timeouts and kills**: the per-check deadline enforcement now kills the
  check's **process group** — the interpreter and every student process it
  spawned die together (the machinery built during the review fixes; no
  mid-bytecode interruption problem exists here, unlike an embedded VM).
- **Failures**: a raised `check50.Failure` exits the subprocess with a
  serialized failure on stdout (JSON envelope); the bridge maps it to
  `Failure`/`Mismatch` causes. Any other Python exception maps to
  `Cause::Error` (panic-path parity). A killed subprocess maps to the
  timeout cause.
- **The API surface lives in the shipped `check50` package** (option (A)):
  the package implements the run/assertion chain itself — subprocess
  spawn with the same semantics check50's `_api.py` documents (prompt
  absorption, EOF, regex/exact/decimal matching, reject, exit-code
  assert, SIGSEGV detection). This is check50's own architecture, so
  upstream parity is achieved by porting documented `_api.py` behavior,
  and the Rust `Run` machinery stays untouched for YAML/native checks.
  Documented alternative (option (B), kept in reserve): an RPC bridge so
  the Rust `Run` remains the single implementation of the assertion
  chain and Python is a thin client — adopted only if golden parity
  shows drift between the two implementations. Either way, the shared
  cross-check fixtures are the drift guard.
- **No GIL, no VM budget**: interpreter startup (~30-50 ms) is negligible
  against check timeouts (default 60 s), and there is no embedded-VM
  footprint in the binary.

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
    /// the pinned stack installed into the check venv? valgrind -> the
    /// `download` resolver: pinned prebuilt binary). Reported by
    /// `u50 --status`.
    fn availability(&self) -> Availability;   // Available | NeedsProvision | Missing { guidance }
    /// Provision when supported by the resolver (flask: yes, once per
    /// machine; valgrind: downloads the pinned build instead).
    fn ensure(&self) -> anyhow::Result<()>;
    /// The native Rust surface (usable from native check sets, not just
    /// Python checks) — e.g. `ctx.flask().get(...)`, `ctx.c_valgrind(..)`.
    /// The Python surface (`check50.flask`, `check50.c.valgrind`) binds to
    /// the same objects, so there is exactly one implementation.
    // (per-capability concrete methods; not part of the shared trait)
}
```

Consequences of the shape:

- **One implementation, two surfaces**: the Python side of each capability
  lives in the shipped `check50` package and binds to the capability's
  native objects; native Rust check sets can use the same capability
  without Python.
- **Provisioning is declarative**: each capability names its resolver and
  package/binary needs; `u50 --status` reports every capability's
  availability; `u50 --install-tools` bulk-provisions the provisionable
  ones — one table, all domains.
- **Skip, never bail**: an unavailable capability yields check-level skips
  with guidance (check50 `Cause`-shaped), never a whole-run failure.
- **Extensibility**: a future capability (e.g. `check50.py` extras, or a
  clang-tidy helper) is one module + one registration line, provisioned by
  an existing resolver plugin.

## Phases

### Phase 0 — Interpreter + venv spike (feasibility + budget)

- Point the uv pipeline at a check50 domain: provision the uv-managed
  CPython and create `<cache>/u50/check50/venv`; round-trip
  `venv/bin/python -c "import check50"` with the shipped package staged
  into site-packages.
- Measure: cold/warm venv creation time, interpreter startup latency,
  subprocess-per-check overhead at the scale of a 10-check set, and the
  Windows layout (`venv\Scripts\python.exe`).
- Gate: both OS legs; no new Rust dependencies; the interpreter is
  resolved cache-only (`PATH` untouched).

### Phase 1 — The shipped `check50` package (core surface)

- `python/check50/`: the authoring surface as real Python — decorator +
  registry, the chainable run builder (subprocess-based with `_api.py`'s
  documented semantics: prompt absorption, EOF, regex/exact/decimal
  matching, reject, exit-code assert, SIGSEGV detection), `exists`/
  `include`/`log`/`data`/`hash` (run_dir-resolved, per the review fixes),
  exceptions with the repr-truncated payload shapes, `EOF`.
- Fixture tests: Rust integration tests drive the venv python against the
  same expectations the YAML cross-check suite already pins (echo/stdin
  prompt, EOF, reject, mismatch payload, Failure propagation).
- Gate: fixture parity with `cross_check.rs`'s expectations.

### Phase 2 — Decorator bridge (the end-to-end pipeline)

- `python/bridge.rs`: the discovery pass (module import → registry JSON →
  `Vec<CheckSpec>` in declaration order, docstring → description,
  dependency fn → name, `timeout=`/`hidden=` honored) and the per-check
  invocation (own process group, cwd = run_dir).
- New `RunKind::Python { module, check }` in `plugin.rs`; raised
  `check50.Failure` → `Failure`/`Mismatch` causes; other exceptions →
  `Cause::Error`; killed subprocess → timeout cause.
- Gate: a hello-world `__init__.py` (exists → compiles → prints hello,
  the canonical cs50 example) produces the same results JSON shape as the
  YAML/native equivalents.

### Phase 3 — Parity work the legacy checks actually need

- **Dependency return-value passing**: each check's subprocess pickles its
  return value to a state file in the run root; dependents receive it as
  their first argument (check50 parity — it pickles dependency state the
  same way). Native Rust checks get the same capability (an API evolution,
  not Python-only — real cs50 chains pass state this way).
- `check50.c.compile`: clang invocation from the shipped package
  (PATH-free clang discovery like rustfmt's).
- `check50.regex.decimal` → the same semantics as the Rust
  `decimal_regex()` (document the consuming-boundary divergences there).
- `import_checks` for sibling check directories.
- `check50.py` helpers where cheap.

### Phase 4 — Goldens, docs, CI

- Golden fixtures: captured check50 3.4.0 results for 2-3 real
  `__init__.py` check sets from cs50/problems (hello world; a chain that
  uses `c.compile`; one that exercises return-value passing), byte-shaped
  against the documented JSON spec — same pattern as `cross_check.rs`,
  including the gated live cross-check when check50 is installed.
- CI: the python-check path exercised on both OS legs (no feature gate —
  there is no optional dependency; the Windows venv layout is covered).
- Docs: `u50_check/AGENTS.md` (remove the divergence note, document the
  venv, the shipped package, and its limits), root `AGENTS.md` status
  line, this file updated with spike measurements.

### Phase 5 — `u50_tools`: shared uv pipeline, resolver plugins, plugin scaffolding

The extraction that makes everything below application-wide instead of
style-local. Two halves:

**(a) Extraction inventory** (from the current code):

| Moves to `u50_tools`                                                                                                                                             | From                                                                            | Notes                                                                                                                                                                                                                                                                   |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| uv provisioning pipeline (1,135 lines: `pipeline.rs`, `venv.rs`, `wheels.rs`, `pins.rs`, `setup/mod.rs`)                                                         | `u50_style/src/setup/`                                                          | Becomes the `uv` resolver plugin, parameterized by cache subdomain + package list; style50 pins stay with style50 (they are style's package table), u50_check adds its own (the check venv, Phase 0).                                                                   |
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
    /// Unique tool name ("clang-format", "rustfmt", "valgrind",
    /// "check-venv", ...).
    pub name: &'static str;
    /// The resolver plugin this tool prefers.
    pub resolver: &'static str;
    /// Optional fallback resolvers, tried in order.
    pub fallback: &'static [&'static str],
    /// Resolver-specific configuration, interpreted by the plugin:
    /// uv -> pip package/interpreter + version pin; toolchain -> binary
    /// name; system -> search locations + minimum version; download ->
    /// pinned URL template + SHA-256 + platform mapping.
    pub config: ResolverConfig,
}
```

- **Initial resolver plugins**:
  - `uv` — wraps the extracted provisioning pipeline (pip packages, pinned
    versions, venv, parallel wheel fetch); serves style50's formatters,
    the check venv + shipped `check50` package, `dependencies:` (Phase 6),
    and flask (Phase 7).
  - `toolchain` — resolves rustfmt from the Rust toolchain; provisioning
    unsupported by design (the rustfmt precedent).
  - `system` — discover-only: bounded standard-location lookup,
    `found (system)`/`missing`; `provision` is unsupported by design and
    returns guidance. No shipped tool declares it initially — it exists as
    the read-only escape hatch for hosts where nothing else can serve a
    tool.
  - `download` — pinned standalone-binary downloads into the cache
    (`<cache>/u50/<domain>/<tool>/<version>/<platform>/`) with pinned
    SHA-256 checksums and an explicit platform-mapping table; full-prefix
    extraction for tools that ship one (valgrind's `bin/` + `lib/`).
    Implemented and unit-tested in Phase 5 (against a synthetic local file
    server — no network in tests). Its first consumer is valgrind
    (Phase 8); it is never the default for pip-installable tools — `uv`
    stays preferred wherever a wheel exists.
- **Policy hooks**: each resolver declares whether `provision` is
  supported. `u50 --status` walks every registered tool → its declared
  resolver → `resolve()` → status line. **`u50 --install-tools`** (new
  root-level flag) bulk-provisions every registered tool through its
  declared resolver — style formatters, the check venv, capability stacks,
  declared `dependencies:`, pinned binary downloads — and prints each
  resolver's guidance for what it cannot provision; lazy per-tool
  provisioning on first use is unchanged. `--setup` keeps its existing
  style-only meaning and is documented as superseded by
  `--install-tools`.
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

Solves the _"embedded interpreter does not install packages"_ limitation —
now trivially: the check interpreter is a real CPython, so the only work is
installing the requirements:

- **Cache layout**: requirement sets provision into
  `<cache>/u50/check50/deps/<hash(requirements sorted)>` — one venv per
  distinct requirement set (cheap; the shared uv cache reuses downloaded
  wheels across sets), so concurrent check sets cannot poison each other.
  A small manifest records the resolved package versions (reproducibility;
  the cache key changes when the declaration changes). The declared
  requirements install **on top of the check venv's interpreter** (the
  deps venv is created from the same uv-managed CPython; the shipped
  `check50` package is staged into it as well).
- **C extensions work**: the interpreter is a real CPython build, so
  host-platform wheels (numpy, etc.) import and run — the old pure-Python
  policy is gone. Only genuinely-unsupported-platform wheels fail with
  uv's normal resolution error, surfaced as a clear check-set error.
- **Wiring**: `python/deps.rs` reads `.cs50.yaml`'s `dependencies:`
  (already parsed in `yaml.rs` as an ignored `serde_yaml::Value`; pip
  requirement-string or list, check50 parity), provisions/verifies the
  deps venv lazily on first use (bulk via `u50 --install-tools`), and the
  bridge points that check set's subprocesses at the deps venv's
  interpreter.
- **Offline/flags**: `--offline` uses only the shared uv cache (clear error
  if a wheel is not cached); `--local`/`--dev` may fetch; the advisory lock
  serializes concurrent first-use provisioning exactly as style50 does.
- Gate: a check set declaring `dependencies: [pyyaml]` runs; a
  C-extension dependency (e.g. `numpy`) imports and runs; both legs in CI
  with a warm cache; cold-cache provisioning shown on stderr only.

### Phase 7 — flask: a capability plugin (the `uv` resolver)

`FlaskCapability` in `capabilities/flask.rs`: Flask + Werkzeug + Jinja2 +
click + itsdangerous + markupsafe are all pure Python, so the capability's
need is "a pinned Python package set" — exactly what the `uv` resolver
provisions:

- **Lazy, pinned provisioning**: first `import check50.flask` (or first
  native use) installs the pinned Flask stack into the check venv — exact
  versions pinned like style50's `PINNED_VERSIONS` with a version-fixture
  test — once per machine, reused by all subsequent runs. `ensure()` is a
  no-op when provisioned.
- **`check50.flask`** is implemented in the shipped package (Python),
  driving the student's app through Werkzeug's test client with check50's
  exact helper names and payload shapes (pinned by the Phase-0/3 study of
  `flask.py`; the local source snapshot in `tmp/check50-src` was a stale
  503 download and must be re-fetched). On a real CPython this is a
  faithful port, not a bridge.
- **Gate**: one golden from a flask-using cs50 check set (or a synthetic
  fixture if no canonical one exists) byte-shaped against captured check50
  output; cold-cache provisioning on stderr only; `u50 --status` shows the
  capability.

### Phase 8 — valgrind: a capability plugin (the `download` resolver)

`ValgrindCapability` in `capabilities/valgrind.rs`, backed by the `download`
resolver plugin — valgrind ships as **pinned prebuilt binaries** resolved
into u50's cache, exactly like the formatters are provisioned, with no
reliance on any system installation:

- **Pinned provisioning**: the capability's `ToolSpec` declares the
  `download` resolver with a pinned upstream build per platform
  (conda-forge valgrind builds: `linux-64`, `linux-aarch64`,
  `linux-ppc64le`, `osx-64`), each with a pinned URL + SHA-256 in an
  explicit platform-mapping table. First use (or `u50 --install-tools`)
  downloads the pinned package into
  `<cache>/u50/check50/valgrind/<version>/<platform>/` and extracts the
  full prefix (`bin/valgrind` + `lib/valgrind/*` — valgrind locates its
  runtime directory relative to the binary, so the extracted prefix is
  self-contained). `provision` is idempotent and cache-first; `--offline`
  uses only the cached copy.
- **No system reliance**: the `ToolSpec` declares **no `system` fallback** —
  a missing valgrind never silently degrades to whatever the host happens
  to have installed. `u50 --status` shows `found (cache)` / `missing` /
  `unsupported (platform)`:
  - `unsupported (platform)` — upstream valgrind has no builds for Apple
    Silicon or Windows (see the platform map above); valgrind-decorated
    checks on those platforms **skip with guidance** naming the
    limitation, instead of depending on a system install.
- **Runtime self-check**: a pinned prebuilt binary can still mismatch the
  host kernel, so provision runs `valgrind --version` once; a failure or
  crash marks the cached copy bad and reports a clear error
  (re-provision/skip) instead of letting checks fail mysteriously.
- **`check50.c.valgrind`** is implemented in the shipped package
  (decorate a check so its spawned programs run under valgrind and
  valgrind-clean output is asserted — exact semantics pinned by
  re-fetching `c.py`, the snapshot was stale), backed by the capability's
  resolved pinned binary.
- **Timeout interplay**: valgrind is 10-50× slower; valgrind-decorated
  checks get a documented multiplier (e.g. ×25, capped) applied to the
  effective deadline — the author's explicit `timeout=` always wins over
  the multiplier.
- **`u50 --status` / `--install-tools`**: the root-level table gains the
  capability/tools section (`valgrind  found (cache) / missing /
unsupported (platform)`); `--install-tools` provisions every registered
  tool through its declared resolver (style formatters, the check venv,
  flask, declared `dependencies:` via `uv`; valgrind via `download`).
- **Why the pip route is impossible** (verified against the live indexes,
  not assumed): PyPI's only `valgrind` package is version **0.0.0** — a
  ctypes helper for controlling callgrind instrumentation from _inside_ a
  process already running under valgrind. It does not ship the valgrind
  binary, so the `uv` resolver has nothing to install; `download` with
  pinned prebuilt packages is the only honest channel.
- **Gate**: golden for a valgrind-shaped check (captured JSON), the
  provision-on-first-use + offline-cache path, checksum-verification
  failure, the `unsupported (platform)` skip (CI Windows / Apple Silicon),
  and the runtime self-check failure path.

## Risks and mitigations

| Risk                                                                                                                          | Mitigation                                                                                                                                                                                                                              |
| ----------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Subprocess-per-check startup cost (~30-50 ms each)                                                                            | Negligible against the 60 s default timeout; check50 itself pays the same cost (per-process checks); measured in Phase 0.                                                                                                               |
| Semantic drift between the Python `check50` package and the Rust `Run` machinery (two implementations of the assertion chain) | Shared cross-check fixtures pin both against identical expectations; the documented fallback (option (B)) is an RPC bridge to the Rust `Run` if drift proves real.                                                                      |
| Pickled dependency state is code-execution if tampered                                                                        | State files live in the run root created and owned by u50 for the duration of one run (trusted check phases only, same trust as check50's ProcessPoolExecutor pickling); never read from student-writable paths.                        |
| Supply-chain exposure from `dependencies:` fetches                                                                            | In-process uv pipeline only (no script execution beyond wheels), resolved versions recorded in the venv manifest, versions change only when the declaration changes, shared cache reused offline thereafter; `--offline` never fetches. |
| uv-managed CPython availability/platform gaps                                                                                 | The same managed builds style50 already depends on; Phase 0 records the platform coverage; `--status` reports the interpreter like any tool.                                                                                            |
| Binary size / build time                                                                                                      | No new Rust dependencies at all — the only cost is the shipped Python package (kilobytes) and the on-demand venv download.                                                                                                              |
| Windows differences (venv layout, `Scripts\python.exe`, paths)                                                                | Phase 0 covers the Windows venv layout; the runner's process-group spawn/kill already handles both legs.                                                                                                                                |
| A resolver plugin misdeclares its config (stale pin, wrong checksum, wrong platform map)                                      | Resolver configs are validated through the shared registry scaffolding; each resolver ships version-fixture tests; `download` requires pinned SHA-256 and never floats to latest.                                                       |
| `u50_tools` extraction regresses style50                                                                                      | Re-exports keep the public API stable; style50's golden fixtures + tool tests are the regression net; the extraction is behavior-preserving by construction (moves + parameterization, no rewrites).                                    |
| Capability plugins drift from upstream `flask.py`/`c.py` semantics                                                            | Semantics pinned by re-fetching the source (the `tmp/check50-src` snapshot is stale) and by golden fixtures against captured check50 output; the capability trait keeps each surface small enough to verify in isolation.               |
| Capability registry bloat (every helper wants to be a capability)                                                             | A capability earns registration by owning a tool/dependency strategy and a dual (native + Python) surface; pure helpers stay in `u50_tools`. Documented admission rule.                                                                 |
| Pinned valgrind binary vs host kernel mismatch                                                                                | Runtime self-check (`valgrind --version`) at provision marks bad cache copies with a clear error (re-provision/skip); per-platform version fixtures; affected checks skip rather than fail mysteriously.                                |
| valgrind has no build for the host platform (Apple Silicon, Windows)                                                          | `unsupported (platform)` status; valgrind-decorated checks skip with guidance naming the limitation; no system fallback is declared.                                                                                                    |

## Verification checklist

- [ ] `cargo build/test --workspace` green on both OS legs (no new deps,
      no feature gate)
- [ ] Phase 0: `<cache>/u50/check50/venv` provisions cache-only (PATH
      untouched) on both legs; startup/subprocess overhead measured
- [ ] Phase-1 fixtures: Python-driven `run/stdin/stdout/exit/reject`
      parity with the YAML cross-check expectations
- [ ] hello-world `__init__.py` results JSON matches the YAML/native shape
- [ ] Golden fixtures for 2-3 real cs50/problems check sets (captured JSON,
      gated live check50 cross-check)
- [ ] Return-value passing covered by a chain test (pickled state file)
- [ ] C extensions: a `dependencies: [numpy]` check set imports and runs
- [ ] No plugin/check-set names outside `checks/` + `registry.rs` (grep)
      still holds; the shipped `check50` package is generic (no per-problem
      knowledge)
- [ ] CI: the python-check path exercised on both legs
- [ ] Resolver plugins: adding a throwaway resolver = 1 module + 1
      registration line; a tool can switch resolvers by editing its
      declaration; `--status`/`--setup` walk tools through their declared
      resolvers generically
- [ ] Phase 5: `u50_tools` extraction keeps style50 goldens byte-identical
      and its tool tests green through the re-exports; `u50_check` consumes
      the shared crate for resolver + provisioning
- [ ] Phase 6: `dependencies:` provisions lazily into the per-set venv;
      `--offline` never fetches; warm-cache runs touch no network
- [ ] Phase 7: pinned Flask stack provisions once (`ensure()` idempotent);
      a flask check fixture matches captured check50 JSON; capability
      visible in `u50 --status`
- [ ] Phase 8: valgrind `found (cache)`/`missing`/`unsupported (platform)`
      in `u50 --status`; pinned download provisions on first use and via
      `--install-tools`; checksum + runtime self-check failure paths
      tested; timeout multiplier documented
- [ ] `u50 --install-tools`: bulk-provisions every registered tool
      through its declared resolver (style formatters, the check venv,
      flask, `dependencies:`, valgrind); lazy first-use provisioning
      unchanged; `--setup` documented as style-specific and superseded
- [ ] Capability registry: adding a throwaway capability = 1 module +
      1 registration line; `--status`/`--install-tools` pick it up
      generically
