# Implementation plan: in-process Python provisioning via uv library crates

> **RECOVERY POINT**: if context is lost, re-read this document top to bottom,
> then `u50_style/AGENTS.md` (tool management + golden sections) and
> `git log --oneline -10`. The Status log at the bottom records what is done.
> Work tree state is always safe to check with `git status` + `git diff`.

## Decision record

- **Decision (user, 2026-08-29)**: use uv's published workspace member crates as
  **library dependencies pinned `=0.0.74`** for everything Python-related in
  `u50_style`, instead of shelling out to a `uv` binary or the system
  `python3`/`pip`. Goal: `u50 style --setup` provisions the pinned interpreter
  and all formatter backends **in-process** — no system python3/pip/uv needed.
- **Cadence directive (user, 2026-08-29)**: commit + push after **every**
  incremental change so any step can be reverted independently. This is a
  complex task; small commits are the recovery mechanism.
- Known trade-off (accepted by the user, evidenced in research): the uv
  workspace members are **internal APIs** — the `uv` crate README states they
  "are considered internal and will have frequent breaking changes"; docs.rs
  coverage is ~37% on `uv-python`; everything is 0.0.x (no semver). Mitigation:
  pin **exactly** `=0.0.74` (cargo treats `^0.0.74` as `=0.0.74` for 0.0.x),
  record every API call in this document, and treat upgrades as manual rework.

## Current state (baseline: HEAD 6f1db0e/dcf6d43 chain, origin/main a6dd917+)

- `setup.rs` `setup_missing()`: `python3 -m pip --version` probe →
  parallel `python3 -m pip download --dest <cache>/wheels <pkg==ver>` threads
  (indicatif spinners) → one `python3 -m pip install --no-index --find-links
  <cache>/wheels --target <cache>/python <specs>` → per-package verification
  via `locate_tool` → per-package summary lines. Exits 3 on failure.
- `formatter.rs`: `locate_tool(tool)` = PATH first, then `<cache>/python/bin`;
  `run_process` adds `PYTHONPATH=<cache>/python` for cache-origin hits (pip
  `--target` layout needs it for module imports); `cache_bin_dir()` =
  `<cache>/python/bin`.
- `PINNED_VERSIONS` (setup.rs) ↔ `tests/tool-versions.txt` (CI constraints),
  cross-checked by an `include_str!` unit test. Tools:
  clang-format==22.1.8 (platform wheel), autopep8==2.3.2, jsbeautifier==2.0.3,
  cssbeautifier==2.0.3, djhtml==3.0.6, sqlparse==0.5.3 (+ transitive deps:
  pycodestyle, editorconfig, …).
- CI (rust.yml): builds, then creates a **system-python venv**, pip-installs
  the pinned tools into it, puts it on PATH, runs `U50_STYLE_GOLDEN=1 cargo
  test -p u50_style --test golden --release`. Golden gate:
  `tool_available` = locate_tool (PATH→cache).

## Target state

- `u50_style` depends on uv member crates pinned `=0.0.74` (exact): at minimum
  `uv-python` (managed interpreter download/install + discovery),
  `uv-virtualenv` (venv creation), `uv-installer` (wheel installation),
  `uv-platform-tags`/`uv-pypi-types`/`uv-pep440` (wheel selection),
  `uv-client`/`uv-distribution-types`/`uv-requirements*`/`uv-resolver`
  (PyPI resolution/download) — final set discovered during the spike.
- `u50 style --setup` is **fully in-process**: no `python3`, no `pip`, no `uv`
  binary. It (1) provisions a **uv-managed CPython 3.14** into uv's managed
  dir, (2) creates the venv at `<cache>/venv` from that interpreter,
  (3) resolves/downloads the pinned tool wheels from PyPI, (4) installs them
  into the venv (uv-installer), (5) verifies + prints the same summary lines.
- Tool spawn after migration: cached console scripts live in
  `<cache>/venv/bin` with **absolute shebangs to the venv python** —
  self-contained, so `run_process` **drops the PYTHONPATH injection**.
  `locate_tool` order stays PATH-first-then-cache (PATH still wins if the
  user has system tools).
