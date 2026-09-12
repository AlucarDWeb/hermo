#!/usr/bin/env bash
# hermes-qr.sh — build the hermes://connect QR payload for a Hermes gateway
# and render it (PLAN §4 T6 item 7 / T12, folded into T6b as the other half
# of the pairing).
#
# Payload contract (the core's `parse_qr_payload`, hermes_core/src/auth/endpoint.rs):
#   hermes://connect?v=1&url=<percent-encoded base URL>&user=<username>&name=<display name>
#   - every value percent-encoded (a percent-decoding pass, `+` preserved)
#   - v must be 1, the URL scheme http/https, no embedded credentials
#   - never carries a secret: URL + username + display name only.
#
# Usage:
#   scripts/hermes-qr.sh --url http://192.168.1.48:9123 --user hermo [--name hermo-lan] [--png /tmp/qr.png]
#
# Rendering: `qrencode -t ANSIUTF8` for the terminal; --png <path> writes a
# PNG. If qrencode is missing the payload is still printed (a QR the app
# cannot parse is worthless, but a payload without a QR is still usable —
# the app's fallback field accepts it verbatim).

set -euo pipefail

URL=""
USER_NAME=""
DISPLAY_NAME=""
PNG=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --url)  URL="$2"; shift 2 ;;
        --user) USER_NAME="$2"; shift 2 ;;
        --name) DISPLAY_NAME="$2"; shift 2 ;;
        --png)  PNG="$2"; shift 2 ;;
        -h|--help)
            grep '^#' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
done

if [[ -z "$URL" || -z "$USER_NAME" ]]; then
    echo "usage: $0 --url <gateway base URL> --user <username> [--name <display name>] [--png <path>]" >&2
    exit 2
fi

# scheme check here too: the core rejects anything but http/https, and the
# script should fail before rendering a QR the app will refuse.
case "$URL" in
    http://*|https://*) ;;
    *) echo "error: --url must start with http:// or https://" >&2; exit 2 ;;
esac

# Percent-encode with Python (present on any dev box; stdlib only).
# safe="" keeps even / : @ encoded — the payload is URL-in-URL.
payload="$(python3 - "$URL" "$USER_NAME" "${DISPLAY_NAME:-$USER_NAME}" <<'PY'
import sys
from urllib.parse import quote
url, user, name = sys.argv[1:4]
print(f"hermes://connect?v=1&url={quote(url, safe='')}&user={quote(user, safe='')}&name={quote(name, safe='')}")
PY
)"

echo "$payload"

if [[ -n "$PNG" ]]; then
    if command -v qrencode >/dev/null 2>&1; then
        qrencode -o "$PNG" -s 6 -m 2 "$payload"
        echo "QR written to $PNG" >&2
    else
        echo "qrencode not installed: payload printed above, no PNG written" >&2
    fi
fi

if command -v qrencode >/dev/null 2>&1; then
    qrencode -t ANSIUTF8 "$payload"
else
    echo "qrencode not installed: scan the payload by pasting it into hermo's fallback field" >&2
fi
