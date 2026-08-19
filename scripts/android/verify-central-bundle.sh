#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEFAULT_GROUP="$(grep '^kurmanciMavenGroup=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo 'io.github.ferhatguneri')"
DEFAULT_VERSION="$(grep '^kurmanciVersion=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo '0.1.0')"

VERSION="${VERSION:-$DEFAULT_VERSION}"
GROUP_ID="${GROUP_ID:-$DEFAULT_GROUP}"
CHECK_SIGNATURES="${CHECK_SIGNATURES:-false}"
GROUP_PATH="${GROUP_ID//./\/}"

STAGING_DIR="${1:-$REPO_ROOT/dist/android/central-staging}"
ARTIFACT_DIR="$STAGING_DIR/$GROUP_PATH/kurmanci-android/$VERSION"

echo "=== Verifying Maven Central Bundle at $ARTIFACT_DIR ==="

if [[ ! -d "$ARTIFACT_DIR" ]]; then
  echo "❌ Error: Artifact directory missing: $ARTIFACT_DIR" >&2
  exit 1
fi

cd "$ARTIFACT_DIR"

REQUIRED_FILES=(
  "kurmanci-android-${VERSION}.aar"
  "kurmanci-android-${VERSION}.pom"
  "kurmanci-android-${VERSION}-sources.jar"
  "kurmanci-android-${VERSION}-javadoc.jar"
)

# Helper function to compute MD5 cross-platform
calc_md5() {
  local target="$1"
  if command -v md5sum >/dev/null 2>&1; then
    md5sum "$target" | awk '{print $1}'
  else
    md5 -q "$target"
  fi
}

for file in "${REQUIRED_FILES[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "❌ Error: Required artifact missing: $file" >&2
    exit 1
  fi
  echo "  ✅ Found $file"

  # Check checksum files existence
  if [[ ! -f "${file}.sha1" || ! -f "${file}.md5" ]]; then
    echo "❌ Error: Missing checksum for $file (${file}.sha1 or ${file}.md5)" >&2
    exit 1
  fi

  # Verify SHA-1
  CALC_SHA1="$(shasum -a 1 "$file" | awk '{print $1}')"
  EXPECTED_SHA1="$(cat "${file}.sha1" | tr -d ' \r\n')"
  if [[ "$CALC_SHA1" != "$EXPECTED_SHA1" ]]; then
    echo "❌ Error: SHA-1 mismatch for $file (got $CALC_SHA1, expected $EXPECTED_SHA1)" >&2
    exit 1
  fi

  # Verify MD5
  CALC_MD5="$(calc_md5 "$file")"
  EXPECTED_MD5="$(cat "${file}.md5" | tr -d ' \r\n')"
  if [[ "$CALC_MD5" != "$EXPECTED_MD5" ]]; then
    echo "❌ Error: MD5 mismatch for $file (got $CALC_MD5, expected $EXPECTED_MD5)" >&2
    exit 1
  fi

  # Verify Signatures if CHECK_SIGNATURES=true
  if [[ "$CHECK_SIGNATURES" == "true" ]]; then
    if [[ ! -f "${file}.asc" ]]; then
      echo "❌ Error: Missing PGP signature for release artifact: ${file}.asc" >&2
      exit 1
    fi
  fi
done

# Optional .module metadata validation
mod_file="kurmanci-android-${VERSION}.module"
if [[ -f "$mod_file" ]]; then
  echo "  ✅ Found optional $mod_file"
  if [[ ! -f "${mod_file}.sha1" || ! -f "${mod_file}.md5" ]]; then
    echo "❌ Error: Missing checksum for $mod_file" >&2
    exit 1
  fi

  CALC_MOD_SHA1="$(shasum -a 1 "$mod_file" | awk '{print $1}')"
  EXPECTED_MOD_SHA1="$(cat "${mod_file}.sha1" | tr -d ' \r\n')"
  if [[ "$CALC_MOD_SHA1" != "$EXPECTED_MOD_SHA1" ]]; then
    echo "❌ Error: SHA-1 mismatch for $mod_file" >&2
    exit 1
  fi

  CALC_MOD_MD5="$(calc_md5 "$mod_file")"
  EXPECTED_MOD_MD5="$(cat "${mod_file}.md5" | tr -d ' \r\n')"
  if [[ "$CALC_MOD_MD5" != "$EXPECTED_MOD_MD5" ]]; then
    echo "❌ Error: MD5 mismatch for $mod_file" >&2
    exit 1
  fi

  if [[ "$CHECK_SIGNATURES" == "true" && ! -f "${mod_file}.asc" ]]; then
    echo "❌ Error: Missing signature for $mod_file" >&2
    exit 1
  fi
fi

# Rule: .asc files must not have .md5 or .sha1 files
for asc in *.asc; do
  if [[ -f "$asc" ]]; then
    if [[ -f "${asc}.md5" || -f "${asc}.sha1" ]]; then
      echo "❌ Error: Signature file $asc must NOT have .md5 or .sha1 checksum files" >&2
      exit 1
    fi
  fi
done

# Rule: checksum files must not have .asc files
for chk in *.md5 *.sha1; do
  if [[ -f "$chk" ]]; then
    if [[ -f "${chk}.asc" ]]; then
      echo "❌ Error: Checksum file $chk must NOT have .asc signature file" >&2
      exit 1
    fi
  fi
done

# Verify AAR layout and native ABIs
AAR_FILE="kurmanci-android-${VERSION}.aar"
AAR_CONTENTS=$(unzip -l "$AAR_FILE")
REQUIRED_ABIS=("arm64-v8a" "armeabi-v7a" "x86_64")

for abi in "${REQUIRED_ABIS[@]}"; do
  if ! echo "$AAR_CONTENTS" | grep -q "jni/${abi}/libkurmanci_jni.so"; then
    echo "❌ Error: Missing native library jni/${abi}/libkurmanci_jni.so in $AAR_FILE" >&2
    exit 1
  fi
  echo "  ✅ AAR contains native library for ABI $abi"
done

# Verify POM metadata
POM_FILE="kurmanci-android-${VERSION}.pom"
POM_CONTENT=$(cat "$POM_FILE")

for tag in "name" "description" "url" "licenses" "developers" "scm"; do
  if ! echo "$POM_CONTENT" | grep -q "<${tag}>"; then
    echo "❌ Error: POM file missing required tag <${tag}>" >&2
    exit 1
  fi
done

echo "✅ POM metadata tags verified (<name>, <description>, <url>, <licenses>, <developers>, <scm>)"
echo "=== Maven Central Bundle Verification PASSED cleanly! ==="
