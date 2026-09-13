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
        cmd.append("-I" + os.getcwd())
        if "cs50.c" not in files:
            cmd.append("cs50.c")
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise Failure(
            f"could not compile {' '.join(files)}",
            help=(result.stderr.strip() or None),
        )
    return target


def _ensure_cs50_files() -> None:
    """Downloads cs50.h and cs50.c into the run dir (compiled alongside
    the student's code so no system-wide libcs50 install is needed)."""
    import urllib.request

    for fname in ("cs50.h", "cs50.c"):
        if not os.path.exists(fname):
            urllib.request.urlretrieve(
                f"https://raw.githubusercontent.com/cs50/libcs50/main/src/{fname}",
                fname,
            )
