#!/usr/bin/env bash
# Record the hermo protocol fixture against a throwaway loopback Hermes
# backend (PLAN.md T0a). Backend must already be running on $HERMO_PORT.
# The session token lives in /tmp/hermo-session-token, NEVER in the repo.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"

if [ -n "${HERMO_PYTHON:-}" ]; then
    PY="$HERMO_PYTHON"
elif [ -x "$HOME/.hermes/hermes-agent/venv/bin/python" ]; then
    PY="$HOME/.hermes/hermes-agent/venv/bin/python"
else
    PY="$REPO/.venv-tools/bin/python"
fi

exec "$PY" "$HERE/record_fixture.py"