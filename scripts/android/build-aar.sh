#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

VERSION="0.1.0"
MIN_SDK="23"
REQUIRED_ABIS=("arm64-v8a" "armeabi-v7a" "x86_64")
TARGET_TRIPLES=("aarch64-linux-android" "armv7-linux-androideabi" "x86_64-linux-android")

echo "=== Building Kurmancî Android SDK v${VERSION} (minSdk=${MIN_SDK}) ==="

cd "$REPO_ROOT"

# Ensure cargo-ndk is installed
if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "cargo-ndk not found, installing cargo-ndk v3.5.0..."
  cargo install cargo-ndk --version 3.5.0
fi

# Locate NDK if ANDROID_NDK_HOME is not set
if [[ -z "${ANDROID_NDK_HOME:-}" && -z "${ANDROID_NDK_ROOT:-}" ]]; then
  POSSIBLE_NDKS=(
    "$HOME/Library/Android/sdk/ndk/26.1.10909125"
    "$HOME/Library/Android/sdk/ndk-bundle"
    "/usr/local/lib/android/sdk/ndk/26.1.10909125"
    "/usr/local/lib/android/sdk/ndk-bundle"
  )
  for ndk in "${POSSIBLE_NDKS[@]}"; do
    if [[ -d "$ndk" ]]; then
      export ANDROID_NDK_HOME="$ndk"
      break
    fi
  done
fi

if [[ -z "${ANDROID_NDK_HOME:-}" && -z "${ANDROID_NDK_ROOT:-}" ]]; then
  echo "⚠️ Warning: ANDROID_NDK_HOME not set. cargo-ndk will attempt auto-detection from ANDROID_HOME."
fi

# 1. Cross-compile native libkurmanci_jni.so for each target ABI
for i in "${!REQUIRED_ABIS[@]}"; do
  abi="${REQUIRED_ABIS[$i]}"
  triple="${TARGET_TRIPLES[$i]}"
  echo "-> Cross-compiling libkurmanci_jni.so for ABI ${abi} (${triple}, platform ${MIN_SDK})..."

  cargo ndk --target "${triple}" --platform "${MIN_SDK}" build --release -p kurmanci-jni

  JNI_STAGE_DIR="$REPO_ROOT/android/kurmanci/src/main/jniLibs/${abi}"
  mkdir -p "$JNI_STAGE_DIR"
  cp "$REPO_ROOT/target/${triple}/release/libkurmanci_jni.so" "$JNI_STAGE_DIR/libkurmanci_jni.so"
  echo "✅ Staged $JNI_STAGE_DIR/libkurmanci_jni.so"
done

# 2. Build Release AAR and publish directly to dist/android/maven
echo "Building Release AAR and publishing to local Maven repository..."
cd "$REPO_ROOT/android"
chmod +x ./gradlew
./gradlew :kurmanci:assembleRelease :kurmanci:publish

# 3. Copy AAR artifact to dist/
mkdir -p "$REPO_ROOT/dist"
AAR_OUTPUT="$REPO_ROOT/android/kurmanci/build/outputs/aar/kurmanci-release.aar"
DIST_AAR="$REPO_ROOT/dist/kurmanci-android-${VERSION}.aar"

if [[ -f "$AAR_OUTPUT" ]]; then
  cp "$AAR_OUTPUT" "$DIST_AAR"
  echo "✅ AAR artifact created at: $DIST_AAR"
else
  echo "❌ Error: AAR build output missing at $AAR_OUTPUT" >&2
  exit 1
fi

# 4. Verify AAR contents
echo "Verifying AAR structure and packaged native libraries..."
AAR_CONTENTS=$(unzip -l "$DIST_AAR")
echo "$AAR_CONTENTS"

for abi in "${REQUIRED_ABIS[@]}"; do
  if ! echo "$AAR_CONTENTS" | grep -q "jni/${abi}/libkurmanci_jni.so"; then
    echo "❌ Error: Missing native library jni/${abi}/libkurmanci_jni.so in AAR" >&2
    exit 1
  fi
done

if ! echo "$AAR_CONTENTS" | grep -q "classes.jar"; then
  echo "❌ Error: Missing classes.jar in AAR" >&2
  exit 1
fi

# 5. Verify local Maven repository publication
MAVEN_POM="$REPO_ROOT/dist/android/maven/org/kurmanci/kurmanci-android/${VERSION}/kurmanci-android-${VERSION}.pom"
MAVEN_AAR="$REPO_ROOT/dist/android/maven/org/kurmanci/kurmanci-android/${VERSION}/kurmanci-android-${VERSION}.aar"

if [[ -f "$MAVEN_POM" && -f "$MAVEN_AAR" ]]; then
  echo "✅ Deterministic local Maven publication verified at dist/android/maven/org/kurmanci/kurmanci-android/${VERSION}/"
else
  echo "❌ Error: Local Maven repository publication incomplete at $MAVEN_POM" >&2
  exit 1
fi

echo "=== Android SDK v${VERSION} build and packaging completed successfully ==="
