#!/usr/bin/env bash
# fake_gateway.sh: start/stop/check the hermes-fake-gateway dev binary
# (PLAN §3.5 T04), the local stand-in Hermes gateway used to exercise the
# iOS app and hermes-probe against recorded fixtures without a live server.
#
# Usage:
#   scripts/fake_gateway.sh [start] [--port <n>] [--user <name>] [--password <pw>]
#                            [--fixture <path>] [--drop-after <n>]
#   scripts/fake_gateway.sh stop
#   scripts/fake_gateway.sh status
#
#   HERMO_FAKE_GATEWAY_DIR=<dir>   where the pid and log files live
#                                  (default: ${TMPDIR:-/tmp}/hermo-fake-gateway)
#
# start builds hermes-fake-gateway in release mode inside tools/fake_gateway,
# launches it in the background with stdout and stderr redirected to a log
# file, records its pid, waits up to 10s for /api/status to answer, then
# prints the pairing payload read back from the log. stop kills the
# recorded pid and removes the pid file; it is a no-op with exit 0 when
# nothing is running. status prints whether the gateway is running and
# exits 0 or 1 to match.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GW_DIR="$ROOT/tools/fake_gateway"

GATEWAY_DIR="${HERMO_FAKE_GATEWAY_DIR:-${TMPDIR:-/tmp}/hermo-fake-gateway}"
PID_FILE="$GATEWAY_DIR/gateway.pid"
LOG_FILE="$GATEWAY_DIR/gateway.log"

CMD="start"
case "${1:-}" in
    start|stop|status) CMD="$1"; shift ;;
esac

PORT=""
GW_USER=""
GW_PASSWORD=""
FIXTURE=""
DROP_AFTER=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --port)       PORT="$2"; shift 2 ;;
        --user)       GW_USER="$2"; shift 2 ;;
        --password)   GW_PASSWORD="$2"; shift 2 ;;
        --fixture)    FIXTURE="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
        --drop-after) DROP_AFTER="$2"; shift 2 ;;
        -h|--help)
            awk 'NR > 1 && !/^#/ { exit } NR > 1 { sub(/^# ?/, ""); print }' "$0"; exit 0 ;;
        *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
done

mkdir -p "$GATEWAY_DIR"

is_running() {
    [[ -f "$PID_FILE" ]] || return 1
    kill -0 "$(cat "$PID_FILE")" 2>/dev/null
}

do_status() {
    if is_running; then
        echo "hermes-fake-gateway: running (pid $(cat "$PID_FILE"))"
        return 0
    fi
    echo "hermes-fake-gateway: not running"
    return 1
}

do_stop() {
    if ! is_running; then
        rm -f "$PID_FILE"
        echo "hermes-fake-gateway: not running"
        return 0
    fi
    local pid
    pid="$(cat "$PID_FILE")"
    kill "$pid" 2>/dev/null || true
    rm -f "$PID_FILE"
    echo "hermes-fake-gateway: stopped (pid $pid)"
}

do_start() {
    if is_running; then
        echo "hermes-fake-gateway: already running (pid $(cat "$PID_FILE"))" >&2
        exit 1
    fi

    echo "== building hermes-fake-gateway ==" >&2
    (cd "$GW_DIR" && cargo build --release --bin hermes-fake-gateway --jobs=4)

    local bin_args=()
    [[ -n "$PORT" ]] && bin_args+=(--port "$PORT")
    [[ -n "$GW_USER" ]] && bin_args+=(--user "$GW_USER")
    [[ -n "$GW_PASSWORD" ]] && bin_args+=(--password "$GW_PASSWORD")
    [[ -n "$FIXTURE" ]] && bin_args+=(--fixture "$FIXTURE")
    [[ -n "$DROP_AFTER" ]] && bin_args+=(--drop-after "$DROP_AFTER")

    : > "$LOG_FILE"
    (
        cd "$GW_DIR"
        nohup "$GW_DIR/target/release/hermes-fake-gateway" ${bin_args[@]+"${bin_args[@]}"} >>"$LOG_FILE" 2>&1 &
        echo $! > "$PID_FILE"
    )
    local pid
    pid="$(cat "$PID_FILE")"

    local status_port="${PORT:-9123}"
    local waited=0
    while (( waited < 10 )); do
        if curl -sf -o /dev/null "http://127.0.0.1:$status_port/api/status"; then
            break
        fi
        if ! kill -0 "$pid" 2>/dev/null; then
            echo "hermes-fake-gateway: process exited before answering, see $LOG_FILE" >&2
            rm -f "$PID_FILE"
            exit 1
        fi
        sleep 1
        waited=$((waited + 1))
    done

    if ! curl -sf -o /dev/null "http://127.0.0.1:$status_port/api/status"; then
        echo "hermes-fake-gateway: did not answer /api/status within 10s, see $LOG_FILE" >&2
        exit 1
    fi

    local payload=""
    local tries=0
    while (( tries < 20 )); do
        payload="$(grep '^hermes://connect' "$LOG_FILE" | tail -n 1 || true)"
        [[ -n "$payload" ]] && break
        sleep 0.2
        tries=$((tries + 1))
    done
    if [[ -z "$payload" ]]; then
        echo "hermes-fake-gateway: no pairing payload in $LOG_FILE" >&2
        exit 1
    fi
    echo "$payload"
}

case "$CMD" in
    start)  do_start ;;
    stop)   do_stop ;;
    status) do_status ;;
esac
