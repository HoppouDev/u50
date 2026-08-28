# u50_check — check50 reimplementation

Rust rewrite of [check50](https://github.com/cs50/check50): runs checks against student code.

**Read the original first**: consult the [check50 Python repo](https://github.com/cs50/check50) before assuming CLI flags, output format, or check semantics. Record findings here (below) so later agents don't have to re-read the Python source.

## Status

Stub engine: `run()` bails with a "not implemented yet" error; request types (`Request`, `Mode`, `Output`) are defined and dispatched from the CLI. Concrete engine behavior is future work.

## Behavior notes

Findings recorded from the official user docs: https://cs50.readthedocs.io/projects/check50/en/latest/check50_user/

### Usage and slug

- Usage: `check50 [flags] <slug>`.
- Slug = `org/repo/branch/path`, e.g. `cs50/problems/2018/x/caesar` (org=`cs50`, repo=`problems`, branch=`2018/x`, path=`caesar`).
- Checks live on GitHub; the tool is decoupled from the checks it runs.

### Operation modes (mutually exclusive in the original)

- **online** (default) — runs remotely, waits for results.
- `--local` — runs locally, fetches checks from GitHub.
- `--offline` — runs locally, reads checks locally, no remote fetch.
- `--dev` — developer mode for check authors; implies `--offline`.

### Output modes (`--output`/`-o`, repeatable and mixable)

- `ansi` — terminal text (default).
- `html` — self-contained static file written to /tmp; prints the path.
- `json` — machine-readable; prints to stdout by default.
- Default output shows **ansi+html**. `--output-file <path>` writes output to a file.

### JSON results schema

- Top-level object: `{slug, results[], version}`.
- Each result: `{name, description, passed (true/false/null), log[], cause {rationale, help}, data, dependency}`.
- Dependencies form a graph; a failed dependency cascades — downstream checks get `passed: null` with rationale `"can't check until a frown turns upside down"`.

### Other flags

- `--target <name>` — run only the named checks plus their dependencies.
- `--verbose` — show tracebacks.
- `--log` — show the log.
- `--log-level INFO|DEBUG` — show git commands run.

> u50 divergence (see `u50_cli/AGENTS.md`): the four mutually-exclusive boolean mode flags become a single `--mode <online|local|offline|dev>` enum. Slug formats and the JSON schema are kept identical (compat-sensitive).

Workspace-wide conventions (Rust edition 2024, workspace dependencies, clippy pedantic, CI gates) live in the root [AGENTS.md](../AGENTS.md) and are not repeated here.
