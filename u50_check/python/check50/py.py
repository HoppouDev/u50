"""Python helpers (check50: `check50.py`)."""

from __future__ import annotations

import builtins
import importlib
import inspect
import os
import sys
from typing import Any, Callable

from .bridge_state import loaded_path


def compile(code: str, name: str = "<check50-compiled>") -> Any:
    return builtins.compile(code, name, "exec")


def import_(name: str) -> Any:
    checks_dir = os.path.dirname(loaded_path)
    if checks_dir and checks_dir not in sys.path:
        sys.path.insert(0, checks_dir)
    return importlib.import_module(name)


def append_code(fn: Callable[..., Any], code: str) -> Callable[..., Any]:
    source = inspect.getsource(fn)
    indent = " " * (len(source) - len(source.lstrip()) + 4)
    body = "\n".join(indent + line for line in code.strip("\n").splitlines())
    new_source = source + "\n" + body + "\n"
    namespace: dict[str, Any] = {**getattr(fn, "__globals__", {})}
    exec(builtins.compile(new_source, f"<{fn.__name__}+>", "exec"), namespace)
    return namespace[fn.__name__]
