#!/bin/bash
# device_install.sh — build the hermo Android app (device ABI) and install it
# on the USB-connected device, then launch. Always: build -> install -> launch.
# SDK env exported inline (same values as t11_gate.sh).
set -e
cd "$(dirname "$0")/.."
PKG=sh.mo
ACTIVITY="$PKG/.MainActivity"

export ANDROID_HOME=~/Android/Sdk ANDROID_SDK_ROOT=~/Android/Sdk
export ANDROID_NDK_HOME=/opt/android-ndk JAVA_HOME=/opt/android-studio/jbr
bazel build //android/app:app --config=device --config=lowmem
adb install -r bazel-bin/android/app/app.apk

adb shell am start -n "$ACTIVITY"
# Foreground check: print the resumed activity so the launch is verifiable.
sleep 1
adb shell dumpsys activity activities 2>/dev/null | grep -m1 "topResumedActivity" || true
