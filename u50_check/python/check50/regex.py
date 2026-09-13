"""Regex helpers (check50: `check50.regex`)."""


def decimal(number: float) -> str:
    """Matches the exact number only (check50 parity)."""
    text = str(number)
    lookbehind = "" if text.startswith("-") else "(?<![\\d-])"
    return f"{lookbehind}{text}(?!(\\.?\\d))"
