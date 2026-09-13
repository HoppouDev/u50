"""C helpers (check50: `check50.c`)."""

import os
import subprocess

from .bridge_state import log
from .errors import Failure


def compile(*files, cc="clang", lcs50=False, **kwargs):
    """Compiles C source files into an executable named after the first
    source file (check50 parity: `c.compile`)."""
    log("compiling...")
    target = os.path.splitext(files[0])[0]
    result = subprocess.run([cc, "-o", target, *files], capture_output=True, text=True)
    if result.returncode != 0:
        raise Failure(
            f"could not compile {' '.join(files)}",
            help=(result.stderr.strip() or None),
        )
    return target



def _ensure_cs50_files():
    """Downloads cs50.h into the run dir if not present (for lcs50)."""
    import urllib.request

    if not os.path.exists("cs50.h"):
        urllib.request.urlretrieve(
            "https://raw.githubusercontent.com/cs50/libcs50/develop/src/cs50.h",
            "cs50.h",
        )
