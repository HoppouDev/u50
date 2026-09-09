# check50 3.x — Source Notes for the u50_check Port

Source studied: [cs50/check50](https://github.com/cs50/check50) `main`,
local snapshot at `tmp/check50-repo/` (shallow clone). ~2,300 lines of
Python across 14 modules plus docs and a test suite of sample check
packages. This document records how check50 actually works, module by
module, as the reference for the u50_check port. (Notes and paraphrase
only — no upstream code is copied; check50 is AGPL-3.0.)

## 1. Module map

| Module                   | LOC | Role                                                                                                                        |
| ------------------------ | --- | --------------------------------------------------------------------------------------------------------------------------- |
| `__init__.py`            | 44  | Public API exports; gettext setup; version                                                                                  |
| `_api.py`                | 519 | The check-authoring API (the "language" checks are written in)                                                              |
| `runner.py`              | 406 | `@check` decorator, `CheckResult`, dependency-graph runner                                                                  |
| `__main__.py`            | 447 | CLI, local/remote modes, output rendering dispatch, exit code                                                               |
| `internal.py`            | 190 | Module-level state (dirs, slug), `.cs50.yaml` config loader, hook registry (`Register`), YAML compiler entry, `import_file` |
| `c.py`                   | 139 | C helper: `compile()` (clang) + `valgrind()`                                                                                |
| `flask.py`               | 178 | Flask app testing helper                                                                                                    |
| `_simple.py`             | 114 | Compiles "simple" YAML checks to Python check files                                                                         |
| `py.py`                  | 66  | Python helper: `append_code`, `import_`, `compile`                                                                          |
| `_exceptions.py`         | 84  | `Error`, `RemoteCheckError`, `ExceptHook` (JSON/ANSI error rendering, exit 1)                                               |
| `regex.py`               | 33  | `decimal(number)` — exact-number regex builder                                                                              |
| `contextmanagers.py`     | 7   | `nullcontext` shim                                                                                                          |
| `renderer/_renderers.py` | 48  | `to_ansi` / `to_json` / `to_html` (jinja2 `results.html` template)                                                          |

Python dependencies: `lib50` (the heavy lifter: slug→repo resolution,
config loading, temp "working areas", git push, progress bars),
`pexpect` (PTY process spawning), `attr`, `jinja2`, `termcolor`,
`requests`, `beautifulsoup4`, `packaging`.

## 2. The authoring model

Two ways to author checks, both ultimately becoming **Python functions**
in an imported module:

### a) Python checks (the real model)

```text
import check50

@check50.check()                # no dependency
def exists():
    """hello.c exists"""       # <- docstring IS the user-visible description
    check50.exists("hello.c")

@check50.check(exists)          # depends on `exists`
def compiles():
    """hello.c compiles"""
    check50.c.compile("hello.c")

@check50.check(compiles)
def prints_hello():
    """prints "Hello, world!\\n"""
    check50.run("./hello").stdout("[Hh]ello, world!?\\n", "hello, world\\n").exit()
```

### b) "Simple" YAML checks (`.cs50.yaml`)

```yaml
check50:
  checks:
    hello:
      - run: python3 hello.py
        stdout: Hello, world!
        exit: 0
```

`_simple.py` compiles these dict pipelines (`run` → `stdin` → `stdout` →
`exit`, in that fixed order) into the equivalent decorated Python
functions at `internal.compile_checks` time.

### c) `.cs50.yaml` top-level keys

`checks` (default `__init__.py`; a string naming the checks file, or a
dict of simple checks), `dependencies` (pip requirements installed
before the run), `translations` (gettext), plus lib50's scoped keys
`files`/`include`/`exclude`/`require` controlling which student files
copy into the run area.

## 3. The runtime model (the part that matters most)

1. **Slug resolution** — local/dev mode uses the slug as a directory
   path; otherwise lib50 downloads the checks repo from GitHub into
   `~/.local/share/check50` (`--local`/`--offline` flag matrices).
2. **Working area** — `lib50.working_area(included_files)` creates a
   **temp directory**, copies the student files selected by the config's
   `files` scope into it, and `cd`s there. `internal.run_root_dir` is
   that temp dir's parent; `internal.student_dir` remembers the original
   cwd (restored on exit).
3. **Module import & registration** — the checks module is imported
   once; the `@check` decorator appends names to a module-global
   `_check_names` list **in declaration order**, and records each
   check's dependency. `inspect.getmembers` then extracts the checks and
   builds the **dependency graph** (`dependency -> set(dependents)`,
   rooted at `None`).
4. **Concurrent execution** — `CheckRunner.run(targets)`:
   - optionally builds a minimal subgraph for `--target`
   - submits all dependency-free checks to a `ProcessPoolExecutor`
   - as checks pass, their dependents are submitted **with the
     dependency's returned state**
   - failures/skips cascade: `_skip_children` marks every transitive
     dependent `passed=None` with the cause
     `"can't check until a frown turns upside down"`
   - results are emitted in **declaration order**
5. **Per-check isolation & inheritance** — each check gets its own
   `run_dir = run_root_dir / check_name`, created by **copytree from the
   dependency's run_dir** (`run_root_dir / dependency_name` or `/ -` for
   dependency-free checks), then `os.chdir` into it. This is how a
   `compiles` check makes its binary available to every check that
   depends on it.
