"""C helpers (check50: `check50.c`)."""

from __future__ import annotations

import os
import subprocess

from .bridge_state import log
from .errors import Failure


def compile(*files: str, cc: str = "clang", lcs50: bool = False, **kwargs: object) -> str:
    """Compiles C source files into an executable named after the first
    source file (check50 parity)."""
    log("compiling...")
    target = os.path.splitext(files[0])[0]
    cmd = [cc, "-o", target, *files]
    if lcs50:
        _ensure_cs50_files()
        cmd.extend(["-lcs50", "-I" + os.getcwd()])
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise Failure(
            f"could not compile {' '.join(files)}",
            help=(result.stderr.strip() or None),
        )
    return target


def _ensure_cs50_files() -> None:
    import urllib.request

    if not os.path.exists("cs50.h"):
        urllib.request.urlretrieve(
            "https://raw.githubusercontent.com/cs50/libcs50/develop/src/cs50.h",
            "cs50.h",
        )
