"""Python helpers (check50: `check50.py`)."""

import builtins
import importlib
import inspect
import os
import sys

from .bridge_state import loaded_path


def compile(code, name="<check50-compiled>"):
    return builtins.compile(code, name, "exec")


def import_(name):
    checks_dir = os.path.dirname(loaded_path())
    if checks_dir and checks_dir not in sys.path:
        sys.path.insert(0, checks_dir)
    return importlib.import_module(name)


def append_code(fn, code):
    source = inspect.getsource(fn)
    indent = " " * (len(source) - len(source.lstrip()) + 4)
    body = "\n".join(indent + line for line in code.strip("\n").splitlines())
    new_source = source + "\n" + body + "\n"
    namespace = {**getattr(fn, "__globals__", {})}
    exec(builtins.compile(new_source, f"<{fn.__name__}+>", "exec"), namespace)
    return namespace[fn.__name__]