- CI: golden step dogfoods the new path — `cargo run -q --bin u50 -- style
  --setup` provisions everything, then `U50_STYLE_GOLDEN=1 cargo test --test
  golden` (the gate resolves tools from the cache via locate_tool).
- `tests/tool-versions.txt` remains the record of pinned **tool** versions
  (cross-checked against `PINNED_VERSIONS` by the include_str! test); the
  pinned **interpreter** version lives in `setup.rs` as `PINNED_PYTHON`.
- Runtime requirement after this change: **none** (no python3, no pip, no uv
  binary). First `--setup` needs network (PyPI + python-build-standalone
  downloads).

## Risks

- **R1 — API churn**: uv member crates are internal; 0.0.x pinning means
  upgrades are manual. Mitigation: record every uv-API call site in this doc
  (section "API usage") so an upgrade is a find-and-replace exercise.
- **R2 — dependency weight**: uv-client → reqwest/rustls; expect a large
  Cargo.lock diff and longer builds. Watch `target/release/u50` size
  (profile is opt-level "z" + lto).
- **R3 — resolver complexity**: full PyPI resolution via uv-resolver's
  internal API may be the hardest piece. Fallback: resolve the 6 pinned
  packages via PyPI's JSON API (`https://pypi.org/pypi/<pkg>/<ver>/json`) —
  simple HTTPS + platform-tag matching via `uv-platform-tags` — then install
  with `uv-installer`. Transitive deps (pycodestyle, editorconfig) must be
  included in the resolution.
- **R4 — Windows console scripts**: uv uses trampolines
  (`uv-trampoline-builder`). CI is Linux; defer Windows to a follow-up.
- **R5 — MSRV**: uv workspace rust-version = 1.96.0; local/CI toolchain is
  1.97.1 ✓.
- **R6 — feature flags**: some uv crates gate functionality behind features
  (e.g. uv-python `schemars`); enable only what compilation requires.

## Phases (each phase = at least one commit, pushed immediately)

- **Phase 0 — SPIKE (current)**: add the uv member deps `=0.0.74`; in a
  scratch example prove the three hard primitives:
  (a) uv-python downloads/installs a managed CPython 3.14 and reports its
      location,
  (b) uv-virtualenv creates a venv from that interpreter,
  (c) uv-installer installs a downloaded wheel into the venv and the console
      script runs.
  Record the exact API calls under "Phase 0 findings" below. The deps-only
  commit goes in FIRST (so the spike has something to build against); if the
  APIs prove unusable, revert the deps commit and re-evaluate (binary route
  remains the fallback).
- **Phase 1**: rewrite `setup_missing()` on the proven primitives (remove
  python3/pip subprocesses; keep missing_backends helper + tests; keep
  spinners/summary lines); simplify `run_process` (drop PYTHONPATH);
  `cache_bin_dir` → `<cache>/venv/bin`.
- **Phase 2**: CI rust.yml — golden step dogfoods `u50 style --setup`;
  remove the system-python venv step.
- **Phase 3**: docs (AGENTS.md tool-management rewrite: uv library
  provisioning, pinned interpreter, no system python; uv binary no longer
  needed; cache layout change; stale `<cache>/python` migration note).
- **Phase 4**: cleanup sweep — dead code, clippy, final gate run, goldens.

### Phase 0 findings — ALL THREE PRIMITIVES PASS (verified end-to-end)

