# u50_submit — submit50 reimplementation

Rust rewrite of [submit50](https://github.com/cs50/submit50): submits work to GitHub via git (uses `git2` with SSH support).

**Read the original first**: consult the [submit50 Python repo](https://github.com/cs50/submit50) before assuming CLI flags, output format, or submission behavior. Record findings here (below) so later agents don't have to re-read the Python source.

## Status

Stub engine: `run()` bails with a "not implemented yet" error; request type (`Request`) is defined and dispatched from the CLI. Concrete engine behavior is future work.

## Behavior notes

Findings recorded from the official docs: https://cs50.readthedocs.io/submit50/

### Usage

- Usage: `submit50 [-h] [--logout] [--log-level {debug,info,warning,error}] [-V] <slug>` where slug is the prescribed identifier of the work to submit (used as a git branch name).

### Mechanics

- Pushes the current directory's files to the GitHub org repo `me50/<username>` on branch = slug.
- Uses its own `GIT_DIR` in /tmp — ignores any local `.git`.
- Builds `.gitignore` from the ignore rules in `https://raw.githubusercontent.com/<slug>/.cs50.yml`.
- Exits WITHOUT submitting if required files are missing; prompts to confirm before submitting otherwise.
- Submitting manually = push work to branch = slug at `git@github.com:me50/<username>.git`.

### Auth

- HTTPS with GitHub username+password (cached in RAM via git-credential-cache, re-prompted at most weekly).
- Or SSH (`ssh.github.com` port 443) to avoid passwords entirely.

### Other

- `--logout` — log out.
- `-V`/`--version`.
- i18n via the `LANGUAGE` env var (e.g. `LANGUAGE=es submit50 problem`).

> u50 divergence (see `u50_cli/AGENTS.md`): adds `--yes`, `--ssh`, and `--dry-run` flags (the original has no non-interactive or dry-run mode). Slug format and the `me50/<username>` push target are kept identical (compat-sensitive).

Workspace-wide conventions (Rust edition 2024, workspace dependencies, clippy pedantic, CI gates) live in the root [AGENTS.md](../AGENTS.md) and are not repeated here.
