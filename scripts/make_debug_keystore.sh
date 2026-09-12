#!/usr/bin/env bash
# make_debug_keystore.sh — generate the app's debug signing keystore.
#
# Standard Android debug credentials (what the SDK/Gradle convention expects):
# alias androiddebugkey, store and key password "android", CN=Android Debug.
# This key is debug-only and carries no authority anywhere; it exists purely
# so `adb install` accepts the APK. Regenerating it invalidates in-place
# upgrades (uninstall first).
set -euo pipefail
OUT="$(cd "$(dirname "$0")/.." && pwd)/android/app/debug.keystore"
if [[ -f "$OUT" ]]; then
  echo "keystore already exists: $OUT"
  exit 0
fi
/opt/android-studio/jbr/bin/keytool -genkeypair \
  -keystore "$OUT" \
  -alias androiddebugkey \
  -storepass android -keypass android \
  -keyalg RSA -keysize 2048 -validity 10000 \
  -dname "CN=Android Debug,O=Android,C=US"
echo "generated: $OUT"
