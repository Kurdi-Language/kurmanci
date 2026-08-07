#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

export MACOSX_DEPLOYMENT_TARGET=11.0
export IPHONEOS_DEPLOYMENT_TARGET=13.0

XCFRAMEWORK="$REPO_ROOT/dist/artifacts/KurmanciFFI.xcframework"
DIST_SWIFT_LOCAL="$REPO_ROOT/dist/swift-package-local"
SEED_PACK="$REPO_ROOT/integration/apple/fixtures/apple_consumer_test.bin"

if [[ ! -d "$XCFRAMEWORK" ]]; then
  echo "❌ Error: XCFramework not found at $XCFRAMEWORK. Run build-xcframework.sh first." >&2
  exit 1
fi

if [[ ! -d "$DIST_SWIFT_LOCAL" ]]; then
  echo "❌ Error: Local Swift package not found at $DIST_SWIFT_LOCAL. Run generate-release-package.sh first." >&2
  exit 1
fi

echo "=== Running Apple Consumer Integration Tests ==="

SWIFTC_CMD="swiftc"
SWIFTC_FLAGS=()
if [[ -f "/tmp/swift_toolchain/usr/bin/swiftc" && -d "/tmp/swift_toolchain/SDKs/MacOSX.sdk" ]]; then
  SWIFTC_CMD="/tmp/swift_toolchain/usr/bin/swiftc"
  SWIFTC_FLAGS=(-sdk "/tmp/swift_toolchain/SDKs/MacOSX.sdk")
fi

MAC_HDR_DIR="$(find "$XCFRAMEWORK" -path "*macos*/Headers" -type d | head -n1)"
MAC_LIB_FILE="$(find "$XCFRAMEWORK" -name "libkurmanci_ffi_macos.a" | head -n1)"

if [[ -z "$MAC_HDR_DIR" || ! -d "$MAC_HDR_DIR" ]]; then
  echo "❌ Error: Could not locate macos Headers directory in XCFramework" >&2
  exit 1
fi

# 1. Plain C Header Compilation across Apple SDKs
echo "-> 1. Testing Plain C Header compilation..."
MACOS_SYSROOT="$(xcrun --sdk macosx --show-sdk-path)"
clang -Wall -Wextra -Werror -target arm64-apple-macos11.0 -isysroot "$MACOS_SYSROOT" -I "$MAC_HDR_DIR" -c "$REPO_ROOT/integration/apple/plain-c-consumer/main.c" -o /dev/null

if xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1; then
  IPHONE_SYSROOT="$(xcrun --sdk iphoneos --show-sdk-path)"
  SIM_SYSROOT="$(xcrun --sdk iphonesimulator --show-sdk-path)"
  IOS_HDR_DIR="$(find "$XCFRAMEWORK" -path "*ios-arm64/Headers" -type d | head -n1)"
  SIM_HDR_DIR="$(find "$XCFRAMEWORK" -path "*simulator*/Headers" -type d | head -n1)"

  if [[ -z "$IOS_HDR_DIR" || ! -d "$IOS_HDR_DIR" ]]; then
    echo "❌ Error: Could not locate ios-arm64 Headers directory in XCFramework" >&2
    exit 1
  fi
  if [[ -z "$SIM_HDR_DIR" || ! -d "$SIM_HDR_DIR" ]]; then
    echo "❌ Error: Could not locate simulator Headers directory in XCFramework" >&2
    exit 1
  fi

  clang -Wall -Wextra -Werror -target arm64-apple-ios13.0 -isysroot "$IPHONE_SYSROOT" -I "$IOS_HDR_DIR" -c "$REPO_ROOT/integration/apple/plain-c-consumer/main.c" -o /dev/null
  clang -Wall -Wextra -Werror -target arm64-apple-ios13.0-simulator -isysroot "$SIM_SYSROOT" -I "$SIM_HDR_DIR" -c "$REPO_ROOT/integration/apple/plain-c-consumer/main.c" -o /dev/null
  echo "  ✅ Plain C header compiles cleanly across macOS, iOS Device, and iOS Simulator SDKs."
else
  echo "  ✅ Plain C header compiles cleanly across macOS SDK."
fi

# 2. Direct Swift C-module import ('import KurmanciFFI')
echo "-> 2. Testing direct Swift C-module import ('import KurmanciFFI')..."
mkdir -p "$REPO_ROOT/target/apple"
if [[ ${#SWIFTC_FLAGS[@]} -gt 0 ]]; then
  "$SWIFTC_CMD" "${SWIFTC_FLAGS[@]}" \
    -I "$MAC_HDR_DIR" \
    "$REPO_ROOT/integration/apple/c-module-consumer/main.swift" \
    -Xlinker -force_load -Xlinker "$MAC_LIB_FILE" \
    -o "$REPO_ROOT/target/apple/c_module_consumer_test"
else
  "$SWIFTC_CMD" \
    -I "$MAC_HDR_DIR" \
    "$REPO_ROOT/integration/apple/c-module-consumer/main.swift" \
    -Xlinker -force_load -Xlinker "$MAC_LIB_FILE" \
    -o "$REPO_ROOT/target/apple/c_module_consumer_test"
fi

"$REPO_ROOT/target/apple/c_module_consumer_test"
echo "  ✅ Direct 'import KurmanciFFI' module test passed."

# 3. External macOS Swift Consumer via Pure SwiftPM Resolution
echo "-> 3. Testing macOS Swift Consumer via pure SwiftPM package resolution..."
if command -v swift >/dev/null 2>&1; then
  swift run --package-path "$REPO_ROOT/integration/apple/macos-consumer" MacOSConsumer "$SEED_PACK"
  echo "  ✅ macOS Swift Consumer package resolution test passed."
fi

# 4. Dependency & Deployment Target Inspection (otool)
echo "-> 4. Inspecting macOS consumer binary dependencies (otool -L)..."
MACOS_BIN=$(find "$REPO_ROOT/integration/apple/macos-consumer/.build" -name "MacOSConsumer" -type f 2>/dev/null | head -n1 || echo "$REPO_ROOT/target/apple/c_module_consumer_test")
DEPENDENCIES=$(otool -L "$MACOS_BIN")
echo "$DEPENDENCIES"
if echo "$DEPENDENCIES" | grep -q "target/debug"; then
  echo "❌ Error: Binary contains unexpected cargo target/debug path" >&2
  exit 1
fi
echo "  ✅ No unexpected local Cargo build paths found in linked binary."

echo "-> 5. Inspecting deployment target (otool -l)..."
MINOS=$(otool -l "$MACOS_BIN" 2>/dev/null | awk '
  /cmd LC_BUILD_VERSION/ { in_build = 1; next }
  in_build && /minos/ { print $2; in_build = 0; exit }
  /cmd LC_VERSION_MIN_/ { in_ver = 1; next }
  in_ver && /version/ { print $2; in_ver = 0; exit }
')
echo "  Parsed LC_BUILD_VERSION minos: '$MINOS'"
ACTUAL_MAJOR=$(echo "$MINOS" | cut -d. -f1)
ACTUAL_MINOR=$(echo "$MINOS" | cut -d. -f2)
if [[ "$ACTUAL_MAJOR" == "11" && "$ACTUAL_MINOR" == "0" ]]; then
  echo "  ✅ Deployment target verified (macOS 11.0 exact)."
else
  echo "❌ Error: macOS Consumer binary minos is '$MINOS', expected exact 11.0." >&2
  exit 1
fi

echo "✅ All Apple macOS consumer integration tests completed successfully!"
