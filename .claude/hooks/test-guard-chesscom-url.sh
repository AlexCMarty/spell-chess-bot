#!/usr/bin/env bash
# Tests for guard-chesscom-url.py. Run with:  bash .claude/hooks/test-guard-chesscom-url.sh
#
# Note the cases live in this file rather than on a command line on purpose: the guard is
# active while you work in this repo, so a test command containing a blocked URL would be
# blocked before it ever ran.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

GUARD="./guard-chesscom-url.py"
ANALYSIS="https://www.chess.com/variants/spell-chess/analysis"
pass=0 fail=0

check() { # name, want_exit, json
  printf '%s' "$3" | python3 "$GUARD" >/dev/null 2>&1
  local got=$?
  if [ "$got" -eq "$2" ]; then
    pass=$((pass + 1))
    printf '  ok   %-44s exit=%s\n' "$1" "$got"
  else
    fail=$((fail + 1))
    printf '  FAIL %-44s exit=%s want=%s\n' "$1" "$got" "$2"
  fi
}

web()  { printf '{"tool_name":"WebFetch","tool_input":{"url":"%s"}}' "$1"; }
nav()  { printf '{"tool_name":"mcp__open-claude-in-chrome__navigate","tool_input":{"url":"%s"}}' "$1"; }
bash_() { printf '{"tool_name":"Bash","tool_input":{"command":"%s"}}' "$1"; }

echo "guard-chesscom-url.py"

# Allowed: the analysis board, with or without a trailing slash.
check "analysis board"              0 "$(web "$ANALYSIS")"
check "analysis board, trailing /"  0 "$(web "$ANALYSIS/")"
check "analysis via browser tool"   0 "$(nav "$ANALYSIS")"
check "analysis via curl"           0 "$(bash_ "curl -s $ANALYSIS")"
# The engine bundle is an inert static asset, not a page that can match into a game.
check "client-package engine bundle" 0 \
  "$(web "https://www.chess.com/r2/client-packages/variants/2026.8.1/variants.js")"
check "engine bundle via curl"      0 \
  "$(bash_ "curl -sO https://www.chess.com/r2/client-packages/variants/2026.8.1/variants.js")"

# Blocked: any other chess.com page, however it is reached.
check "chess.com play page"         2 "$(web "https://www.chess.com/play/online")"
check "variant page without /analysis" 2 "$(web "https://www.chess.com/variants/spell-chess")"
check "browser tool to /home"       2 "$(nav "https://www.chess.com/home")"
check "curl to /play"               2 "$(bash_ "curl -s https://www.chess.com/play")"
check "apex domain"                 2 "$(web "https://chess.com/live")"
check "other subdomain"             2 "$(web "https://www2.chess.com/live")"

# Not navigation: must not fire.
check "unrelated host"              0 "$(web "https://example.com/chess.com")"
check "lookalike host"              0 "$(web "https://notchess.com/play")"
check "grep for a URL in the docs"  0 "$(bash_ "grep -n https://www.chess.com/variants/spell-chess README.md")"
check "ordinary cargo command"      0 "$(bash_ "cargo test --workspace")"

# Must fail open, never wedge the session.
check "malformed json"              0 'not json at all'
check "missing tool_input"          0 '{"tool_name":"WebFetch"}'
check "tool_input not an object"    0 '{"tool_name":"WebFetch","tool_input":"nope"}'

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
