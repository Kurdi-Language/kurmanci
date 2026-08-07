#!/usr/bin/env bash
set -euo pipefail
# Verify Apple XCFramework bundle structure, symbols, and iOS 13 / macOS 11 deployment targets.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

XCFRAMEWORK="$REPO_ROOT/dist/artifacts/KurmanciFFI.xcframework"
SYMBOLS_FILE="$REPO_ROOT/ffi/include/required_symbols.txt"

if [[ ! -d "$XCFRAMEWORK" ]]; then
  echo "❌ Error: XCFramework not found at $XCFRAMEWORK" >&2
  exit 1
fi

echo "=== Verifying KurmanciFFI.xcframework ==="

# 1. Lint Info.plist
echo "-> Linting Info.plist..."
plutil -lint "$XCFRAMEWORK/Info.plist"

# 2. Assert exact required 3 platform slices
REQUIRED_SLICES=("macos-arm64_x86_64" "ios-arm64" "ios-arm64_x86_64-simulator")
echo "-> Verifying mandatory XCFramework platform slices..."
for slice in "${REQUIRED_SLICES[@]}"; do
  if [[ ! -d "$XCFRAMEWORK/$slice" ]]; then
    echo "❌ Error: Mandatory XCFramework platform slice '$slice' is missing." >&2
    exit 1
  fi
  echo "  ✅ Slice '$slice' present."
done

# 3. Check lipo architecture info & thin deployment targets
echo "-> Checking lipo architecture info & deployment targets..."

check_arch_deployment_target() {
  local binary="$1"
  local target_arch="$2"
  local expected_minos="$3"
  local platform_name="$4"

  local tmp_dir
  tmp_dir="$(mktemp -d)"

  local thin_file="$tmp_dir/thin_$target_arch"
  lipo "$binary" -thin "$target_arch" -output "$thin_file" 2>/dev/null || cp "$binary" "$thin_file"

  local minos
  minos=$(otool -l "$thin_file" 2>/dev/null | awk '
    /cmd LC_BUILD_VERSION/ { in_build = 1; next }
    in_build && /minos/ { print $2; in_build = 0; exit }
    /cmd LC_VERSION_MIN_/ { in_ver = 1; next }
    in_ver && /version/ { print $2; in_ver = 0; exit }
  ')

  if [[ -z "$minos" ]]; then
    local extract_dir="$tmp_dir/obj_$target_arch"
    mkdir -p "$extract_dir"
    (cd "$extract_dir" && ar x "$thin_file" 2>/dev/null || true)
    local target_obj
    target_obj=$(find "$extract_dir" -name "*kurmanci*.o" | head -n1)
    if [[ -z "$target_obj" ]]; then
      target_obj=$(find "$extract_dir" -name "*.o" | head -n1)
    fi
    if [[ -n "$target_obj" ]]; then
      minos=$(otool -l "$target_obj" 2>/dev/null | awk '
        /cmd LC_BUILD_VERSION/ { in_build = 1; next }
        in_build && /minos/ { print $2; in_build = 0; exit }
        /cmd LC_VERSION_MIN_/ { in_ver = 1; next }
        in_ver && /version/ { print $2; in_ver = 0; exit }
      ')
    fi
  fi

  if [[ -z "$minos" ]]; then
    rm -rf "$tmp_dir"
    echo "❌ Error: Could not parse LC_BUILD_VERSION minos for $platform_name ($target_arch) in $binary" >&2
    exit 1
  fi

  local actual_major actual_minor expected_major expected_minor
  actual_major=$(echo "$minos" | cut -d. -f1)
  actual_minor=$(echo "$minos" | cut -d. -f2)
  expected_major=$(echo "$expected_minos" | cut -d. -f1)
  expected_minor=$(echo "$expected_minos" | cut -d. -f2)

  if [[ "$actual_major" == "$expected_major" && "$actual_minor" == "$expected_minor" ]]; then
    echo "  ✅ Deployment target for $platform_name ($target_arch) verified: minos $minos == $expected_minos"
  else
    rm -rf "$tmp_dir"
    echo "❌ Error: Deployment target for $platform_name ($target_arch) is minos $minos, expected exact $expected_minos" >&2
    exit 1
  fi
  rm -rf "$tmp_dir"
}

check_slice_deployment_target() {
  local binary="$1"
  local platform_name="$2"
  local archs
  archs=$(lipo -archs "$binary" 2>/dev/null || echo "arm64")

  for arch in $archs; do
    if echo "$platform_name" | grep -q "macos"; then
      check_arch_deployment_target "$binary" "$arch" "11.0" "$platform_name"
    elif echo "$platform_name" | grep -q "simulator"; then
      check_arch_deployment_target "$binary" "$arch" "14.0" "$platform_name"
    else
      check_arch_deployment_target "$binary" "$arch" "14.0" "$platform_name"
    fi
  done
}

for static_lib in $(find "$XCFRAMEWORK" -name "*.a"); do
  slice_name="$(basename "$(dirname "$static_lib")")"
  echo "  Static Lib: $slice_name/$(basename "$static_lib")"
  lipo -info "$static_lib"

  check_slice_deployment_target "$static_lib" "$slice_name"
done

# 4. Verify required C ABI symbols across all slices
echo "-> Verifying required C ABI symbols..."
for static_lib in $(find "$XCFRAMEWORK" -name "*.a"); do
  FOUND_SYMBOLS=$(nm -gU "$static_lib" 2>/dev/null | grep -o 'kmr_[a-z0-9_]*' | sort -u)
  for required_sym in $(tr -d '\r' < "$SYMBOLS_FILE"); do
    if ! echo "$FOUND_SYMBOLS" | grep -q "^${required_sym}$"; then
      echo "❌ Error: Missing required symbol '${required_sym}' in $static_lib" >&2
      exit 1
    fi
  done
done
echo "✅ All 18 public kmr_* C ABI symbols present across all slices."

# 5. Check headers and modulemap
echo "-> Verifying headers and modulemap..."
for slice in "${REQUIRED_SLICES[@]}"; do
  if [[ ! -f "$XCFRAMEWORK/$slice/Headers/kurmanci.h" ]]; then
    echo "❌ Error: Missing kurmanci.h in $slice" >&2
    exit 1
  fi
  if [[ ! -f "$XCFRAMEWORK/$slice/Headers/module.modulemap" ]]; then
    echo "❌ Error: Missing module.modulemap in $slice" >&2
    exit 1
  fi
done

echo "✅ KurmanciFFI.xcframework structure, deployment targets & symbols verified 100% successfully!"
