#!/usr/bin/env bash
# Sourced helper: renders the Apple SDK release manifest (release-manifest.json) for a built
# XCFramework archive. The C ABI fields come from the checkout's public header through
# scripts/apple/c-abi-version.sh, never from a literal in this file.
#
#   write_release_manifest REPO_ROOT VERSION COMMIT SHA256 SWIFTPM_CHECKSUM RUST_VERSION XCODE_VER SWIFT_VER OUT_PATH
write_release_manifest() {
  local repo_root="$1" version="$2" commit="$3" sha256="$4" swiftpm_checksum="$5"
  local rust_version="$6" xcode_ver="$7" swift_ver="$8" out="$9"
  local helper
  helper="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/c-abi-version.sh"
  # shellcheck source=scripts/apple/c-abi-version.sh
  source "$helper"
  local abi_major abi_minor
  read -r abi_major abi_minor < <(read_c_abi_version "$repo_root") || return 1
  cat <<EOF > "$out"
{
  "schema_version": "apple-sdk-release-v1",
  "sdk_version": "${version}",
  "source_repository": "Kurdi-Language/kurmanci",
  "source_tag": "swift-v${version}",
  "source_commit": "${commit}",
  "distribution_repository": "Kurdi-Language/kurmanci-swift",
  "distribution_tag": "${version}",
  "c_abi_major": ${abi_major},
  "c_abi_minor": ${abi_minor},
  "supported_pack_format_versions": [
    4
  ],
  "artifact_sha256": "${sha256}",
  "swiftpm_checksum": "${swiftpm_checksum}",
  "toolchain": {
    "rust": "${rust_version}",
    "xcode": "${xcode_ver}",
    "swift": "${swift_ver}",
    "deployment_targets": {
      "macos": "11.0",
      "ios": "14.0"
    }
  }
}
EOF
}
