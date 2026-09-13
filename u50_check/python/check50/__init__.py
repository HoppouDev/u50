"""The check50 authoring API, shipped by u50.

A faithful port of check50 3.x's `_api.py` authoring surface (see
docs/CHECK50_PORT_NOTES.md), executed on u50's provisioned CPython.
Checks import this as `import check50`.
"""

from __future__ import annotations

import codecs
import contextlib
import hashlib
import os
import re as _re
import shutil
import subprocess
import sys
import threading
import time
from typing import Any, Callable

from . import c, py, regex
from . import bridge_state as state
from .errors import EOF, Eof, Failure, Mismatch, Missing

__all__ = [
    "EOF", "Failure", "Mismatch", "Missing", "c", "check", "data",
    "exists", "hash", "hidden", "include", "log", "regex", "run",
]


def check(dependency: Callable[..., object] | str | None = None, *, timeout: float | None = None) -> Callable[..., object]:
    def decorator(fn: Callable[..., object]) -> Callable[..., object]:
        dep = dependency
        if dep is not None and not isinstance(dep, str):
            dep = dep.__name__
        state.register(fn, dep, timeout)
        return fn
    if callable(dependency) and getattr(dependency, "__name__", "") not in state.checks:
        fn, dep = dependency, None
        return decorator(fn)  # type: ignore[arg-type]
    return decorator


def hidden(rationale: str) -> Callable[[Callable[..., object]], Callable[..., object]]:
    def decorator(fn: Callable[..., object]) -> Callable[..., object]:
        setattr(fn, "_check50_hidden", rationale)
        entry = state.checks.get(fn.__name__)
        if entry is not None:
            entry.hidden = rationale
        return fn
    return decorator


