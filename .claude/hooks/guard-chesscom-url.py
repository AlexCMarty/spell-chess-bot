#!/usr/bin/env python3
"""PreToolUse guard: the analysis board is the only chess.com page this repo may open.

Every other chess.com page can match an automated client into a live game against a
human, which violates chess.com's terms of service and risks the account. The rule is
stated in CLAUDE.md and README.md, but prose is a request; this hook is enforcement.

Exit 0 = allow, exit 2 = block (stderr is fed back to Claude as the reason).
Any unexpected failure exits 0: a broken guard must not wedge the session.
"""

import json
import re
import sys
from urllib.parse import urlparse

ALLOWED_PATH = "/variants/spell-chess/analysis"

# Static client-package assets (the variants.js engine bundle this project reverse-
# engineered the rules from). These are inert files, not pages that can match a client
# into a live game, so fetching them carries none of the risk this guard exists for.
ALLOWED_PREFIXES = ("/r2/client-packages/",)

# Bash is only inspected when the command actually looks like it fetches or opens a URL,
# so that grepping the docs for a chess.com link is not treated as navigation.
FETCH_HINT = re.compile(
    r"\b(curl|wget|xdg-open|open|start|chromium|chrome|firefox|playwright|puppeteer|"
    r"selenium|httpie|http|lynx|w3m)\b|webbrowser",
    re.IGNORECASE,
)
URL_RE = re.compile(r"https?://[^\s\"'`<>\\)]+", re.IGNORECASE)


def is_chesscom(host: str) -> bool:
    host = (host or "").lower().split(":")[0]
    return host == "chess.com" or host.endswith(".chess.com")


def offending(url: str) -> bool:
    """True if this URL is a chess.com page that is not the analysis board."""
    try:
        parsed = urlparse(url)
    except ValueError:
        return False
    if not is_chesscom(parsed.netloc):
        return False
    if parsed.path.startswith(ALLOWED_PREFIXES):
        return False
    return parsed.path.rstrip("/") != ALLOWED_PATH


def urls_from(tool_name: str, tool_input: dict):
    """Collect URLs worth checking for this tool."""
    if tool_name == "Bash":
        command = str(tool_input.get("command", ""))
        if FETCH_HINT.search(command):
            yield from URL_RE.findall(command)
        return

    # WebFetch and the browser-automation MCP tools carry the target in a field.
    for key in ("url", "uri", "href", "link"):
        value = tool_input.get(key)
        if isinstance(value, str):
            yield value

    # Some browser tools nest the target (e.g. in a coordinate/action payload).
    for value in tool_input.values():
        if isinstance(value, str) and "chess.com" in value.lower():
            yield from URL_RE.findall(value)


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except Exception:
        return 0

    tool_name = payload.get("tool_name", "")
    tool_input = payload.get("tool_input") or {}
    if not isinstance(tool_input, dict):
        return 0

    for url in urls_from(tool_name, tool_input):
        if offending(url):
            sys.stderr.write(
                f"BLOCKED: {url}\n\n"
                "Only https://www.chess.com/variants/spell-chess/analysis may be opened "
                "from this repo. Every other chess.com page can match an automated "
                "client into a live game against a human, which violates chess.com's "
                "terms of service.\n\n"
                "The analysis board exposes the same engine and is sufficient for all "
                "rules verification -- see rules/70-engine-api.md.\n"
            )
            return 2

    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:
        # Never let a guard bug block unrelated work.
        sys.exit(0)
