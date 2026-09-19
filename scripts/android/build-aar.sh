#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEFAULT_GROUP="$(grep '^kurmanciMavenGroup=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo 'io.github.ferhatguneri')"

# shellcheck source=scripts/android/kurmanci-version.sh
source "$SCRIPT_DIR/kurmanci-version.sh"
VERSION="$(resolve_kurmanci_version "$REPO_ROOT")"
GROUP_ID="${GROUP_ID:-$DEFAULT_GROUP}"
GROUP_PATH="${GROUP_ID//./\/}"
MIN_SDK="23"
REQUIRED_ABIS=("arm64-v8a" "armeabi-v7a" "x86_64")
TARGET_TRIPLES=("aarch64-linux-android" "armv7-linux-androideabi" "x86_64-linux-android")

echo "=== Building Kurmancî Android SDK v${VERSION} for group ${GROUP_ID} (minSdk=${MIN_SDK}) ==="

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

# 16 KB page-size invariants (every LOAD segment aligned to at least 16 KB; the GNU_RELRO region
# present and ending on a 16 KB boundary; .cargo/config.toml sets max-page-size and
# common-page-size) are checked fail-closed on every staged library and again on the exact
# bytes packaged in the AAR by scripts/android/verify-elf-page-alignment.sh.
VERIFY_ELF="$SCRIPT_DIR/verify-elf-page-alignment.sh"
[[ -x "$VERIFY_ELF" ]] || { echo "❌ Error: $VERIFY_ELF not found or not executable." >&2; exit 1; }

# 1. Cross-compile native libkurmanci_jni.so for each target ABI
for i in "${!REQUIRED_ABIS[@]}"; do
  abi="${REQUIRED_ABIS[$i]}"
  triple="${TARGET_TRIPLES[$i]}"
  echo "-> Cross-compiling libkurmanci_jni.so for ABI ${abi} (${triple}, platform ${MIN_SDK})..."

  cargo ndk --target "${triple}" --platform "${MIN_SDK}" build --release -p kurmanci-jni

  JNI_STAGE_DIR="$REPO_ROOT/android/kurmanci/src/main/jniLibs/${abi}"
  mkdir -p "$JNI_STAGE_DIR"
  cp "$REPO_ROOT/target/${triple}/release/libkurmanci_jni.so" "$JNI_STAGE_DIR/libkurmanci_jni.so"
  "$VERIFY_ELF" "$JNI_STAGE_DIR/libkurmanci_jni.so" "staged ${abi}"
  echo "✅ Staged $JNI_STAGE_DIR/libkurmanci_jni.so"
done

# 2. Build Release AAR and publish explicitly to distMaven repository
echo "Building Release AAR and publishing to local Maven repository (distMaven)..."
cd "$REPO_ROOT/android"
chmod +x ./gradlew
./gradlew :kurmanci:assembleRelease :kurmanci:publishReleasePublicationToDistMavenRepository \
  -PcentralRelease=false \
  -PkurmanciMavenGroup="${GROUP_ID}" \
  -PkurmanciVersion="${VERSION}"

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
MAVEN_BASE="$REPO_ROOT/dist/android/maven/$GROUP_PATH/kurmanci-android/${VERSION}"
MAVEN_POM="$MAVEN_BASE/kurmanci-android-${VERSION}.pom"
MAVEN_AAR="$MAVEN_BASE/kurmanci-android-${VERSION}.aar"
MAVEN_SRC="$MAVEN_BASE/kurmanci-android-${VERSION}-sources.jar"
MAVEN_DOC="$MAVEN_BASE/kurmanci-android-${VERSION}-javadoc.jar"

if [[ -f "$MAVEN_POM" && -f "$MAVEN_AAR" && -f "$MAVEN_SRC" && -f "$MAVEN_DOC" ]]; then
  echo "✅ Deterministic local Maven publication verified at $MAVEN_BASE"
else
  echo "❌ Error: Local Maven repository publication incomplete at $MAVEN_BASE" >&2
  exit 1
fi

# 4. The release invariant applies to what is shipped: check the exact bytes packaged in the
#    published AAR for every ABI, not only the staged inputs Gradle consumed.
echo "Verifying 16 KB page-size invariants on the libraries inside $MAVEN_AAR..."
AAR_CHECK_DIR="$(mktemp -d)"
trap 'rm -rf "$AAR_CHECK_DIR"' EXIT
for abi in "${REQUIRED_ABIS[@]}"; do
  unzip -q -o "$MAVEN_AAR" "jni/${abi}/libkurmanci_jni.so" -d "$AAR_CHECK_DIR" || { echo "❌ Error: jni/${abi}/libkurmanci_jni.so missing from $MAVEN_AAR" >&2; exit 1; }
  "$VERIFY_ELF" "$AAR_CHECK_DIR/jni/${abi}/libkurmanci_jni.so" "packaged AAR ${abi}"
done

echo "=== Android SDK v${VERSION} build and packaging completed successfully ==="
