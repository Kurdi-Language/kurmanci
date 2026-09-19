#!/usr/bin/env bash
# Sourced by the Android scripts: resolves the Kurmancî Android SDK version exactly once.
#
#   VERSION="$(resolve_kurmanci_version "$REPO_ROOT")"
#
# 1. An explicit VERSION environment variable wins (the release workflow passes the tag's
#    version this way; CI and the local scripts may too).
# 2. Otherwise `kurmanciVersion` in android/gradle.properties is the single authoritative
#    default: a release changes that one property, and no script carries a version of its own.
# 3. A missing or empty property is an error, never a silent fallback to an older version.
# 4. The result must be X.Y.Z (the same check the release workflow applies to its tag).
resolve_kurmanci_version() {
  local repo_root="$1"
  local props="$repo_root/android/gradle.properties"
  local version="${VERSION:-}"
  local origin="the VERSION environment variable"
  if [[ -z "$version" ]]; then
    origin="kurmanciVersion in $props"
    if [[ ! -f "$props" ]]; then
      echo "❌ $props not found; cannot resolve the Kurmancî Android SDK version (set VERSION=X.Y.Z or restore the property)" >&2
      return 1
    fi
    version="$(grep '^kurmanciVersion=' "$props" | head -n1 | cut -d'=' -f2- | tr -d ' \r\n' || true)"
    if [[ -z "$version" ]]; then
      echo "❌ kurmanciVersion is missing or empty in $props; refusing to guess a version (set VERSION=X.Y.Z or set the property)" >&2
      return 1
    fi
  fi
  if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "❌ Kurmancî Android SDK version '$version' (from $origin) is not X.Y.Z" >&2
    return 1
  fi
  printf '%s\n' "$version"
}
