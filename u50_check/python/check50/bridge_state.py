"""Per-invocation state shared by the authoring API and the process
bridge: the check registry (declaration order, descriptions,
dependencies, timeouts) and the current check's log/payload.
"""

from __future__ import annotations

from typing import Any, Callable


class Check:
    """A registered check: the decorated function plus metadata."""

    fn: Callable[..., object]
    description: str
    dependency: str | None
    timeout: float | None
    hidden: str | None

    def __init__(
        self,
        fn: Callable[..., object],
        description: str,
        dependency: str | None,
        timeout: float | None,
    ) -> None:
        self.fn = fn
        self.description = description
        self.dependency = dependency
        self.timeout = timeout
        self.hidden: str | None = None


# The registry: declaration order (check50 parity: the decorator
# appends to a module-global list).
checks: dict[str, Check] = {}
order: list[str] = []

# The current check's student-visible log and result payload.
log_lines: list[str] = []
payload: dict[str, Any] = {}

# The checks file currently loaded (set by the bridge; import_checks
# resolves sibling modules against it).
loaded_path: str = ""


def reset() -> None:
    """Clears the per-invocation state (bridge, before each check)."""
    log_lines.clear()
    payload.clear()


def log(line: str = "") -> None:
    """Adds a line to the check log (newlines escaped, check50 parity)."""
    log_lines.append(str(line).replace("\n", "\\n"))


def register(
    fn: Callable[..., object],
    dependency: str | None,
    timeout: float | None,
) -> Callable[..., object]:
    """Registers a check in declaration order (the decorator's write
    side); the docstring is the user-visible description."""
    name = fn.__name__
    check = Check(fn, (fn.__doc__ or "").strip() or name, dependency, timeout)
    check.hidden = getattr(fn, "_check50_hidden", None)
    # cs50/problems parity: later definitions override earlier ones
    # (a check set that imports another via import_checks may redefine
    # checks with the same name — the last registration wins).
    is_new = name not in checks
    checks[name] = check
    if is_new:
        order.append(name)
    return fn
