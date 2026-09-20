#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

VERSION="0.1.1"
COMMIT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="$2"
      shift 2
      ;;
    --commit)
      COMMIT="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "❌ Error: Invalid Apple SDK version: '$VERSION' (must match X.Y.Z semver format)" >&2
  exit 1
fi

if [[ -z "$COMMIT" ]]; then
  COMMIT="$(cd "$REPO_ROOT" && git rev-parse HEAD)"
fi

DIST_DIR="$REPO_ROOT/dist"
XCFRAMEWORK="$DIST_DIR/artifacts/KurmanciFFI.xcframework"
ZIP_NAME="KurmanciFFI-v${VERSION}.xcframework.zip"
ZIP_PATH="$DIST_DIR/$ZIP_NAME"

if [[ ! -d "$XCFRAMEWORK" ]]; then
  echo "❌ Error: XCFramework not found at $XCFRAMEWORK" >&2
  exit 1
fi

echo "=== Creating Release Archive for Kurmancî Apple SDK v${VERSION} ==="

rm -f "$ZIP_PATH"

# Compress XCFramework with normalized timestamps for byte-level determinism
find "$DIST_DIR/artifacts/KurmanciFFI.xcframework" -exec touch -t 202601010000.00 {} +
(cd "$DIST_DIR/artifacts" && TZ=UTC zip -r -q -X "$ZIP_PATH" "KurmanciFFI.xcframework")

# Compute checksums
SHA256_HASH=$(shasum -a 256 "$ZIP_PATH" | awk '{print $1}')
SWIFTPM_CHECKSUM=$(swift package compute-checksum "$ZIP_PATH")

if [[ ! "$SHA256_HASH" =~ ^[a-f0-9]{64}$ ]]; then
  echo "❌ Error: Invalid SHA-256 hash: '$SHA256_HASH'" >&2
  exit 1
fi

if [[ ! "$SWIFTPM_CHECKSUM" =~ ^[a-f0-9]{64}$ ]]; then
  echo "❌ Error: Invalid SwiftPM checksum: '$SWIFTPM_CHECKSUM'" >&2
  exit 1
fi

# Extract toolchain versions explicitly
RUST_VERSION=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$REPO_ROOT/rust-toolchain.toml" | tr -d '\r')
if [[ -z "$RUST_VERSION" ]]; then
  echo "❌ Error: Failed to read Rust toolchain version from rust-toolchain.toml" >&2
  exit 1
fi

XCODE_VER=$(xcodebuild -version 2>/dev/null | head -n1 || echo "Xcode")
SWIFT_VER=$(swift --version 2>/dev/null | head -n1 || echo "Swift")

# Generate release-manifest.json; the C ABI fields come from ffi/include/kurmanci.h through
# scripts/apple/release-manifest.sh (scripts/apple/test-release-manifest-abi.sh checks it).
# shellcheck source=scripts/apple/release-manifest.sh
source "$SCRIPT_DIR/release-manifest.sh"
write_release_manifest "$REPO_ROOT" "$VERSION" "$COMMIT" "$SHA256_HASH" "$SWIFTPM_CHECKSUM" "$RUST_VERSION" "$XCODE_VER" "$SWIFT_VER" "$DIST_DIR/release-manifest.json"

echo "✅ Release archive created: dist/${ZIP_NAME}"
echo "   SHA-256: ${SHA256_HASH}"
echo "   SwiftPM Checksum: ${SWIFTPM_CHECKSUM}"
