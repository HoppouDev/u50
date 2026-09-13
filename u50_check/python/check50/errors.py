"""The check50 exception types and sentinels (check50 parity)."""

from __future__ import annotations

from typing import override


def raw(value: object, n: int = 15) -> str:
    """Truncated double-quoted rendering (parity with the engine's
    mismatch rationale and the captured check50 goldens)."""
    rendered = '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"' if isinstance(value, str) else repr(value)  # type: ignore[attr-defined]
    return rendered if len(rendered) <= n else rendered[:n] + "..."


class Failure(Exception):
    """Signifies a check failure."""

    rationale: str
    help: str | None

    def __init__(self, rationale: str, help: str | None = None) -> None:
        self.rationale = rationale
        self.help = help
        super().__init__(rationale)


class Mismatch(Failure):
    """A check failure caused by output not matching."""

    expected: str
    actual: str

    def __init__(self, expected: str, actual: str, help: str | None = None) -> None:
        super().__init__(f"expected {raw(expected)}, not {raw(actual)}", help)
        self.expected = expected
        self.actual = actual


class Missing(Failure):
    """A check failure caused by an item missing from a collection."""

    def __init__(self, item: str, collection: str, help: str | None = None) -> None:
        super().__init__(f'Did not find "{item}" in "{collection}"', help)


class Eof:
    """Sentinel for end-of-file (check50: `check50.EOF`)."""

    @override
    def __repr__(self) -> str:
        return "EOF"


EOF = Eof()
