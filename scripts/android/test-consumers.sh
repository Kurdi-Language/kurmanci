#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

VERSION="0.1.0"

echo "=== Running Android Consumer Integration Tests ==="

cd "$REPO_ROOT"

# 1. Verify build-aar.sh artifact exists or build if missing
MAVEN_AAR="$REPO_ROOT/dist/android/maven/org/kurmanci/kurmanci-android/${VERSION}/kurmanci-android-${VERSION}.aar"

if [[ ! -f "$MAVEN_AAR" ]]; then
  echo "Building Android AAR and staging to local Maven repository..."
  "$REPO_ROOT/scripts/android/build-aar.sh"
fi

# 2. Run SDK unit tests
echo "-> Running JVM unit tests for Kurmancî Android SDK..."
cd "$REPO_ROOT/android"
./gradlew :kurmanci:test

# 3. Test clean Android consumer resolution and compilation
echo "-> Testing clean Android consumer resolution against local Maven repository..."
cd "$REPO_ROOT/integration/android/android-consumer"
chmod +x ../../../android/gradlew || true

# Copy gradle wrapper if needed
if [[ ! -f "./gradlew" ]]; then
  cp -r "$REPO_ROOT/android/gradle"* .
  cp "$REPO_ROOT/android/gradlew"* .
fi

./gradlew assembleDebug testDebugUnitTest

echo "✅ Android consumer build and JVM unit tests passed."

# 4. Connected Android Instrumentation Test
if command -v adb >/dev/null 2>&1 && adb devices | grep -q "device$"; then
  echo "-> Connected Android device/emulator detected, running instrumentation tests..."
  ./gradlew connectedDebugAndroidTest
  echo "✅ Android instrumentation tests passed."
elif [[ "${REQUIRE_INSTRUMENTATION:-0}" == "1" || "${CI:-false}" == "true" ]]; then
  echo "❌ Error: Mandatory Android instrumentation testing enabled (CI=true), but no active Android device/emulator found via adb." >&2
  exit 1
else
  echo "ℹ️ Note: No running Android emulator/device detected via adb. Skipping connectedDebugAndroidTest."
fi

echo "=== Android Consumer Integration Tests completed successfully ==="
