#!/bin/sh
# format-rust.sh — Claude Code PostToolUse hook: format edited Rust sources.
#
# Runs rustfmt --edition 2024 on the edited file when it exists, ends in
# .rs, and lives under a src/ directory. NEVER blocks the agent: always
# exits 0 (a rustfmt failure only prints a one-line warning to stderr).
#
# HARD EXCLUSION: files under tests/fixtures/ or examples/ are byte-pinned
# goldens and are never touched. Non-Rust files (docs, yaml, ...) no-op.

set -u

input=$(cat 2>/dev/null) || exit 0
[ -n "$input" ] || exit 0

file=$(printf '%s\n' "$input" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
[ -n "$file" ] || exit 0

# Byte-pinned goldens: never format, regardless of anything else.
case "$file" in
  */tests/fixtures/*|tests/fixtures/*|*/examples/*|examples/*) exit 0 ;;
esac

# Only .rs files under a src/ directory.
case "$file" in
  *.rs) ;;
  *) exit 0 ;;
esac
case "$file" in
  */src/*|src/*) ;;
  *) exit 0 ;;
esac

[ -f "$file" ] || exit 0

if ! command -v rustfmt >/dev/null 2>&1; then
  echo "format-rust: rustfmt not found; skipping $file (non-blocking)" >&2
  exit 0
fi

if ! rustfmt --edition 2024 -- "$file" >/dev/null 2>&1; then
  echo "format-rust: rustfmt failed for $file (continuing, non-blocking)" >&2
fi

exit 0
