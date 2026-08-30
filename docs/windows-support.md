# Windows Support Plan

Status: implemented — phases 0–4 complete. Scope: build/test/run on Windows; style engine is the only implemented program, check/submit are stubs. Gates (`cargo fmt --check`, `clippy --workspace --all-targets -- -Dwarnings`, `cargo test --workspace`) are green on Linux (WSL on this machine) and `--setup` / `--status` / `u50 style` were verified end-to-end here (6/6 backends provisioned, exit 0). Windows-native gates run on the CI `windows-latest` leg — first dual-OS CI run pending.

## Goal

The four gates (cargo build, test, fmt --check, clippy -- -Dwarnings) pass on ubuntu-latest AND windows-latest; u50 --setup + u50 --status + u50 style work end-to-end on Windows via the uv-managed venv cache (no system Python, no PATH lookups). Byte-level golden parity is preserved (engine already normalizes CRLF on input, engine.rs:44).

## Non-goals (deferred)

- u50_submit SSH plumbing (Windows OpenSSH agent uses a named pipe, not SSH_AUTH_SOCK; git2/libssh2 on MSVC unverified) - revisit when submit is implemented. Note u50_submit/AGENTS.md:21 plans a GIT_DIR under /tmp - must use std::env::temp_dir() instead; same for /tmp in u50_check/AGENTS.md:30.
- TTY detection / legacy conhost VT enablement (pre-existing gap on Linux too).
- aarch64-pc-windows-msvc (untested, nothing blocks it).

## Verified findings (with file:line)

### Hard blockers

1. Does not compile on Windows: u50_style/src/formatter.rs:69-75 is_executable_file() imports std::os::unix::fs::PermissionsExt unconditionally (confirmed E0433 from rustc on Windows).
2. Cache dir has no Windows base: formatter.rs:45-57 cache_dir() knows only XDG_CACHE_HOME / HOME/.cache and falls back to a relative .cache. XDG_CACHE_HOME override must keep working (u50_cli/tests/style_output.rs sets it at :63, :289, :365, :453, :497; u50_style/src/tests.rs:829 asserts only the cache-path suffix).
3. venv layout is bin/-only. Windows uv venvs use venv\Scripts with .exe shims. Two sites: formatter.rs:61-64 cache_bin_dir() returns venv/bin and locate_tool appends no .exe; setup.rs:448-477 ensure_venv() probes venv/bin/python and venv/bin/python3, so on Windows a healthy venv is misread as broken and remove_dir_all is called on every run.
4. Wheel ranking rejects Windows wheels: setup.rs:72-77 ranks only any/manylinux/linux_* (wheel_rank at setup.rs:144-161); clang-format ships only platform wheels, so provisioning installs nothing on Windows. Unit test setup.rs:940-953 asserts win_amd64 is rejected; must flip.
5. Path detection is /-only: formatter.rs:87 tool.contains('/') misclassifies C:\tools\tool.exe and .\tool as bare cache-only names.

### Likely changes

6. Tests are Unix-only: u50_cli/tests/style_output.rs has ZERO cfg gates - PermissionsExt at :38, :244, :340, :429 (test-target compile blocker), set_mode/chmod at :51, :265, :274, :447, symlink("/usr/bin/cat") at :261, #!/bin/sh stub backends at :18-23. Also verify u50_style/src/tests.rs:676-690 symlink-walk test on Windows (partially gated at :681).
7. CI is ubuntu-only (.github/workflows/rust.yml:14); the U50_STYLE_GOLDEN=1 inline env prefix (rust.yml:26) breaks under pwsh (Windows default shell). Golden dogfood pins via u50_style/tests/tool-versions.txt (golden.rs:15-19).
8. Docs assume ~/.cache: u50_style/AGENTS.md:80 (cache location), u50_style/AGENTS.md:134-135 and formatter.rs:103-105 (console scripts carry absolute shebangs - on Windows they are .exe shims; needs a Windows note), README.md:31, root AGENTS.md:42.

### Probably fine (verified, no change)

uv stack (uv-windows crate already in dep tree), tokio/clap/anyhow/reqwest+rustls/fs-err/similar, process spawning (formatter.rs:112-140, std::process::Command on a resolved absolute path), CRLF handling (engine.rs:44; fs::write never translates line endings), symlink-walk logic (symlink_metadata at engine.rs:216,243), ANSI colors under Windows Terminal, stub crates (u50_check/src/lib.rs:47-50, u50_submit/src/lib.rs:26-30). No signal handling, no lock files, no direct libc/nix dependency or unix-only API usage in u50's own code (transitive libc/nix exist in Cargo.lock).

## Phases

### Phase 0 - make it build (blockers 1, 5) — complete

