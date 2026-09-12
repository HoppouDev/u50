"""Regex helpers (check50: `check50.regex`)."""


def decimal(number):
    """Matches the exact number only (check50 parity: a negative
    lookbehind for non-negative numbers and a negative lookahead
    always, so `"420"` does not match `42`). Python's `re` supports
    look-around, so this is check50-faithful."""
    number = str(number)
    lookbehind = "" if number.startswith("-") else "(?<![\\d-])"
    return f"{lookbehind}{number}(?!(\\.?\\d))"
