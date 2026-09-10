# u50_check Python Checks Plan (embedded RustPython)

Runs the legacy Python check files — the `__init__.py` / `check.py` authoring
model that the real check sets in [cs50/problems](https://github.com/cs50/problems)
actually use — inside `u50_check`, by embedding [RustPython](https://github.com/RustPython/RustPython)
and exposing a native `check50` module that bridges to the existing Rust check API.
This closes the port's largest documented divergence: today
`u50_check/src/lib.rs` bails with _"no registered check set matches … (Python
checks are not executable)"_ for the model that most shipped check sets use
(see `docs/CHECK50_PORT_NOTES.md` §7.1 and `docs/U50_CHECK_PLUGIN_PLAN.md` §"What
is NOT ported").

## Goal and non-goals

**Goal**: a check package whose `.cs50.yaml` says `checks: __init__.py` (or any
Python checks file) runs its `@check50.check()` functions through the same
engine — declaration order, dependency graph, filesystem inheritance,
timeouts, skip cascade, ansi/json rendering — that YAML "simple" checks and
native Rust plugins already use (per `docs/U50_CHECK_PLUGIN_PLAN.md`).

**Non-goals (documented divergences, same family as the plugin plan):**

- `check50.flask` (real Flask + WSGI test client) — not portable to an
  embedded interpreter without vendoring a web stack; provide a stub that
  raises `check50.Failure`-shaped errors with a clear rationale, revisit per-problem.
- pip `dependencies:` in `.cs50.yaml` — the embedded interpreter does not
  install packages. Warn loudly when a check set declares them.
- `check50.c.valgrind` — requires the valgrind binary; out of scope (same as
  upstream's optional tooling).
- gettext translations, remote mode, `import_checks` across GitHub repos
  (local paths only).

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

## Target architecture

```text
u50_check/src/
├── python.rs                  feature-gated module root (mod python when built)
│   ├── interp.rs              interpreter lifecycle: one VM per run, warm pool
│                             of scopes, sys.path wiring (check dir, run dir)
│   ├── api.rs                 the injected `check50` package: run/stdin/stdout/
│                             exit/kill, exists/include/log/data/hash, EOF,
│                             Failure/Mismatch/Missing exceptions — each call
│                             bridges to the Rust `CheckContext`/`Run` API
│   ├── registry.rs            `@check50.check` decorator: records (name,
│                             docstring, dependency, timeout, hidden) into
│                             the module registry, in declaration order
│   └── c.rs / regex.rs        check50.c.compile (spawn clang), regex.decimal
│                             (reuses the existing decimal_regex())
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

## Risks and mitigations

| Risk                                                           | Mitigation                                                                                                                                             |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| rustpython stdlib gaps (a check imports a module the VM lacks) | Inventory imports across cs50/problems check sets during Phase 0; stub-with-clear-error for missing modules rather than a segfault; document coverage. |
| No C extensions (numpy etc.)                                   | Not used by the legacy check API surface; clear error if imported.                                                                                     |
| Hang in pure-Python loop ignores the deadline                  | Instruction-count/trace hook if the pinned rustpython supports it; else documented limitation (spawn-bound real checks are already bounded).           |
| GIL + per-check threads serialize                              | Real check sets are mostly spawn-wait time; document measured concurrency delta from Phase 0.                                                          |
| Binary size / build time blowup                                | Behind the `python` feature; measure in Phase 0; workspace pin exactly (like the uv crates).                                                           |
| rustpython API churn between versions                          | Exact workspace pin; Phase 0 records the tested version.                                                                                               |
| MSRV conflict                                                  | Gate in Phase 0 against `msrv = "1.96.0"`.                                                                                                             |
| Windows differences (paths, PTY-free pipes)                    | The Rust `Run` layer already handles both legs; Python bindings only bridge.                                                                           |

## What is NOT ported (unchanged)

`check50.flask` (stub with clear rationale), pip `dependencies:` (loud
warning), `c.valgrind`, gettext, remote mode, `import_checks` from GitHub.

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
