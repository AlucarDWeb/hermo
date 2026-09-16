#!/usr/bin/env bash
# ios_simulator_install.sh: build the hermo iOS app and deploy to a booted
# simulator (boots the default device if none is online).
#
# Usage:
#   ./scripts/ios_simulator_install.sh                       # build only
#   ./scripts/ios_simulator_install.sh --install             # build + boot simulator (if none booted) + install
#   ./scripts/ios_simulator_install.sh --run                 # build + boot simulator (if needed) + install + launch
#   ./scripts/ios_simulator_install.sh --run --screenshot <path>  # also capture a screenshot 3s after launch
#
#   SIM="iPhone 17" ./scripts/ios_simulator_install.sh --run   # pick a specific simulator name (default: iPhone 17)
#   NO_SHUTDOWN=1 ...                                           # keep the bazel server alive after the run
#
# Exit codes: 0 = ok, non-zero = failure.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

SIM="${SIM:-iPhone 17}"
BUNDLE_ID="sh.mo"

MODE="build"
SCREENSHOT=""

# Free the bazel server (~1.5-2GB idle RAM) like the Android script; the
# analysis cache is lost, so NO_SHUTDOWN=1 keeps it for back-to-back runs.
finish() {
  [[ "${NO_SHUTDOWN:-0}" == "1" ]] || bazel shutdown
  exit 0
}
while [[ $# -gt 0 ]]; do
  case "$1" in
    --install) MODE="install" ;;
    --run)     MODE="run" ;;
    --screenshot)
      [[ $# -ge 2 ]] || { echo "ERROR: --screenshot requires a path argument"; exit 2; }
      SCREENSHOT="$2"; shift ;;
    --help|-h)
      awk 'NR > 1 && !/^#/ { exit } NR > 1 { sub(/^# ?/, ""); print }' "$0"; exit 0 ;;
    *) echo "unknown arg: $1"; exit 2 ;;
  esac
  shift
done

echo "== [1/2] Bazel build: hermo ios_application =="
cd "$ROOT"
bazel build //ios/App:hermo --config=ios_sim --jobs=4

IPA="bazel-bin/ios/App/hermo.ipa"
[[ -f "$IPA" ]] || { echo "ERROR: ipa not found: $IPA"; exit 1; }
echo "IPA: $IPA"

if [[ "$MODE" == "build" ]]; then
  echo "== done (build only). Use --install or --run to deploy. =="
  finish
fi

# resolve_device: pick the simulator matching $SIM, preferring one already
# booted, and print "<udid> <state>".
resolve_device() {
  xcrun simctl list devices available -j | python3 -c '
import json, sys
data = json.load(sys.stdin)
name = sys.argv[1]
candidates = []
for runtime, devices in data.get("devices", {}).items():
    for d in devices:
        if d.get("name") == name:
            candidates.append(d)
if not candidates:
    sys.exit(1)
candidates.sort(key=lambda d: d.get("state") != "Booted")
print(candidates[0]["udid"], candidates[0]["state"])
' "$SIM"
}

DEVICE="$(resolve_device)" || { echo "ERROR: no simulator named \"$SIM\" found"; exit 1; }
read -r UDID STATE <<<"$DEVICE"
echo "UDID: $UDID ($STATE)"

if [[ "$STATE" == "Shutdown" ]]; then
  echo "== booting simulator $UDID =="
  xcrun simctl boot "$UDID"
  xcrun simctl bootstatus "$UDID" -b
fi

echo "== [2/2] install =="
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
unzip -q "$IPA" -d "$TMP"
APP_PATH="$TMP/Payload/hermo.app"
[[ -d "$APP_PATH" ]] || { echo "ERROR: app bundle not found: $APP_PATH"; exit 1; }
xcrun simctl install "$UDID" "$APP_PATH"
echo "APP: $APP_PATH"

if [[ "$MODE" == "install" ]]; then
  echo "== done: $BUNDLE_ID installed =="
  finish
fi

echo "== launch =="
xcrun simctl terminate "$UDID" "$BUNDLE_ID" || true
PID="$(xcrun simctl launch "$UDID" "$BUNDLE_ID" | awk '{print $2}')"
echo "PID: $PID"

if [[ -n "$SCREENSHOT" ]]; then
  sleep 3
  xcrun simctl io "$UDID" screenshot "$SCREENSHOT"
  echo "SCREENSHOT: $SCREENSHOT"
fi

echo "== done: $BUNDLE_ID launched =="
finish
