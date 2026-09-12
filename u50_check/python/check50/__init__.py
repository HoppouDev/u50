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
import re
import subprocess
import threading
import time

from . import bridge_state as state
from . import c, regex
from .errors import EOF, Failure, Mismatch, Missing

__all__ = [
    "EOF", "Failure", "Mismatch", "Missing", "c", "check", "data",
    "exists", "hash", "hidden", "include", "log", "regex", "run",
]


def check(dependency=None, *, timeout=None):
    """Registers a check (check50 parity): the docstring is the
    user-visible description; the dependency is a function or name."""

    def decorator(fn, dependency=dependency):
        state.register(fn, dependency, timeout)
        return fn

    # Support the bare form: @check50.check (no parentheses). The
    # discriminator is registry membership: a dependency must be an
    # already-registered check (check50's own constraint), so a callable
    # that is not in the registry is the function being decorated.
    if callable(dependency) and dependency.__name__ not in state.checks:
        fn, dependency = dependency, None
        return decorator(fn)
    return decorator


def hidden(rationale):
    """Marks a check hidden: the engine suppresses its log and replaces
    any failure with the generic rationale (applied from the registry
    metadata, so ordering with @check50.check is free)."""

    def decorator(fn):
        fn._check50_hidden = rationale
        entry = state.checks.get(fn.__name__)
        if entry is not None:
            entry.hidden = rationale
        return fn

    return decorator


def log(line=""):
    """Adds a line to the check log (newlines escaped, check50 parity)."""
    state.log(line)


def data(**kwargs):
    """Adds key/value pairs to the check's result payload."""
    state.payload.update(kwargs)


def exists(*paths):
    """Asserts every path exists (relative to the run dir)."""
    for path in paths:
        log(f"checking that {path} exists...")
        if not os.path.exists(path):
            raise Failure(f"{path} not found")


def _copy(src, dst):
    """Recursive copy (check50: `_copy`); any copy failure is a check
    failure (the Rust `include` maps errors the same way)."""
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


def include(*paths):
    """Copies files from the check's own directory into the run dir."""
    check_dir = os.environ.get("CHECK50_CHECK_DIR", "")
    for path in paths:
        _copy(os.path.join(check_dir, path), path)


def hash(file):
    """SHA-256 of a file (streaming)."""
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


# --- the run builder -------------------------------------------------------


def _shell():
    """The POSIX shell running check commands (mirrors the Rust
    `bash_command` resolution: Git for Windows' bash preferred over the
    WSL launcher stub)."""
    if os.name == "nt":
        for candidate in (
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files\Git\usr\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ):
            if os.path.isfile(candidate):
                return candidate
    return "bash"


def _sleep(seconds=0.025):
    time.sleep(seconds)


def _now():
    return time.monotonic()


