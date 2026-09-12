#!/usr/bin/env bash
# build_android.sh — build the hermo Android app and deploy to an
# emulator/device. Full-Bazel build of record: the app is the Bazel
# android_binary //android/app:app (T6 stream A).
#
# Usage:
#   ./scripts/build_android.sh              # build only (emulator ABI)
#   ./scripts/build_android.sh --install    # build + boot emulator (if none online) + install
#   ./scripts/build_android.sh --run        # build + boot emulator (if needed) + install + launch
#   DEVICE=1 ...                            # arm64 device ABI; default x86_64 emulator
#   LOWMEM=1 ...                            # --config=lowmem (jobs=2) anti-freeze profile
#   AVD=MyAvd ./scripts/build_android.sh --run   # pick a specific AVD
#   NO_SHUTDOWN=1 ...                       # keep the bazel server alive after the build
#
# Exit codes: 0 = ok, non-zero = failure.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-${HOME}/Android/Sdk}"
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-/opt/android-ndk}"
export JAVA_HOME="${JAVA_HOME:-/opt/android-studio/jbr}"
export PATH="${HOME}/.cargo/bin:${PATH}"

ADB="$ANDROID_HOME/platform-tools/adb"
EMU="$ANDROID_HOME/emulator/emulator"
AVD="${AVD:-JustNutrients_API36}"
PKG="sh.mo"

MODE="build"
for arg in "$@"; do
  case "$arg" in
    --install) MODE="install" ;;
    --run)     MODE="run" ;;
    --help|-h)
      grep '^#' "$0" | head -16; exit 0 ;;
    *) echo "unknown arg: $arg"; exit 2 ;;
  esac
done

echo "== [1/2] Bazel build: core natives + UniFFI bindings + android_binary =="
cd "$ROOT"
BAZEL_TARGET="//android/app:app"
# ABI selection (rules_android 0.7 starlark): --android_platforms drives the
# split transition and aar_import jni/<abi> extraction; --fat_apk_cpu is kept
# only for legacy flag compatibility. DEVICE=1 targets arm64 hardware;
# default remains the x86_64 emulator. LOWMEM=1 adds --config=lowmem (jobs=2)
# — use on this laptop: full-parallel bazel froze the machine before.
CONFIGS=()
[[ "${DEVICE:-0}" == "1" ]] && CONFIGS+=(--config=device) || CONFIGS+=(--config=emulator)
[[ "${LOWMEM:-0}" == "1" ]] && CONFIGS+=(--config=lowmem)
bazel build "$BAZEL_TARGET" "${CONFIGS[@]}"

# Debug: the canonical signed `app.apk`. NEVER install `zipaligned_app.apk` —
# it is a pre-signing intermediate (no certificates).
APK="bazel-bin/android/app/app.apk"

[[ -f "$APK" ]] || { echo "ERROR: APK not found: $APK"; exit 1; }
echo "APK: $APK"

# Sanity check: verify the packaged ABI matches what was requested BEFORE any
# install attempt (stale APK from another config fails on-device with
# INSTALL_FAILED_NO_MATCHING_ABIS).
EXPECTED_ABI="arm64-v8a"; [[ "${DEVICE:-0}" == "1" ]] || EXPECTED_ABI="x86_64"
if ! grep -q "lib/$EXPECTED_ABI/" <(unzip -l "$APK"); then
  echo "ERROR: $APK does not contain lib/$EXPECTED_ABI/ native libs."
  unzip -l "$APK" | grep 'lib/' || true
  exit 1
fi

if [[ "$MODE" == "build" ]]; then
  echo "== done (build only). Use --install or --run to deploy. =="
  [[ "${NO_SHUTDOWN:-0}" == "1" ]] || bazel shutdown
  exit 0
fi

# ---- deployment ----
# ensure_emulator: if no emulator/device is actually online, boot the
# configured AVD headless and wait for it.
ensure_emulator() {
  local online
  online="$("$ADB" devices | awk 'NR>1 && $2=="device"{print $1}' | head -1)"
  if [[ -n "$online" ]]; then
    echo "device already online: $online"
    return 0
  fi
  echo "== booting emulator $AVD (headless) =="
  "$EMU" -avd "$AVD" -no-window -no-audio -no-boot-anim -no-snapshot-load >/dev/null 2>&1 &
  EMU_PID=$!
  if ! timeout 120 "$ADB" wait-for-device; then
    echo "ERROR: emulator $AVD never came online" >&2
    kill "$EMU_PID" 2>/dev/null || true
    exit 1
  fi
  local booted=0
  for _ in $(seq 1 60); do
    if [[ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]]; then
      booted=1
      break
    fi
    sleep 3
  done
  if [[ "$booted" != "1" ]]; then
    echo "ERROR: emulator $AVD did not finish booting (sys.boot_completed)" >&2
    kill "$EMU_PID" 2>/dev/null || true
    exit 1
  fi
  echo "emulator booted: $("$ADB" devices | awk 'NR>1 && $2=="device"{print $1}')"
}

ensure_emulator

echo "== [2/2] install + launch =="
"$ADB" install -r "$APK"
"$ADB" shell am start -n "$PKG/.MainActivity"
echo "== done: $PKG launched =="

# Free the bazel server + persistent workers (~1.5-2GB idle RAM). The analysis
# cache is lost, so the next build re-pays analysis; opt out with NO_SHUTDOWN=1.
[[ "${NO_SHUTDOWN:-0}" == "1" ]] || bazel shutdown
