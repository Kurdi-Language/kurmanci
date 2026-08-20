#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEFAULT_GROUP="$(grep '^kurmanciMavenGroup=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo 'io.github.ferhatguneri')"
DEFAULT_VERSION="$(grep '^kurmanciVersion=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo '0.1.0')"

VERSION="${VERSION:-$DEFAULT_VERSION}"
GROUP_ID="${GROUP_ID:-$DEFAULT_GROUP}"
GROUP_PATH="${GROUP_ID//./\/}"

echo "=== Preparing Maven Central Bundle v${VERSION} for ${GROUP_ID} ==="

cd "$REPO_ROOT"

STAGING_DIR="$REPO_ROOT/dist/android/central-staging"
rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR"

# 1. Publish to release-only Central Staging directory with centralRelease=true
echo "Publishing signed release artifacts to central-staging..."
cd "$REPO_ROOT/android"
chmod +x ./gradlew

./gradlew :kurmanci:publishReleasePublicationToCentralStagingMavenRepository \
  -PcentralRelease=true \
  -PkurmanciMavenGroup="${GROUP_ID}" \
  -PkurmanciVersion="${VERSION}"

ARTIFACT_DIR="$STAGING_DIR/$GROUP_PATH/kurmanci-android/$VERSION"

if [[ ! -d "$ARTIFACT_DIR" ]]; then
  echo "❌ Error: Central staging directory missing at $ARTIFACT_DIR" >&2
  exit 1
fi

echo "Generating MD5 and SHA-1 checksums for primary deployed artifacts..."

cd "$ARTIFACT_DIR"

for file in *; do
  # Skip existing checksum files and signature files
  if [[ "$file" == *.md5 || "$file" == *.sha1 || "$file" == *.asc ]]; then
    continue
  fi

  if [[ -f "$file" ]]; then
    echo "  Generating checksums for $file..."
    # Generate SHA-1
    shasum -a 1 "$file" | awk '{print $1}' > "${file}.sha1"

    # Generate MD5
    if command -v md5sum >/dev/null 2>&1; then
      md5sum "$file" | awk '{print $1}' > "${file}.md5"
    else
      md5 -q "$file" > "${file}.md5"
    fi
  fi
done

echo "Normalizing Central staging bundle (removing Gradle signature-checksum sidecars)..."
find "$ARTIFACT_DIR" -maxdepth 1 -type f \
  \( -name '*.asc.md5' \
     -o -name '*.asc.sha1' \
     -o -name '*.asc.sha256' \
     -o -name '*.asc.sha512' \) \
  -print -delete

# Package Central ZIP bundle
ZIP_OUTPUT="$REPO_ROOT/dist/central-bundle-${VERSION}.zip"
rm -f "$ZIP_OUTPUT"

cd "$STAGING_DIR"
zip -r "$ZIP_OUTPUT" .

echo "✅ Maven Central bundle packaged at: $ZIP_OUTPUT"
echo "=== Central Bundle Preparation Complete ==="