class _Run:
    """The chainable run/assertion builder (check50 parity: prompt
    absorption, EOF, regex/exact/decimal matching, reject, exit-code
    assert, SIGSEGV detection)."""

    def __init__(self, command, env=None):
        log(f"running {command}...")
        process_env = dict(os.environ, **(env or {}))
        try:
            process = subprocess.Popen(
                [_shell(), "-c", command],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=process_env,
            )
        except OSError as error:
            raise Failure(f"could not run {command}: {error}") from None
        stdin = process.stdin
        stdout = process.stdout
        stderr = process.stderr
        if stdin is None or stdout is None or stderr is None:
            process.kill()
            raise Failure(f"could not run {command}: pipes unavailable")
        self._process = process
        self._stdin = stdin
        self._buffer = ""
        self._lock = threading.Lock()
        self._cursor = 0
        self._exited = False
        self._decoder = codecs.getincrementaldecoder("utf-8")("replace")
        threading.Thread(target=self._read, args=(stdout,), daemon=True).start()
        threading.Thread(target=self._drain_stderr, args=(stderr,), daemon=True).start()

    def _read(self, pipe):
        while True:
            chunk = pipe.read(4096)
            if not chunk:
                break
            text = self._decoder.decode(chunk)
            with self._lock:
                self._buffer += text
        with self._lock:
            self._buffer += self._decoder.decode(b"", True)

    def _drain_stderr(self, pipe):
        # A program that fills the OS pipe buffer would otherwise
        # deadlock against an unread stderr pipe.
        while pipe.read(4096):
            pass

    def _text(self):
        with self._lock:
            return self._buffer

    def _len(self):
        with self._lock:
            return len(self._buffer)

    def _quiesce(self):
        """Bounded window (100ms of stability) for the reader to drain
        the tail output after the program exited."""
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

    def _try_exit(self):
        if self._exited:
            return True
        if self._process.poll() is not None:
            self._exited = True
            self._quiesce()
            return True
        return False

    def _segfault(self):
        code = self._process.returncode
        if code is None:
            return False
        if os.name == "nt":
            return code == -1073741819  # STATUS_ACCESS_VIOLATION
        return code == -11  # SIGSEGV

    def stdin(self, input_, prompt=True, timeout=3):
        """Sends input (or EOF). With a prompt, absorbs output first
        (else `"expected prompt for input, found none"`)."""
        if input_ is EOF:
            log("sending EOF...")
        else:
            log(f"sending input {input_}...")
        if prompt:
            deadline = _now() + timeout
            start = self._len()
            while self._len() <= start:
                if self._try_exit():
                    break
                if _now() >= deadline:
                    raise Failure("expected prompt for input, found none")
                _sleep(0.05)
            self._quiesce()
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

    def _make_matcher(self, pattern, exact):
        if exact:
            def matcher(text):
                start = text.find(pattern)
                return None if start < 0 else start + len(pattern)
        else:
            try:
                compiled = re.compile(pattern)
            except re.error:
                raise Failure(
                    "could not verify output (pattern is not a valid regex)"
                ) from None

            def matcher(text):
                match = compiled.search(text)
                return None if match is None else match.end()
        return matcher

    def stdout(self, output=None, str_output=None, regex=True, timeout=3):
        """Waits until the unconsumed output matches (regex by default,
        exact with `regex=False`, numbers via `regex.decimal`); with
        `output=None` waits for exit and returns all unconsumed output."""
        if output is None:
            return self._stdout_text(timeout)
        eof = output is EOF
        exact = not regex
        pattern = ""
        if eof:
            log("checking for EOF...")

            def matcher(text):
                return None

        else:
            if isinstance(output, (int, float)) and not isinstance(output, bool):
                pattern = decimal(output) if regex else str(output)
            else:
                pattern = str_output if (str_output is not None and not regex) else output
                pattern = str(pattern)
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
                raise Failure(
                    f"timed out while waiting for output (waited {timeout:.1f}s)"
                )
            _sleep()

    def _stdout_text(self, timeout):
        self._wait_exit(timeout)
        return self._text()[self._cursor:].replace("\r\n", "\n").lstrip("\n")

    def _wait_exit(self, timeout):
        """Waits for exit within `timeout`; returns the exit code
        (SIGSEGV parity: an explicit Failure)."""
        deadline = _now() + timeout
        while not self._try_exit():
            if _now() >= deadline:
                self.kill()
                raise Failure("timed out while waiting for program to exit")
            _sleep()
        if self._segfault():
            raise Failure("failed to execute program due to segmentation fault")
        return self._process.returncode

    def reject(self, timeout=1):
        """Asserts the program survived without consuming the input."""
        log("checking that input was rejected...")
        deadline = _now() + timeout
        while _now() < deadline:
            if self._try_exit():
                raise Failure("expected program to reject input, but it did not")
            _sleep()
        return self

    def exit(self, code=None, timeout=5):
        """Waits for exit; asserts the code when given, else returns it."""
        actual = self._wait_exit(timeout)
        if code is None:
            return actual
        log(f"checking that program exited with status {code}...")
        if actual != code:
            raise Failure(f"expected exit code {code}, not {actual}")
        return actual

    def kill(self):
        """Kills the program (and its process group on POSIX: the check
        process is the group leader, so this takes down any student
        grandchildren too)."""
        if os.name != "nt":
            with contextlib.suppress(OSError):
                os.killpg(os.getpgrp(), 9)  # SIGKILL
        with contextlib.suppress(OSError):
            self._process.kill()
        self._exited = True
        return self


def run(command, env=None):
    """Spawns `command` (via `bash -c`, exactly like check50) in the run
    dir, returning the chainable builder."""
    return _Run(command, env)


def decimal(number):
    """The exact-number regex (check50: `regex.decimal`)."""
    return regex.decimal(number)
