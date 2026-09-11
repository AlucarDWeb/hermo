#!/usr/bin/env python
"""Redact environment and personal fields from recorded protocol fixtures.

A live `session.info` frame (and, in some shapes, a `session.create` result) carries the
agent's **full system prompt** — which embeds the user's memory and profile — plus the
installed skill list, MCP server status and the absolute cwd. None of that belongs in a
public repository, so every frame passes through `redact_frame()` before it is written,
and existing fixtures can be cleaned in place.

The field lists are deliberately wider than today's payloads: `instructions`, the
`working_directory` aliases, camelCase prompt aliases and a **string** `prompt` (a numeric
`usage.prompt` token count must survive) are redacted too, and `tools` is emptied alongside
`skills` / `mcp_servers` / `skill_commands`, because a future gateway shape must not slip
through the same list that missed the durable `session.title.session_id`.

The scrub is recursive and key-based (by field name, at any depth). Durable session ids
(`session_id` / `stored_session_id`) are redacted by VALUE in event payloads
(the key is kept so the shape stays recognizable); the per-turn `session_id` on `params`
is a short-lived id and stays untouched.

Usage:
    python scripts/fixture_redact.py <file.jsonl> [<file.jsonl> ...]

Tests: `python3 -m unittest` from `scripts/`.
"""

import json
import os
import sys

REDACTED = "<redacted>"
#: Scalar fields whose value is environment-specific or personal.
_SCALAR_FIELDS = (
    "system_prompt",
    "systemPrompt",
    "instructions",
    "stored_session_id",
    "storedSessionId",
    "conversation_id",
    "conversationId",
    "session_key",
    "sessionKey",
    "working_directory",
    "working_dir",
)
#: Collection fields that leak the user's installed capabilities or environment.
_COLLECTION_FIELDS = ("skills", "mcp_servers", "skill_commands", "tools")
#: Durable-id fields whose VALUE is redacted wherever they appear (key kept).
_SESSION_ID_VALUE_FIELDS = ("session_id", "stored_session_id", "sessionId", "storedSessionId")


def _scrub(node) -> None:
    """Redact in place at any depth."""
    if isinstance(node, dict):
        for key, value in list(node.items()):
            if key in _SCALAR_FIELDS and value:
                node[key] = REDACTED
            elif key in _COLLECTION_FIELDS and isinstance(value, (dict, list)):
                node[key] = {} if isinstance(value, dict) else []
            elif key == "cwd" and isinstance(value, str) and value:
                node[key] = "/redacted"
            elif key in _SESSION_ID_VALUE_FIELDS and isinstance(value, str) and value:
                # Redact the durable-id VALUE, keep the key so the shape stays.
                node[key] = REDACTED
            elif key == "prompt" and isinstance(value, str) and value:
                # `usage.prompt` is a numeric token count and must survive; a string
                # `prompt` is a prompt body wherever it appears.
                node[key] = REDACTED
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