6. **Timeout & errors** — SIGALRM-based per-check timeout (default 60 s
   → `Timeout(Failure)`); `Failure` → `passed=false` with `cause.payload`;
   any other exception → `passed=None` (rendered as a skip with an error
   payload); registration hooks (`Register`) run before/after each check.

## 4. The check-authoring API (`_api.py`)

- **`run(command, env)`** — spawns via **pexpect** (`bash -c <shlex-quoted
command>`; PTY so programs see a tty), returns a chainable builder:
  - `.stdin(line | EOF, str_line, prompt=True, timeout=3)` — logs,
    optionally absorbs a **prompt** (expects `.+` output first, else
    `Failure("expected prompt for input, found none")`), sends line or EOF
  - `.stdout(output | None, str_output, regex=True, timeout=3)` — with
    `output=None` returns all output (CRLF→LF, lstripped); with a value:
    regex by default, exact-match with `regex=False`, numeric values
    match via `regex.decimal`, EOF supported; `Failure` on invalid UTF-8,
    `Mismatch(expected, actual)` on mismatch, `Missing` on timeout
  - `.reject(timeout=1)` — asserts the program survives without consuming
    input (i.e. it rejected the input)
  - `.exit(code=None, timeout=5)` — waits for exit (SIGSEGV → explicit
    `Failure`), asserts the exit code, or returns it
  - `.kill()` — SIGHUP→SIGINT→SIGKILL escalation
- **`exists(*paths)`**, **`hash(file)`** (SHA-256), **`include(*paths)`**
  (copy from check dir into the run dir), **`log(line)`**, **`data(**kw)`\*\*
- **`Failure(rationale, help=None)`**, **`Missing(item, collection)`**,
  **`Mismatch(expected, actual)`** — all carry a `.payload` dict that
  becomes the result's `cause`; `Mismatch` produces the
  `"expected X, not Y"` rationale with repr-truncation (`_raw`, 15 chars)
- **`hidden(rationale)`** decorator — suppresses the log, converts any
  `Failure` into a generic one (for answer-key checks)
- **`import_checks("../less")`** — imports another checks module so check
  sets can extend each other

## 5. Results model and output

`CheckResult` = `{name, description, passed(bool|None), log[], cause,
data{}, dependency}`. `passed=None` means skipped. Log is truncated to
`max_log_lines` (100) with a `"..."` head marker.

JSON output (documented in `docs/source/json_specification.rst`):
`{slug, results[], version}`; `cause` is `null` iff passed, otherwise
`{rationale, help?, error?{type,value,traceback,data}}`; errors replace
`results` with an `error` key.

Renderers: `to_ansi` (`:)`/`:|`/`:( description` lines, green/yellow/red,
optional log), `to_json` (4-space indent), `to_html` (jinja2
`results.html`). CLI `-o` defaults to **both `ansi` and `html`**; the
html output goes to a temp file (opened in a browser; WSL/CS50-IDE
special cases). Exit code: `1` if any result is not passed or an error
occurred, else `0`.

## 6. CLI surface (`__main__.py`)

`check50 <slug> [-d/--dev | --offline | -l/--local] [-o ansi|json|html ...]
[--target ...] [--output-file FILE] [--log-level ...] [--ansi-log]
[--no-download-checks] [--no-install-dependencies] [-V] [--logout]`.
Remote mode (the default without `--local`) pushes to GitHub and polls
`submit.cs50.io` — explicitly out of scope for the u50 port.

## 7. Things the port must decide (recorded for the plan)

1. **Check authoring**: check50's checks are Python functions; u50_check
   cannot run Python. The port's equivalent: checks declared natively in
   Rust behind a plugin trait (one module per problem/check set, like
   u50_style's language plugins), plus a native interpreter for the
   "simple" YAML check pipelines (`run/stdin/stdout/exit`), which need no
   Python at all.
2. **Process model**: check50 spawns each check in a separate OS process
   (isolation + SIGALRM timeout + copytree filesystem inheritance). The
   port needs process-level isolation for the same reasons (a crashing
   student program must not take down the runner; filesystem inheritance
   requires real directories).
3. **lib50 responsibilities**: slug→directory resolution, temp working
   areas with scoped file copies, config loading. The port replaces
   GitHub slug downloads with local paths (dev mode) and its own config
   loader; `lib50.push`/remote results are out of scope.
4. **pexpect responsibilities**: PTY spawn, prompt absorption, regex and
   EOF matching, CRLF normalization, exit-status/SIGSEGV detection. Rust
   equivalent: a PTY crate or pipe-based spawn with the same matching
   semantics (this is where behavioral parity with check50's
   prompt/EOF semantics lives — needs careful tests).
5. **gettext/i18n, remote results, WSL/CS50-IDE html special cases**:
   documented divergences — out of scope for the port.
6. **Renderer**: same three output modes; ansi and json are the ports'
   first targets, html can reuse the u50_style html patterns later.
