# u50_check — check50 reimplementation

Rust rewrite of [check50](https://github.com/cs50/check50): runs checks against student code.

**Read the original first**: consult the [check50 Python repo](https://github.com/cs50/check50) before assuming CLI flags, output format, or check semantics. Record findings here (below) so later agents don't have to re-read the Python source.

## Status

Implemented (see `docs/U50_CHECK_PLUGIN_PLAN.md` and `docs/CHECK50_PORT_NOTES.md` for the port plan and source notes). Local/offline/dev modes run checks from a `.cs50.yaml`-rooted check directory (the slug is a local path — online/remote mode is a documented divergence and still bails with an error). Two ways to author checks, both compiled into `CheckSpec`s:

- **"Simple" YAML checks** (`check50.checks` as a dict in `.cs50.yaml`): interpreted natively (`yaml.rs`) against the check API — no Python, no compilation step.
- **Native check-set plugins** (`CheckSetPlugin`, one module per problem under `checks/`, registered in `registry.rs` — the only core file that names a plugin; see `docs/U50_CHECK_PLUGIN_PLAN.md`): looked up by id when `.cs50.yaml` names a native checks file. `checks/hello.rs` is the authoring template.
- **Legacy Python checks** (`__init__.py`/`check.py` — the cs50/problems model, Phases 0-2 of `docs/U50_CHECK_PYTHON_PLAN.md`): a `checks:` string naming a `.py` file routes through `python/` — the check venv (`<cache>/u50/check50/venv`, uv-managed CPython + the shipped `check50` package staged into site-packages, provisioned on first use, cache-only), a discovery pass (registry JSON -> `CheckSpec`s in declaration order), and one subprocess per check (own process group; the runner kills the whole group on timeout; the dependency's pickled return value passes via a state file in the run dir). Non-Failure Python exceptions map to `Cause::Error`.
- **Still divergences** (Phases 6-8 pending): pip `dependencies:` (parsed but ignored), `check50.flask`, `check50.c.valgrind` (a check set using them fails at the Python layer with a clear error).
- **u50_tools** (`u50_tools/` shared crate): the resolver plugin system (4 plugins: uv/toolchain/system/download) + shared fs/proc helpers, consumed by u50_check for venv provisioning + kill/copy. style50 pipeline extraction deferred to a follow-up.

The engine (`api.rs`, `graph.rs`, `runner.rs`) reproduces check50's runtime model: a chainable `run().stdin().stdout().exit()` builder with prompt absorption, EOF, regex/exact/decimal matching, and SIGSEGV detection; per-check thread isolation with filesystem inheritance (copytree from the dependency's run dir) and per-check timeouts; declaration-order results; and the failure skip cascade (`"can't check until a frown turns upside down"`). `render/ansi.rs` and `render/json.rs` render check50's documented output shapes; `html` output is deferred (Phase 5 of the plugin plan).

## Behavior notes

Findings recorded from the official user docs: <https://cs50.readthedocs.io/projects/check50/en/latest/check50_user/>

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
