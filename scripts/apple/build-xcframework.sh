#!/usr/bin/env bash
set -euo pipefail

# Scripts dir resolution
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

VERSION="0.1.0"
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

echo "=== Building Kurmancî Apple XCFramework v${VERSION} (${COMMIT}) ==="

cd "$REPO_ROOT"

# Ensure PATH includes standard Rust and Homebrew paths cleanly
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

# 1. Require Apple CLI tools
REQUIRED_TOOLS=("xcodebuild" "lipo" "nm" "plutil" "otool" "swift" "cargo")
for tool in "${REQUIRED_TOOLS[@]}"; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "❌ Error: Required tool '$tool' is not installed or not in PATH." >&2
    exit 1
  fi
done

# 2. Require Apple SDKs
REQUIRED_SDKS=("macosx" "iphoneos" "iphonesimulator")
for sdk in "${REQUIRED_SDKS[@]}"; do
  if ! xcrun --sdk "$sdk" --show-sdk-path >/dev/null 2>&1; then
    echo "❌ Error: Required Apple SDK '$sdk' is not available." >&2
    exit 1
  fi
done
echo "✅ All required Apple SDKs (macosx, iphoneos, iphonesimulator) detected."

# 3. Require Rust Target Triples
REQUIRED_TARGETS=(
  "aarch64-apple-darwin"
  "x86_64-apple-darwin"
  "aarch64-apple-ios"
  "aarch64-apple-ios-sim"
  "x86_64-apple-ios"
)

if command -v rustup >/dev/null 2>&1; then
  for target in "${REQUIRED_TARGETS[@]}"; do
    if ! rustup target list --installed | grep -q "^${target}$"; then
      echo "Adding required Rust target ${target}..."
      rustup target add "${target}"
    fi
  done
else
  for target in "${REQUIRED_TARGETS[@]}"; do
    if ! cargo check -p kurmanci-ffi --target "${target}" >/dev/null 2>&1; then
      echo "❌ Error: Required Rust target '${target}' is not supported by cargo." >&2
      exit 1
    fi
  done
fi

# Set deployment targets
export MACOSX_DEPLOYMENT_TARGET=11.0
export IPHONEOS_DEPLOYMENT_TARGET=14.0

echo "Building release static libraries for Apple targets..."
for target in "${REQUIRED_TARGETS[@]}"; do
  echo "-> Building for ${target}..."
  cargo build --release -p kurmanci-ffi --target "${target}"
done

APPLE_STAGE="$REPO_ROOT/target/apple"
rm -rf "$APPLE_STAGE"
mkdir -p "$APPLE_STAGE/headers"

echo "Creating universal static archives with lipo..."
# macOS Universal (arm64 + x86_64)
lipo -create \
  "$REPO_ROOT/target/aarch64-apple-darwin/release/libkurmanci_ffi.a" \
  "$REPO_ROOT/target/x86_64-apple-darwin/release/libkurmanci_ffi.a" \
  -output "$APPLE_STAGE/libkurmanci_ffi_macos.a"

# iOS Simulator Universal (arm64 + x86_64)
lipo -create \
  "$REPO_ROOT/target/aarch64-apple-ios-sim/release/libkurmanci_ffi.a" \
  "$REPO_ROOT/target/x86_64-apple-ios/release/libkurmanci_ffi.a" \
  -output "$APPLE_STAGE/libkurmanci_ffi_iossim.a"

# iOS Device (arm64)
cp "$REPO_ROOT/target/aarch64-apple-ios/release/libkurmanci_ffi.a" \
   "$APPLE_STAGE/libkurmanci_ffi_ios.a"

# Prepare Headers
cp "$REPO_ROOT/ffi/include/kurmanci.h" "$APPLE_STAGE/headers/kurmanci.h"
cat <<'EOF' > "$APPLE_STAGE/headers/module.modulemap"
module KurmanciFFI {
    header "kurmanci.h"
    export *
}
EOF

DIST_ARTIFACTS="$REPO_ROOT/dist/artifacts"
rm -rf "$DIST_ARTIFACTS/KurmanciFFI.xcframework"
mkdir -p "$DIST_ARTIFACTS"

echo "Creating XCFramework using xcodebuild..."
xcodebuild -create-xcframework \
  -library "$APPLE_STAGE/libkurmanci_ffi_macos.a" -headers "$APPLE_STAGE/headers" \
  -library "$APPLE_STAGE/libkurmanci_ffi_iossim.a" -headers "$APPLE_STAGE/headers" \
  -library "$APPLE_STAGE/libkurmanci_ffi_ios.a" -headers "$APPLE_STAGE/headers" \
  -output "$DIST_ARTIFACTS/KurmanciFFI.xcframework"

echo "✅ KurmanciFFI.xcframework built successfully at dist/artifacts/KurmanciFFI.xcframework"
