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

## Phase 0 findings

(pending — record exact API calls, crate features, and gotchas here)

## API usage (for uv upgrades)

(pending — list each uv crate function/type u50 calls with its source file)

## Status log

- 2026-08-29: plan created and committed; Phase 0 starting.
