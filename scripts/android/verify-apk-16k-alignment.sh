#!/usr/bin/env bash
# Fail-closed check that every uncompressed entry of an APK, the native libraries included, sits
# at a 16 KB-aligned offset (`zipalign -c -P 16 4`), which 16 KB-page devices need to map
# libkurmanci_jni.so directly from the APK. The Android Gradle Plugin 8.5.1+ packages this way;
# the check makes sure the consumer build really did. zipalign comes from $ZIPALIGN, the
# newest build-tools under $ANDROID_HOME / $ANDROID_SDK_ROOT, or PATH.
#
# Usage: scripts/android/verify-apk-16k-alignment.sh <app.apk>
set -euo pipefail
APK="${1:-}"
[[ -n "$APK" && -f "$APK" ]] || { echo "usage: $0 <apk>  (file not found: '${APK:-}')" >&2; exit 2; }
ZIPALIGN="${ZIPALIGN:-}"
if [[ -z "$ZIPALIGN" ]]; then
  for candidate in "${ANDROID_HOME:-/nonexistent}"/build-tools/*/zipalign "${ANDROID_SDK_ROOT:-/nonexistent}"/build-tools/*/zipalign; do
    [[ -x "$candidate" ]] && ZIPALIGN="$candidate"   # glob order: the newest build-tools wins
  done
fi
if [[ -z "$ZIPALIGN" ]] && command -v zipalign >/dev/null 2>&1; then ZIPALIGN="$(command -v zipalign)"; fi
[[ -n "$ZIPALIGN" ]] || { echo "❌ zipalign not found (set ANDROID_HOME or ZIPALIGN)" >&2; exit 2; }
OUT="$("$ZIPALIGN" -c -v -P 16 4 "$APK" 2>&1)" || {
  echo "❌ $APK is not 16 KB-aligned (zipalign -c -P 16 4):" >&2
  printf '%s\n' "$OUT" | grep -E 'BAD|lib/|Verif' >&2
  exit 1
}
printf '%s\n' "$OUT" | grep -E 'libkurmanci_jni' | sed 's/^/   /'
echo "✅ $(basename "$APK"): zipalign -c -P 16 4 verification successful (native libraries at 16 KB-aligned offsets)"
