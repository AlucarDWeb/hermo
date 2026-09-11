#!/usr/bin/env python
"""Redact environment and personal fields from recorded protocol fixtures.

A live `session.info` frame (and, in some shapes, a `session.create` result) carries the
agent's **full system prompt** — which embeds the user's memory and profile — plus the
installed skill list, MCP server status and the absolute cwd. None of that belongs in a
public repository, so every frame passes through `redact_frame()` before it is written,
and existing fixtures can be cleaned in place.

The scrub is recursive and key-based: it redacts by field name at any depth, so it keeps
working when the gateway nests these payloads differently in a future version.

Usage:
    python scripts/fixture_redact.py <file.jsonl> [<file.jsonl> ...]
"""

import json
import os
import sys

REDACTED = "<redacted>"
#: Scalar fields whose value is environment-specific or personal.
_SCALAR_FIELDS = ("system_prompt", "stored_session_id", "conversation_id")
#: Collection fields that leak the user's installed capabilities.
_COLLECTION_FIELDS = ("skills", "mcp_servers", "skill_commands")


def _scrub(node) -> None:
    """Redact in place at any depth."""
    if isinstance(node, dict):
        for key, value in list(node.items()):
            if key in _SCALAR_FIELDS and value:
                node[key] = REDACTED
            elif key in _COLLECTION_FIELDS and isinstance(value, (dict, list)):
                node[key] = {} if isinstance(value, dict) else []
            elif key == "cwd" and isinstance(value, str) and value.startswith("/"):
                node[key] = "/redacted"
            else:
                _scrub(value)
    elif isinstance(node, list):
        for item in node:
            _scrub(item)


def redact_frame(frame: dict) -> dict:
    """Return `frame` with personal/environment fields redacted."""
    if not isinstance(frame, dict):
        return frame
    _scrub(frame)
    return frame


def redact_line(line: str) -> str:
    """Redact one JSONL line; non-JSON lines are returned stripped and unchanged."""
    stripped = line.strip()
    if not stripped:
        return stripped
    try:
        frame = json.loads(stripped)
    except json.JSONDecodeError:
        return stripped
    return json.dumps(redact_frame(frame), separators=(",", ":"), ensure_ascii=False)


def redact_file(path: str) -> int:
    """Rewrite `path` in place, redacted. Returns the number of lines written."""
    with open(path, encoding="utf-8") as handle:
        lines = [line for line in handle if line.strip()]
    redacted = [redact_line(line) for line in lines]
    tmp = f"{path}.tmp"
    with open(tmp, "w", encoding="utf-8") as handle:
        handle.write("\n".join(redacted) + "\n")
    os.replace(tmp, path)
    return len(redacted)


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    for path in argv[1:]:
        count = redact_file(path)
        print(f"redacted {count} frames: {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
