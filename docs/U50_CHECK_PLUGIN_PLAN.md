# u50_check Plugin Port Plan

Implements the port of check50's architecture into `u50_check/`, per
`docs/CHECK50_PORT_NOTES.md`, following the same plugin-registry model
already shipped in `u50_style` (`registry.rs` + zero-sized plugin
structs + trait objects; the `gate.AddPlugin` analog).

## Principles

1. **One module per check set (problem)** — everything about a problem's
   checks lives in that module (like `u50_style`'s per-language modules).
2. **`registry.rs` is the only core file that names plugins** — adding a
   problem is one module + one registration line.
3. **Core is generic** — the runner, results model, builders, YAML
   interpreter, and renderers never name a specific check or tool.
4. **Byte/behavior parity with check50 where it matters**: result model,
   JSON shape, ansi render, exit codes, dependency/skip cascade, prompt
   and EOF semantics.
5. **Documented divergences**: no GitHub slug resolution/remote results
   (local + dev mode only), no gettext i18n, no Flask/py helpers (not
   portable; can be revisited per-problem as plugins).

## Target architecture

```text
u50_check/src/
├── registry.rs               the plugin registry (names every check set)
├── lib.rs                    crate root (mod + pub use)
│
├── plugin.rs                 CheckSetPlugin trait + CheckHandle
├── check.rs                  the check-authoring API (builder + failures)
├── result.rs                 CheckResult / Cause / payload model
├── graph.rs                  dependency graph + scheduling + skip cascade
├── runner.rs                 process-isolated execution + filesystem
│                             inheritance + timeout
├── yaml.rs                   "simple" .cs50.yaml check interpretation
│
├── render/
│   ├── mod.rs                render registry accessors
│   ├── ansi.rs               AnsiPlugin (:)/:|/:( lines)
│   ├── json.rs               JsonPlugin (check50 JSON schema, 4-space)
│   └── html.rs               (later; jinja2-equivalent templating)
│
└── checks/                   one module per problem (plugins)
    ├── hello.rs              example: exists/compiles/prints_hello
    └── ...
```

## Phase 1 — results model + check API (core)

- `result.rs`: `CheckResult { name, description, passed: Passed,
log: Vec<String>, cause: Option<Cause>, data, dependency: Option<String> }`
  with `Passed` = `Passed | Failed | Skipped` (check50's
  `bool|None`) and `Cause { rationale, help, error? }` — JSON shaped
  exactly like check50's documented schema.
- `check.rs`: the authoring API as a builder, the Rust analog of
  `_api.py::run`:
  - `Run { .. }` chain: `run(cmd)` → `stdin(line | Eof, prompt?, timeout?)`
    → `stdout(pattern | None, exact?, timeout?)` → `reject(timeout?)` →
    `exit(code | None, timeout?)` → `kill()`
  - PTY/pipe spawn with prompt-absorption, EOF sentinel, CRLF→LF,
    regex/exact/number matching (`regex::decimal` analog),
    SIGSEGV detection, exit-code assertion
  - `exists`, `include` (copy from check dir), `log`, `data`, `hash`
  - `Failure { rationale, help }`, `Missing`, `Mismatch` with the same
    payload shape and repr-truncated messages
- Acceptance: the API compiles and unit-tests against `/bin/sh` fixtures
  (prompt, EOF, reject, exit code, mismatch payload), mirroring
  check50's own `tests/checks/*` samples.

## Phase 2 — runner: graph, isolation, inheritance

- `graph.rs`: dependency graph (check → dependents, `None`-rooted),
  declaration-order results, `--target` subgraph, skip cascade with the
  exact `"can't check until a frown turns upside down"` rationale.
- `runner.rs`: each check runs in a **separate process** with its own
  `run_dir`, created by copying the dependency's `run_dir` (filesystem
  inheritance, the `compiles → runs the binary` pattern); per-check
  timeout; `Failure` → failed, other errors → skipped-with-error;
  log truncation (`max_log_lines`, `"..."` head).
- Concurrency: dependency-free checks run in parallel; dependents
  dispatch as dependencies pass (like `ProcessPoolExecutor` + futures).
- Acceptance: a synthetic 3-check chain (exists → compiles → runs)
  exercises inheritance, skip cascade, timeout, and declaration-order
  results.

## Phase 3 — plugins: CheckSetPlugin + registry

- `plugin.rs`:

  ```rust
  pub(crate) trait CheckSetPlugin: Sync {
      /// Stable id ("hello"), also the slug-ish lookup key.
      fn id(&self) -> &'static str;
      /// The checks in declaration order; dependencies reference ids.
      fn checks(&self) -> &'static [CheckSpec];
  }
  ```

  with `CheckSpec { name, description, dependency: Option<&'static str>,
run: fn(&mut CheckContext) -> Result<(), Failure> }` — a check is a
  plain `fn` in the problem's module, registered via the plugin's
  `checks()` slice (zero-sized plugin struct, like `u50_style`).

- `registry.rs`: `fn check_sets() -> &'static [&'static dyn
CheckSetPlugin]` — the only file naming plugins.
- Acceptance: `u50_check`'s public `run(request)` produces
  declaration-ordered results for the example plugin; the grep test
  (no plugin names outside `checks/` + `registry.rs`) passes.

## Phase 4 — YAML simple checks + CLI wiring

- `yaml.rs`: parse `.cs50.yaml` `check50.checks` dict pipelines
  (`run`/`stdin`/`stdout`/`exit` in fixed order) and interpret them
  natively against the Phase-1 API (no Python compilation step);
  same validation errors (missing `run`, unknown command).
- CLI (`u50 check <slug-or-path>`): local + dev modes, `--target`,
  `-o ansi|json`, `--output-file`, exit code 1 when any check is not
  passed, mirroring `should_fail`.
- Acceptance: a `.cs50.yaml` sample drives the same runner; the CLI
  exit codes and JSON shape match check50's documented spec.

## Phase 5 — renderers

- `render/ansi.rs` + `render/json.rs` as plugins over the results model,
  byte-matching check50's `to_ansi`/`to_json` (including the
  `:)`/`:|`/`:(` forms and 4-space JSON).
- `render/html.rs` deferred (templating choice + the results.html port
  can be a follow-up; u50_style's html renderer patterns apply).

## Dependency direction

`checks/*` and `render/*` → core (`check.rs`, `result.rs`, `runner.rs`);
core never names a plugin — `registry.rs` is the single registration
point; `u50_check`'s public surface is `run(request)`, `CheckResult`,
and the plugin trait for future out-of-crate check sets.

## What is NOT ported

- Remote mode (`lib50.push` + submit.cs50.io polling), GitHub slug
  downloads, `--logout`, version-update checks
- gettext translations, `flask.py`, `py.py` (Python-runtime helpers),
  `c.py` valgrind (can be revisited as a per-problem plugin capability)
- YAML→Python compilation (superseded by native interpretation)

## Verification checklist

- [ ] `cargo test --workspace` — core API, graph, skip cascade, YAML
      interpreter, renderers
- [ ] `cargo clippy --workspace --all-targets -- -Dwarnings` — clean
- [ ] Golden-style fixtures: a sample check package drives identical
      JSON/ANSI output shapes to check50's documented spec
- [ ] Adding a throwaway check set = 1 module + 1 registry line
- [ ] No check-set names outside `checks/` + `registry.rs` (grep)
- [ ] CI green (both OS legs)
