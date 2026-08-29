#!/bin/sh
# gate-destructive.sh — Claude Code PreToolUse guardrail for Bash commands.
#
# Fail-open by design: exits 0 (ALLOW) for anything it cannot parse or does
# not recognize. It only DENIES (exit 2 + stderr reason) clearly destructive
# patterns:
#   * rm -rf/-fr/--recursive targeting /, ~, $HOME, ., .. or /* (root/home/cwd wipe)
#   * git push --force / -f / --delete / :main / +main / +refs/heads/main
#   * git reset --hard
#   * redirects into .env / .env.*
#
# Pure POSIX sh + grep. No deps. Silent on allow.

set -u

input=$(cat 2>/dev/null) || exit 0
[ -n "$input" ] || exit 0

# Only gate Bash tool calls; anything else is allowed.
printf '%s\n' "$input" | grep -q '"tool_name"[[:space:]]*:[[:space:]]*"Bash"' || exit 0

# JSON-unescape enough of the payload to extract the full command string.
# The naive "[^"]*" capture stops at the first literal quote byte, which in
# a real payload is the escaped quote of any command containing a quoted
# argument (e.g. `echo "done" && rm -rf /`) - truncating the command so deny
# patterns never fire. Neutralize escape sequences first: \\ before \" so an
# escaped backslash cannot fuse with the real closing quote, then \" ->
# placeholder (no longer looks like a string terminator), and \n -> space so
# a destructive second line cannot hide from the token-boundary patterns.
esc=$(printf '%s\n' "$input" | sed -e 's/\\\\/__U50_BS__/g' -e 's/\\"/__U50_QT__/g' -e 's/\\n/ /g')
cmd=$(printf '%s\n' "$esc" | sed -n 's/.*"command"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
[ -n "$cmd" ] || exit 0

BUGGER='(^|[[:space:];&|])'

deny() {
  echo "gate-destructive: BLOCKED destructive command: $1" >&2
  echo "gate-destructive: only clearly destructive operations are gated; if this is a false positive, run the command manually after review." >&2
  exit 2
}

# 1. rm with a recursive flag aimed at /, /*, ~, $HOME, . or ..
#    Matches rm -rf, rm -fr, rm -r -f (also -R). Plain 'rm -rf build/' and
#    'rm -rf ./build' stay allowed.
if printf '%s\n' "$cmd" | grep -Eq "${BUGGER}rm[[:space:]]+(-[a-zA-Z]*[rR][a-zA-Z]*|--recursive)([[:space:]]+(-[a-zA-Z]*[rR][a-zA-Z]*|--recursive)[[:space:]]+)*"; then
  if printf '%s\n' "$cmd" | grep -Eq '(^|[[:space:]])(/\*|\.\.?/|\$\{?HOME\}?/|~/|/|\.\.?|\$\{?HOME\}?|~)([[:space:]]|$)'; then
    deny "$cmd"
  fi
fi

# 2. git push force/delete targeting main (normal commits to main are
#    pre-authorized; force-push and branch-delete of main are not).
if printf '%s\n' "$cmd" | grep -Eq "${BUGGER}git[[:space:]]+push([[:space:]]|$)"; then
  if printf '%s\n' "$cmd" | grep -Eq '(^|[[:space:]:/+])(refs/heads/)?main([[:space:]]|:|/|$)' \
     && printf '%s\n' "$cmd" | grep -Eq '(^|[[:space:]])--force([[:space:]]|$)|(^|[[:space:]])-f([[:space:]]|$)|(^|[[:space:]])--delete([[:space:]]|$)|:main([[:space:]]|$)|(^|[[:space:]])\+(refs/heads/)?main([[:space:]]|:|$)'; then
    deny "$cmd"
  fi
fi

# 3. git reset --hard
if printf '%s\n' "$cmd" | grep -Eq "${BUGGER}git[[:space:]]+reset([[:space:]]|$)" \
   && printf '%s\n' "$cmd" | grep -Eq '(^|[[:space:]])--hard([[:space:]]|$)'; then
  deny "$cmd"
fi

# 4. Redirect into .env / .env.* (covers > and >>, incl. dir-prefixed paths;
#    a file like 'build.env' is NOT matched).
if printf '%s\n' "$cmd" | grep -Eq '>+[[:space:]]*(\.env(\.[^[:space:];&|]*)?|[^[:space:];&|]*/\.env(\.[^[:space:];&|]*)?)([[:space:]]|;|&|$)'; then
  deny "$cmd"
fi

exit 0
