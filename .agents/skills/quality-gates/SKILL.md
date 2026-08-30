---
name: quality-gates
description: >-
  Use when verifying, committing, or shipping any change to u50: runs the
  fmt/clippy/build/test/golden gate ladder, interprets exit codes, and only
  then commits and pushes.
---

# Quality gates

Run every gate below, in order. All gates must pass **before** any commit;
never weaken a gate to make it green.

## The gate ladder

```sh
cargo fmt --all -- --check                              # 1. formatting
cargo clippy --workspace --all-targets -- -Dwarnings    # 2. lints (pedantic, zero warnings)
cargo build                                             # 3. compiles
cargo test --workspace                                  # 4. unit + integration tests
U50_STYLE_GOLDEN=1 cargo test -p u50_style --test golden  # 5. golden fixtures
```

What each gate catches:

1. **fmt** — non-rustfmt-clean code; CI runs the same check, so local drift
   fails the pipeline.
2. **clippy** — lint violations; pedantic is enabled crate-wide
   (`#![warn(clippy::pedantic)]`) and `-Dwarnings` makes any warning fatal.
   Msrv-gated lints read `msrv = "1.96.0"` from the root `clippy.toml`.
3. **build** — compile errors the test run would also hit, but earlier and
   with clearer output.
4. **test** — unit and integration tests for all workspace crates.
5. **golden** — u50's formatter output must stay byte-identical to the
   style50-oracle fixtures. Runs only when `U50_STYLE_GOLDEN=1` is set AND
   the backing tool is found in the u50 style cache (cache-only resolution;
   the system `PATH` is never consulted). If a language prints `skip`, run
   `cargo run --bin u50 -- --setup` once and re-run.

## Exit-code semantics (u50 style)

- **0** — clean: no violations; or plain `--fix` succeeded; or a check that
  found nothing to do.
- **1** — violations found; or a `--fix --dry-run` that would fix at least
  one file (this is the *success* signal for dry runs).
- **2** — usage error (clap flag conflicts, unknown flags).
- **3** — per-file or infrastructure error (unreadable file, unsupported
  extension, missing/failing formatter, provisioning failure). Takes
  precedence over 1.

A golden or provisioning test failing with exit 3 usually means the cache is
empty or stale — check `u50 style --list` (reports `found (cache)` or
`missing`; it never provisions).

## Smoke-testing provisioning changes

Any change to tool provisioning or cache resolution must be smoke-tested in
isolation so a warm cache or a hostile `PATH` cannot mask a bug:

```sh
CACHE=$(mktemp -d)                        # empty cache
mkdir -p /tmp/u50smokebin && ln -sf "$(command -v u50)" /tmp/u50smokebin/u50
env -i HOME="$HOME" XDG_CACHE_HOME="$CACHE" PATH=/tmp/u50smokebin \\
  /tmp/u50smokebin/u50 style --list       # every backend must read "missing"
env -i HOME="$HOME" XDG_CACHE_HOME="$CACHE" PATH=/tmp/u50smokebin \\
  /tmp/u50smokebin/u50 style some-file.c  # auto-provisions on first use, then formats
env -i HOME="$HOME" XDG_CACHE_HOME="$CACHE" PATH=/tmp/u50smokebin \\
  /tmp/u50smokebin/u50 style --list       # now "found (cache)"
```

Each line is a single `env -i` invocation: there is no subshell that could
escape the minimal environment, the bare restricted `PATH` carries only the
`u50` symlink, and `$CACHE` pins the empty cache across invocations.


Verified property: a fake same-named binary planted on `PATH` is never used —
resolution is cache-only, and an empty cache auto-provisions exactly the
needed backend on first use.

## Gate-to-ship rule

All gates green → conventional commit (`feat`/`fix`/`refactor`/`docs`/`chore`)
→ push → **watch the CI run** (`gh run watch` or the Actions page) and fix
failures until green. CI running is not the same as CI passing.
