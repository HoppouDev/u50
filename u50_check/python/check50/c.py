"""C helpers (check50: `check50.c`)."""

import os
import subprocess

from .bridge_state import log
from .errors import Failure


def compile(*files, cc="clang"):
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