def import_checks(path: str) -> Any:
    import importlib.util
    import inspect
    frame = inspect.stack()[1]
    base = os.path.dirname(os.path.abspath(frame.filename))
    target = os.path.normpath(os.path.join(base, path))
    if os.path.isdir(target):
        target = os.path.join(target, "__init__.py")
    elif not target.endswith(".py"):
        target += ".py"
    spec = importlib.util.spec_from_file_location(
        f"check50.imported.{os.path.basename(target)[:-3]}", target
    )
    if spec is None or spec.loader is None:
        raise Failure(f"could not import checks from {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def log(line: str = "") -> None:
    state.log(line)


def data(**kwargs: Any) -> None:
    state.payload.update(kwargs)


def exists(*paths: str) -> None:
    for path in paths:
        log(f"checking that {path} exists...")
        if not os.path.exists(path):
            raise Failure(f"{path} not found")


def _copy(src: str, dst: str) -> None:
    try:
        if os.path.isdir(src):
            shutil.copytree(src, dst, dirs_exist_ok=True)
        else:
            parent = os.path.dirname(dst)
            if parent:
                os.makedirs(parent, exist_ok=True)
            shutil.copy(src, dst)
    except Exception as error:
        raise Failure(f"could not copy {src}", help=str(error)) from None


def include(*paths: str) -> None:
    check_dir = os.environ.get("CHECK50_CHECK_DIR", "")
    for path in paths:
        _copy(os.path.join(check_dir, path), path)


def hash(file: str) -> str:
    exists(file)
    log(f"hashing {file}...")
    digest = hashlib.sha256()
    try:
        with open(file, "rb") as handle:
            for chunk in iter(lambda: handle.read(65536), b""):
                digest.update(chunk)
    except OSError as error:
        raise Failure(f"could not read {file}", help=str(error)) from None
    return digest.hexdigest()


def _shell() -> str:
    if os.name == "nt":
        for candidate in (
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files\Git\usr\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ):
            if os.path.isfile(candidate):
                return candidate
    return "bash"


def _sleep(seconds: float = 0.025) -> None:
    time.sleep(seconds)


def _now() -> float:
    return time.monotonic()


class _Run:
    """The chainable run/assertion builder (check50 parity)."""

    def __init__(self, command: str, env: dict[str, str] | None = None) -> None:
        log(f"running {command}...")
        process_env = dict(os.environ, **(env or {}))
        # Force unbuffered C stdio so printf prompts are flushed
        # immediately even when stdout is a pipe (the C stdio
        # auto-flush before stdin reads only works when stdin and
        # stdout share a terminal; with separate pipes, C stdio
        # fully-buffers stdout and the prompt never appears).
        if os.name != "nt" and shutil.which("stdbuf"):
            command = f"stdbuf -o0 {command}"
        try:
            self._process = subprocess.Popen(
                [_shell(), "-c", command],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=process_env,
            )
        except OSError as error:
            raise Failure(f"could not run {command}: {error}") from None
        stdin = self._process.stdin
        stdout = self._process.stdout
        stderr = self._process.stderr
        if stdin is None or stdout is None or stderr is None:
            self._process.kill()
            raise Failure(f"could not run {command}: pipes unavailable")
        self._stdin: Any = stdin
        self._buffer = ""
        self._lock = threading.Lock()
        self._cursor = 0
        self._exited = False
        self._decoder = codecs.getincrementaldecoder("utf-8")("replace")
        threading.Thread(target=self._read, args=(stdout,), daemon=True).start()
        threading.Thread(target=self._drain_stderr, args=(stderr,), daemon=True).start()

    def _read(self, pipe: Any) -> None:
        while True:
            chunk = pipe.read1(4096)
            if not chunk:
                break
            text = self._decoder.decode(chunk)
            with self._lock:
                self._buffer += text
        with self._lock:
            self._buffer += self._decoder.decode(b"", True)

    def _drain_stderr(self, pipe: Any) -> None:
        while pipe.read1(4096):
            pass

    def _text(self) -> str:
        with self._lock:
            return self._buffer

    def _len(self) -> int:
        with self._lock:
            return len(self._buffer)

    def _quiesce(self) -> None:
        last = self._len()
        stable = 0
        while stable < 4:
            time.sleep(0.025)
            length = self._len()
            if length == last:
                stable += 1
            else:
                stable = 0
                last = length

    def _try_exit(self) -> bool:
        if self._exited:
            return True
        if self._process.poll() is not None:
            self._exited = True
            self._quiesce()
            return True
        return False

    def _segfault(self) -> bool:
        code = self._process.returncode
        if code is None:
            return False
        if os.name == "nt":
            return code == -1073741819
        return code == -11

    def stdin(self, input_: str | Eof, prompt: bool = True, timeout: float = 3) -> _Run:
        if input_ is EOF:
            log("sending EOF...")
        else:
            log(f"sending input {input_}...")
        if prompt:
            deadline = _now() + timeout
            while self._len() == 0:
                if self._try_exit():
                    break
                if _now() >= deadline:
                    raise Failure("expected prompt for input, found none")
                _sleep(0.05)
            self._quiesce()
            self._cursor = self._len()
        stdin = self._stdin
        if stdin is None or stdin.closed:
            raise Failure("stdin is closed")
        if input_ is EOF:
            stdin.close()
        else:
            try:
                stdin.write(f"{input_}\n".encode())
                stdin.flush()
            except OSError:
                raise Failure("could not send input to the program") from None
        return self

    def _make_matcher(self, pattern: str, exact: bool) -> Any:
        if exact:
            def matcher(text: str) -> int | None:
                start = text.find(pattern)
                return None if start < 0 else start + len(pattern)
        else:
            try:
                compiled = _re.compile(pattern)
            except _re.error:
                raise Failure("could not verify output (pattern is not a valid regex)") from None
            def matcher(text: str) -> int | None:
                match = compiled.search(text)
                return None if match is None else match.end()
        return matcher

    def stdout(self, output: str | int | float | Eof | None = None, str_output: str | None = None, regex: bool = True, timeout: float = 3) -> Any:
        if output is None:
            return self._stdout_text(timeout)
        eof = output is EOF
        exact = not regex
        pattern = ""
        if eof:
            log("checking for EOF...")
            def matcher(text: str) -> int | None:
                return None
        else:
            if isinstance(output, (int, float)) and not isinstance(output, bool):
                pattern = decimal(output) if regex else str(output)
            else:
                pattern = str_output if (str_output is not None and not regex) else str(output)
            log(f'checking for output "{pattern}"...')
            matcher = self._make_matcher(pattern, exact)
        deadline = _now() + timeout
        last = -1
        while True:
            length = self._len()
            if length != last:
                last = length
                if not eof:
                    unconsumed = self._text()[self._cursor:]
                    end = matcher(unconsumed)
                    if end is not None:
                        self._cursor += end
                        return self
            if self._try_exit():
                self._quiesce()
                unconsumed = self._text()[self._cursor:]
                if eof and not unconsumed:
                    return self
                raise Mismatch("EOF" if eof else pattern, unconsumed)
            if _now() >= deadline:
                raise Failure(f"timed out while waiting for output (waited {timeout:.1f}s)")
            _sleep()

    def _stdout_text(self, timeout: float) -> str:
        # Close stdin (EOF) and wait for the output to stabilize. Then
        # kill the process (the output is what matters for checks that
        # capture it with stdout() — no args).
        if self._stdin and not self._stdin.closed:
            self._stdin.close()
        deadline = _now() + timeout
        last = -1
        stable = 0
        while stable < 12 and _now() < deadline:
            length = self._len()
            if length == last:
                stable += 1
            else:
                stable = 0
                last = length
            _sleep(0.025)
        with contextlib.suppress(OSError):
            self._process.kill()
        with contextlib.suppress(OSError):
            self._process.wait()
        return self._text()[self._cursor:].replace("\r\n", "\n").lstrip("\n")

    def _wait_exit(self, timeout: float) -> int:
        deadline = _now() + timeout
        while not self._try_exit():
            if _now() >= deadline:
                self.kill()
                raise Failure("timed out while waiting for program to exit")
            _sleep()
        if self._segfault():
            raise Failure("failed to execute program due to segmentation fault")
        return self._process.returncode or 0

    def reject(self, timeout: float = 1) -> _Run:
        log("checking that input was rejected...")
        deadline = _now() + timeout
        while _now() < deadline:
            if self._try_exit():
                raise Failure("expected program to reject input, but it did not")
            _sleep()
        return self

    def exit(self, code: int | None = None, timeout: float = 5) -> int:
        if self._stdin and not self._stdin.closed:
            self._stdin.close()
        actual = self._wait_exit(timeout)
        if code is None:
            return actual
        log(f"checking that program exited with status {code}...")
        if actual != code:
            raise Failure(f"expected exit code {code}, not {actual}")
        return actual

    def kill(self) -> _Run:
        with contextlib.suppress(OSError):
            self._process.kill()
        self._exited = True
        return self


def run(command: str, env: dict[str, str] | None = None) -> _Run:
    return _Run(command, env)


def decimal(number: float) -> str:
    return regex.decimal(number)
