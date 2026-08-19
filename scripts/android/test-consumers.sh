#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEFAULT_GROUP="$(grep '^kurmanciMavenGroup=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo 'io.github.ferhatguneri')"
DEFAULT_VERSION="$(grep '^kurmanciVersion=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo '0.1.0')"

VERSION="${VERSION:-$DEFAULT_VERSION}"
GROUP_ID="${GROUP_ID:-$DEFAULT_GROUP}"
CONSUMER_MODE="${CONSUMER_MODE:-local}"
GROUP_PATH="${GROUP_ID//./\/}"

echo "=== Running Android Consumer Integration Tests (mode=${CONSUMER_MODE}, group=${GROUP_ID}) ==="

cd "$REPO_ROOT"

# 1. If CONSUMER_MODE=local, verify build-aar.sh artifact exists or build if missing
if [[ "$CONSUMER_MODE" == "local" ]]; then
  MAVEN_AAR="$REPO_ROOT/dist/android/maven/$GROUP_PATH/kurmanci-android/${VERSION}/kurmanci-android-${VERSION}.aar"
  if [[ ! -f "$MAVEN_AAR" ]]; then
    echo "Building Android AAR and staging to local Maven repository..."
    VERSION="$VERSION" GROUP_ID="$GROUP_ID" "$REPO_ROOT/scripts/android/build-aar.sh"
  fi
fi

# 2. Run SDK unit tests
echo "-> Running JVM unit tests for Kurmancî Android SDK..."
cd "$REPO_ROOT/android"
chmod +x ./gradlew
./gradlew :kurmanci:test

# 3. Test clean Android consumer resolution and compilation
echo "-> Testing clean Android consumer resolution in mode=${CONSUMER_MODE}..."
cd "$REPO_ROOT/integration/android/android-consumer"

if [[ ! -f "./gradlew" ]]; then
  cp -r "$REPO_ROOT/android/gradle"* .
  cp "$REPO_ROOT/android/gradlew"* .
fi
chmod +x ./gradlew

export CONSUMER_MODE="$CONSUMER_MODE"
./gradlew assembleDebug testDebugUnitTest -PkurmanciMavenGroup="${GROUP_ID}" -PkurmanciVersion="${VERSION}"

echo "✅ Android consumer build and JVM unit tests passed in mode=${CONSUMER_MODE}."

# 4. Connected Android Instrumentation Test
if command -v adb >/dev/null 2>&1 && adb devices | grep -q "device$"; then
  echo "-> Connected Android device/emulator detected, running instrumentation tests..."
  ./gradlew connectedDebugAndroidTest -PkurmanciMavenGroup="${GROUP_ID}" -PkurmanciVersion="${VERSION}"
  echo "✅ Android instrumentation tests passed."
elif [[ "${REQUIRE_INSTRUMENTATION:-0}" == "1" || "${CI:-false}" == "true" ]]; then
  echo "❌ Error: Mandatory Android instrumentation testing enabled (CI=true), but no active Android device/emulator found via adb." >&2
  exit 1
else
  echo "ℹ️ Note: No running Android emulator/device detected via adb. Skipping connectedDebugAndroidTest."
fi

echo "=== Android Consumer Integration Tests completed successfully ==="