cfg-split is_executable_file (unix keeps exec-bit check; windows accepts any existing regular file). locate_tool: portable explicit-path detection (backslash, drive letter, parent components). Gate: cargo check --workspace green on Windows.

### Phase 1 - cache + venv + wheels (blockers 2, 3, 4) — complete

Verified on this machine (Linux): `u50 --setup` provisioned CPython 3.14 + all 6 backends, `u50 --status` reports `found (cache)` for all 8 languages, and `u50 style` formats a sample file (exit 1 with violations, exit 0 clean). Windows-path behaviour is covered by the flipped unit tests (`win_amd64`/`win_arm64`/`win32` ranking, `Scripts`/`.exe` naming); the windows-latest CI leg exercises the real Windows paths.

cache_dir(): keep XDG_CACHE_HOME override; Windows uses LOCALAPPDATA-based base (dirs::cache_dir() equivalent) -> <base>\u50\style50; never fall back to a relative path - error instead. One platform-aware helper for venv bin dir (Scripts on Windows, bin elsewhere) and tool file name (.exe suffix on Windows) used by cache_bin_dir(), locate_tool(), and ensure_venv() (probe Scripts\python.exe on Windows). wheel_rank(): add win_amd64/win32/win_arm64 ranks keyed on std::env::consts::OS/ARCH, reject foreign-platform tags; flip the unit tests at setup.rs:940-953 and u50_style/src/tests.rs:833 (cache_bin_dir venv/bin assertion). Gate: u50 --setup provisions CPython + all 6 backends on Windows; u50 --status all found (cache); u50 style works on a sample file.

### Phase 2 - CI (item 7) — complete

`strategy.matrix.os: [ubuntu-latest, windows-latest]` added; `U50_STYLE_GOLDEN=1` moved into the step `env:` block (pwsh-safe); harness gate moved to a dedicated ubuntu-only `harness` job. A green dual-OS CI run is pending.

Status: implemented-pending-CI-verification (.github/workflows/rust.yml). Added strategy.matrix.os [ubuntu-latest, windows-latest] with runs-on: ${{ matrix.os }}; moved U50_STYLE_GOLDEN=1 into the golden step's env: block (an inline `VAR=1 cmd` prefix is bash-only and breaks under pwsh). Golden policy: run goldens on both OSes (CRLF normalized in-engine, engine.rs:44). The harness gate is now a dedicated ubuntu-only `harness` job using `paladini/harness-score@v1` with `min-level: '4'` and a self-updating badge published to the `badges` branch (replacing the `npx` step that ran in the matrix).

If golden drift appears on Windows (backend tool output differing per platform), relax the golden step to ubuntu-only by adding `if: runner.os == 'Linux'` to the golden tests step - do not flip tool-versions.txt per OS. Gate: workflow green on both OSes.

### Phase 3 - tests (item 6) — complete

All Unix-only tests in `u50_cli/tests/style_output.rs` are `#[cfg(unix)]`-gated (chmod/exec-bit/symlink/sh-stub); `u50_style/src/tests.rs:682` symlink use is inside `#[cfg(unix)]`. Full `cargo test --workspace` green on Linux (103 tests).

cfg-gate the Unix-only tests in u50_cli/tests/style_output.rs #[cfg(unix)] (chmod/exec-bit/symlink/sh-stub tests), keeping every test running on Linux; Windows spawn covered via the real provisioned venv rather than .cmd stubs. Verify u50_style/src/tests.rs:676-690 on Windows. Gate: full cargo test green on both OSes.

### Phase 4 - docs (item 8) — complete

`u50_style/AGENTS.md` (cache base per platform, `Scripts\` `.exe` shims, shebang note, wheel platform ranking), `README.md` (cache path, dual-OS CI, Windows roadmap item checked), root `AGENTS.md` (cache path, CI matrix, clippy `--all-targets`), this file.

Update u50_style/AGENTS.md (:80, :134-135), README.md:31, root AGENTS.md:42: cache is <XDG_CACHE_HOME|~/.cache>/u50/style50 on Unix, LOCALAPPDATA-based u50\style50 on Windows; note Windows support and dual-OS CI.

### Deferred

git2/libssh2 MSVC + named-pipe ssh-agent; /tmp contracts in u50_check/AGENTS.md:30, u50_submit/AGENTS.md:21, u50_style/examples/uv_spike.rs:34-35 (use std::env::temp_dir() when built). Note: .agents/skills/quality-gates/SKILL.md:63-73 smoke-test scripts are Unix-only (repo tooling, not shipped code).

## Acceptance criteria

1. cargo check/test/fmt --check/clippy -- -Dwarnings green on Windows (this machine) and ubuntu CI.
2. u50 --setup + u50 --status + u50 style verified end-to-end on Windows.
3. No Unix-only APIs outside #[cfg(unix)] gates.
4. CI workflow green on windows-latest.
