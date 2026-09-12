"""Per-invocation state shared by the authoring API and the process
bridge: the check registry (declaration order, descriptions,
dependencies, timeouts) and the current check's log/payload.
"""


class Check:
    """A registered check: the decorated function plus metadata."""

    def __init__(self, fn, description, dependency, timeout):
        self.fn = fn
        self.description = description
        self.dependency = dependency
        self.timeout = timeout
        self.hidden = None


# The registry: declaration order (check50 parity: the decorator
# appends to a module-global list).
checks = {}
order = []

# The current check's student-visible log and result payload.
log_lines = []
payload = {}


def reset():
    """Clears the per-invocation state (bridge, before each check)."""
    log_lines.clear()
    payload.clear()


def log(line=""):
    """Adds a line to the check log (newlines escaped, check50 parity)."""
    log_lines.append(str(line).replace("\n", "\\n"))


def register(fn, dependency, timeout):
    """Registers a check in declaration order (the decorator's write
    side); the docstring is the user-visible description."""
    name = fn.__name__
    if dependency is not None and not isinstance(dependency, str):
        dependency = dependency.__name__
    check = Check(fn, (fn.__doc__ or "").strip() or name, dependency, timeout)
    check.hidden = getattr(fn, "_check50_hidden", None)
    checks[name] = check
    order.append(name)
    return fn
