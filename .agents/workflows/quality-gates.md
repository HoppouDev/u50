# Workflow: quality gates

On-demand entry point for the `quality-gates` skill. Run these steps in order;
every step must pass before committing.

```sh
# 1. Formatting — must be rustfmt-clean
cargo fmt --all -- --check

# 2. Lints — pedantic, zero warnings tolerated
cargo clippy --workspace --all-targets -- -Dwarnings

# 3. Build
cargo build

# 4. Tests (all crates)
cargo test --workspace

# 5. Golden fixtures (needs the u50 style cache; see skill for skip semantics)
U50_STYLE_GOLDEN=1 cargo test -p u50_style --test golden
```

Interpreting `u50` exit codes: `0` clean · `1` violations or dry-run
would-fix · `2` usage error · `3` per-file/infra error (takes precedence).

After all five gates pass:

```sh
git commit -m "<type>: <summary>"   # conventional: feat|fix|refactor|docs|chore
git push origin main
gh run watch                        # or the Actions page — CI must finish green
```

If gate 5 reports `skip` per language, the cache lacks the backend:
`cargo run --bin u50 -- --setup`, then re-run. Full details and the
provisioning smoke-test: `.agents/skills/quality-gates/SKILL.md`.