- **(a) Managed CPython provisioning** (mirrors uv's own
  `crates/uv/src/commands/python/install.rs`):
  1. `BaseClientBuilder::default()` → capture `.retry_policy()` BEFORE
     `.retries(0).build()` (GOTCHA: builder methods consume self).
  2. `ManagedPythonDownloadList::new(&client_builder, &cache, None).await?`
     → `.find(&PythonDownloadRequest::default().with_version(
     VersionRequest::from_str("3.14")?).fill()?)?` → clone.
  3. `ManagedPythonInstallations::from_settings(None)?.init()?` → root(),
     scratch(), `lock().await` held for fetch+unpack.
  4. `download.fetch_with_retry(&client, &retry_policy, &root, &scratch,
     false, None, None, None).await?` → `DownloadResult::Fetched(path)`.
  5. `ManagedPythonInstallation::new(path, &download).executable(false)` →
     `Interpreter::query(&exe, &cache)?`.
  Result: `cpython-3.14.7-linux-x86_64-gnu` provisioned into
  `~/.local/share/uv/python/`, queryable.
- **(b) `uv_virtualenv::create_venv(location, interpreter, Prompt::Static(..),
  system_site_packages, OnExisting::Allow, relocatable, Seed::Disabled,
  upgradeable)`** (bool order verified against vendored lib.rs:80) →
  `PythonEnvironment` (`.root()`, `.python_executable()`, `.scripts()`).
- **(c) `uv_installer::Installer::new(&PythonEnvironment, Preview::default())
  .with_cache(&cache).with_installer_metadata(false).install_blocking(dists)`
  with `CachedDist::Url(CachedDirectUrlDist { filename, url: VerbatimParsedUrl,
  path, hashes: HashDigests, cache_info, build_info })` built from the PyPI
  JSON API wheel entry (reqwest direct GET — uv-client exposes no plain-GET).
- Managed pythons present after the run: 3.14.7, 3.12.13, 3.8.20.

### API usage (for uv upgrades)

All call sites live in `u50_style/examples/uv_spike.rs` (the reference
implementation for Phase 1's setup.rs rewrite):

- `uv_python::downloads::{ManagedPythonDownloadList, PythonDownloadRequest,
  DownloadResult}` — `new(&BaseClientBuilder, &Cache, None)`, `.find()`,
  `fetch_with_retry(...)`.
- `uv_python::managed::{ManagedPythonInstallations, ManagedPythonInstallation}`
  — `from_settings`, `init`, `lock`, `root`, `scratch`, `new`, `executable`.
- `uv_python::{Interpreter, PythonEnvironment, VersionRequest}` —
  `Interpreter::query`, `PythonEnvironment::from_interpreter` (via create_venv).
- `uv_virtualenv::create_venv` (8-arg form; bool order is load-bearing).
- `uv_installer::Installer` — `new`, `with_cache`, `with_installer_metadata`,
  `install_blocking`.
- `uv_cache::Cache` — `from_path(...).init()`.
- `uv_preview::set(Preview::default())` — global init, REQUIRED first.
- `uv_extract::unzip` + `uv_distribution_filename::WheelFilename` +
  `uv_distribution_types::{CachedDirectUrlDist, CachedDist}` +
  `uv_pypi_types::{HashDigest, HashDigests, ParsedUrl, VerbatimParsedUrl}` +
  `uv_pep508::VerbatimUrl` + `uv_redacted::DisplaySafeUrl`.

GOTCHAS: (1) `uv_preview::set(...)` required before any uv API (panic:
"preview configuration has not been initialized"). (2) `install_blocking`
requires an UNZIPPED wheel archive dir (reads `<prefix>.dist-info/WHEEL` from
`dist.path()`) — download the .whl then `uv_extract::unzip(fs_err::File,
&archive_dir)` (uv's archive-v0 layout). (3) `BaseClientBuilder` methods
consume self — capture `retry_policy()` before building. (4)
`install_blocking` rejects symlink link-mode with a temporary cache — use
`Cache::from_path` (persistent). (5) uv-python APIs are async (tokio).

Reference implementation: `u50_style/examples/uv_spike.rs` — the template for
Phase 1's setup.rs rewrite.

## Status log

- 2026-08-29: plan created and committed; Phase 0 starting.
- 2026-08-29 (late): deps commit 32cb133; spike example 09da6e0; Phase 0
  COMPLETE — all three primitives verified end-to-end. Phase 1 next.
