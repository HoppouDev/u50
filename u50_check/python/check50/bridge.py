"""The process bridge: discovery and invocation of Python checks.

The Rust runner spawns `python -m check50.bridge <cmd>` once to
discover the check registry (declaration order, descriptions,
dependencies, timeouts) and once per check to invoke it, passing the
dependency's return value via a JSON state file in the run dir.
"""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import traceback
from typing import Any

from . import bridge_state as state
from .errors import Failure, Mismatch

STATE_FILE = ".check50-state"


def _load_module(path: str) -> Any:
    spec = importlib.util.spec_from_file_location("checks", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"could not load the checks module from {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["checks"] = module
    spec.loader.exec_module(module)
    return module


def _envelope(ok: bool, **extra: Any) -> None:
    envelope: dict[str, Any] = {
        "ok": ok,
        "log": list(state.log_lines),
        "data": dict(state.payload),
    }
    envelope.update(extra)
    print(json.dumps(envelope), flush=True)


def _error_envelope(exc: Exception) -> None:
    _envelope(
        False,
        error={
            "type": type(exc).__name__,
            "value": str(exc),
            "traceback": traceback.format_exc().splitlines(),
        },
    )


def discover(path: str) -> None:
    try:
        _load_module(path)
    except Exception as exc:
        print(traceback.format_exc(), file=sys.stderr)
        raise SystemExit(f"could not load the checks module {path}: {exc}") from exc
    checks = [
        {
            "name": name,
            "description": state.checks[name].description,
            "dependency": state.checks[name].dependency,
            "timeout": state.checks[name].timeout,
            "hidden": state.checks[name].hidden,
        }
        for name in state.order
    ]
    print(json.dumps({"checks": checks}), flush=True)


def invoke(path: str, name: str, state_file: str) -> None:
    state.reset()
    try:
        _load_module(path)
    except Exception as exc:
        _error_envelope(exc)
        return
    check = state.checks.get(name)
    if check is None:
        _envelope(
            False,
            error={
                "type": "UnknownCheck",
                "value": f"no registered check named {name}",
                "traceback": [],
            },
        )
        return
    fn = check.fn
    state_info: Any = None
    has_state = state_file != "-" and os.path.exists(state_file)
    if has_state:
        try:
            with open(state_file) as handle:
                state_info = json.load(handle)
        except Exception as exc:
            _error_envelope(exc)
            return
    try:
        result = fn(state_info) if has_state else fn()
    except Failure as failure:
        cause: dict[str, Any] = {
            "rationale": failure.rationale,
            "help": failure.help,
        }
        if isinstance(failure, Mismatch):
            cause["expected"] = failure.expected
            cause["actual"] = failure.actual
        _envelope(False, cause=cause)
        return
    except Exception as exc:
        _error_envelope(exc)
        return
    if result is None:
        # check50 parity: a dependency that returns None passes nothing
        # (the README hello pattern has zero-arg dependents).
        _envelope(True)
        return
    try:
        with open(STATE_FILE, "w") as handle:
            json.dump(result, handle)
    except Exception as exc:
        _error_envelope(exc)
        return
    _envelope(True)


def main() -> None:
    command = sys.argv[1]
    if command == "discover":
        discover(sys.argv[2])
    elif command == "invoke":
        invoke(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        raise SystemExit(f"unknown bridge command: {command}")


if __name__ == "__main__":
    main()
