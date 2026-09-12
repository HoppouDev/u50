"""The check50 exception types and sentinels (check50 parity)."""


def raw(value, n=15):
    """Truncated double-quoted rendering (parity with the engine's
    mismatch rationale and the captured check50 goldens)."""
    value = '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"' if isinstance(value, str) else repr(value)
    return value if len(value) <= n else value[:n] + "..."


class Failure(Exception):
    """Signifies a check failure."""

    def __init__(self, rationale, help=None):
        self.rationale = rationale
        self.help = help
        super().__init__(rationale)


class Mismatch(Failure):
    """A check failure caused by output not matching."""

    def __init__(self, expected, actual, help=None):
        super().__init__(f"expected {raw(expected)}, not {raw(actual)}", help)
        self.expected = expected
        self.actual = actual


class Missing(Failure):
    """A check failure caused by an item missing from a collection."""

    def __init__(self, item, collection, help=None):
        super().__init__(f'Did not find "{item}" in "{collection}"', help)


class Eof:
    """Sentinel for end-of-file (check50: `check50.EOF`)."""

    def __repr__(self):
        return "EOF"


EOF = Eof()
