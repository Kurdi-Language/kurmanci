#!/usr/bin/env bash
# Sourced helper: reads the authoritative C ABI version of a checkout from the public header
# ffi/include/kurmanci.h (KMR_ABI_VERSION_MAJOR / KMR_ABI_VERSION_MINOR), the same source
# the release bundle's compatibility manifest is derived from (data-builder release.rs,
# C_HEADER_PATH). No script may carry an ABI literal of its own.
#
#   read -r ABI_MAJOR ABI_MINOR < <(read_c_abi_version "$REPO_ROOT")
#   read_c_abi_version_from_header "$path_to_kurmanci.h"
read_c_abi_version_from_header() {
  local header="$1"
  [[ -f "$header" ]] || { echo "❌ C ABI header not found: $header" >&2; return 1; }
  local major minor
  major="$(sed -n 's/^#define KMR_ABI_VERSION_MAJOR \([0-9][0-9]*\)U\{0,1\}[[:space:]]*$/\1/p' "$header" | head -n1)"
  minor="$(sed -n 's/^#define KMR_ABI_VERSION_MINOR \([0-9][0-9]*\)U\{0,1\}[[:space:]]*$/\1/p' "$header" | head -n1)"
  if [[ ! "$major" =~ ^[0-9]+$ || ! "$minor" =~ ^[0-9]+$ ]]; then
    echo "❌ KMR_ABI_VERSION_MAJOR / KMR_ABI_VERSION_MINOR not found in $header" >&2
    return 1
  fi
  printf '%s %s\n' "$major" "$minor"
}

read_c_abi_version() {
  read_c_abi_version_from_header "$1/ffi/include/kurmanci.h"
}
