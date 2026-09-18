#!/usr/bin/env bash
# Clean-room proof for the Android SDK: a standalone Kotlin consumer application resolves the
# packaged AAR from a Maven repository layout, builds, runs its JVM unit tests and exercises
# known / correct / complete / predict on a connected emulator or device, with no Rust
# toolchain, Cargo, cargo-ndk or NDK involved. Nothing native is built here; the AAR must
# already exist (from scripts/android/build-aar.sh or a downloaded CI artifact).
#
# Usage: scripts/android/verify-clean-room-consumer.sh
#   ALLOW_RUST_ON_PATH=1   local developer runs where cargo is installed (it is still unused)
#   CI=true                makes the instrumentation run mandatory (fails without a device)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEFAULT_GROUP="$(grep '^kurmanciMavenGroup=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo 'io.github.ferhatguneri')"
DEFAULT_VERSION="$(grep '^kurmanciVersion=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo '0.1.0')"
VERSION="${VERSION:-$DEFAULT_VERSION}"
GROUP_ID="${GROUP_ID:-$DEFAULT_GROUP}"
GROUP_PATH="${GROUP_ID//./\/}"
MAVEN_DIR="$REPO_ROOT/dist/android/maven/$GROUP_PATH/kurmanci-android/$VERSION"
AAR="$MAVEN_DIR/kurmanci-android-$VERSION.aar"
POM="$MAVEN_DIR/kurmanci-android-$VERSION.pom"

echo "=== Android clean-room consumer verification (group=$GROUP_ID version=$VERSION) ==="

# 1. No Rust toolchain may take part. In CI the runner PATH is stripped of Rust before this
#    script runs; a developer machine may keep cargo installed, but it is never invoked.
for tool in cargo rustc cargo-ndk; do
  if command -v "$tool" >/dev/null 2>&1; then
    if [[ "${ALLOW_RUST_ON_PATH:-0}" == "1" ]]; then
      echo "note: $tool is on PATH (ALLOW_RUST_ON_PATH=1); it is not used by this verification"
    else
      echo "❌ $tool is on PATH; the clean-room consumer must run without a Rust toolchain (set ALLOW_RUST_ON_PATH=1 only for local runs)" >&2
      exit 1
    fi
  fi
done
if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
  echo "note: ANDROID_NDK_HOME is set; the consumer build does not use it"
fi
echo "✅ no Rust toolchain is used"

# 2. The prebuilt SDK artifact must already be in the Maven layout the consumer resolves from.
if [[ ! -f "$AAR" || ! -f "$POM" ]]; then
  echo "❌ packaged SDK not found at $MAVEN_DIR (expected kurmanci-android-$VERSION.aar and .pom)." >&2
  echo "   Build it with scripts/android/build-aar.sh (needs Rust + NDK) or download the CI artifact 'android-aar-artifacts' into dist/." >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then AAR_SHA="$(sha256sum "$AAR" | awk '{print $1}')"; else AAR_SHA="$(shasum -a 256 "$AAR" | awk '{print $1}')"; fi
echo "AAR: $AAR"
echo "     sha256 $AAR_SHA, $(wc -c < "$AAR" | tr -d ' ') bytes"
ABIS="$(unzip -Z1 "$AAR" | grep -E '^jni/[^/]+/libkurmanci_jni\.so$' | cut -d/ -f2 | sort | paste -sd, -)"
echo "     prebuilt native libraries: ${ABIS:-none}"
for abi in arm64-v8a armeabi-v7a x86_64; do
  if ! unzip -Z1 "$AAR" | grep -q "^jni/$abi/libkurmanci_jni\.so$"; then
    echo "❌ AAR lacks jni/$abi/libkurmanci_jni.so" >&2
    exit 1
  fi
done
echo "✅ AAR carries every supported ABI; the consumer receives prebuilt natives"

# 3. Standalone consumer: resolve the AAR from the Maven layout, build, JVM unit tests.
cd "$REPO_ROOT/integration/android/android-consumer"
if [[ ! -f "./gradlew" ]]; then
  # Only the wrapper: the glob android/gradle* would also copy android/gradle.properties over
  # the consumer's own tracked gradle.properties.
  cp -r "$REPO_ROOT/android/gradle" .
  cp "$REPO_ROOT/android/gradlew"* .
fi
chmod +x ./gradlew
export CONSUMER_MODE=local
./gradlew --quiet assembleDebug testDebugUnitTest -PkurmanciMavenGroup="$GROUP_ID" -PkurmanciVersion="$VERSION"
echo "✅ consumer resolved $GROUP_ID:kurmanci-android:$VERSION from dist/android/maven, built and passed JVM unit tests"
# The consumer APK must place the native libraries at 16 KB-aligned offsets (AGP 8.5.1+).
"$REPO_ROOT/scripts/android/verify-apk-16k-alignment.sh" app/build/outputs/apk/debug/app-debug.apk

# 4. Instrumentation on a connected emulator or device: load pack, known, correct, complete,
#    predict (AndroidInstrumentationTest.testCleanRoomContractKnownCorrectCompletePredict and
#    the surrounding suite). Mandatory in CI.
if command -v adb >/dev/null 2>&1 && adb devices | grep -q "device$"; then
  ./gradlew --quiet connectedDebugAndroidTest -PkurmanciMavenGroup="$GROUP_ID" -PkurmanciVersion="$VERSION"
  echo "✅ instrumentation passed on the connected device: load pack, known, correct, complete, predict"
elif [[ "${REQUIRE_INSTRUMENTATION:-0}" == "1" || "${CI:-false}" == "true" ]]; then
  echo "❌ no Android device/emulator via adb, and instrumentation is mandatory (CI=true)" >&2
  exit 1
else
  echo "ℹ️ no Android device/emulator via adb; instrumentation skipped (set REQUIRE_INSTRUMENTATION=1 to require it)"
fi

echo "=== clean-room verification passed: standalone Kotlin consumer → packaged AAR ($AAR_SHA) → no Cargo/Rust/NDK → load pack → known → correct → complete → predict ==="
