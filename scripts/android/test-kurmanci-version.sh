#!/usr/bin/env bash
# Shell-level check of scripts/android/kurmanci-version.sh: an explicit VERSION wins; otherwise
# kurmanciVersion in android/gradle.properties is the only default; a missing file, a missing
# or empty property and a non-X.Y.Z value fail with a clear message instead of a silent older
# version; and every Android script sources the resolver and carries no version literal of
# its own. Needs only bash.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=scripts/android/kurmanci-version.sh
source "$SCRIPT_DIR/kurmanci-version.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fake_root() {
  local root="$TMP/$1"; shift
  mkdir -p "$root/android"
  if [[ $# -gt 0 ]]; then printf '%s\n' "$@" > "$root/android/gradle.properties"; fi
  printf '%s' "$root"
}

expect_ok() {
  local label="$1" expected="$2"; shift 2
  local got
  got="$("$@")" || { echo "❌ $label: resolver failed" >&2; exit 1; }
  [[ "$got" == "$expected" ]] || { echo "❌ $label: got '$got', expected '$expected'" >&2; exit 1; }
  echo "✅ $label → $got"
}

expect_fail() {
  local label="$1" needle="$2"; shift 2
  local out status
  set +e
  out="$("$@" 2>&1)"; status=$?
  set -e
  [[ $status -ne 0 ]] || { echo "❌ $label: resolver succeeded with '$out'" >&2; exit 1; }
  [[ "$out" == *"$needle"* ]] || { echo "❌ $label: message does not say '$needle': $out" >&2; exit 1; }
  echo "✅ $label is refused: ${out:0:110}"
}

# 1. The repository's own property resolves and is X.Y.Z (and equals what the scripts use).
REPO_VERSION="$(grep '^kurmanciVersion=' "$REPO_ROOT/android/gradle.properties" | cut -d'=' -f2 | tr -d ' \r\n')"
[[ "$REPO_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "❌ repository kurmanciVersion '$REPO_VERSION' is not X.Y.Z" >&2; exit 1; }
expect_ok "repository property" "$REPO_VERSION" env -u VERSION bash -c 'source "$0/scripts/android/kurmanci-version.sh"; resolve_kurmanci_version "$0"' "$REPO_ROOT"

# 2. Explicit VERSION overrides the property.
ROOT="$(fake_root override 'kurmanciVersion=1.2.3')"
expect_ok "explicit VERSION" "9.8.7" env VERSION=9.8.7 bash -c 'source "$0"; resolve_kurmanci_version "$1"' "$SCRIPT_DIR/kurmanci-version.sh" "$ROOT"

# 3. Property read with surrounding whitespace and CRLF tolerated.
ROOT="$(fake_root crlf $'kurmanciMavenGroup=io.example\r' $'kurmanciVersion= 1.2.3 \r')"
expect_ok "property with whitespace/CRLF" "1.2.3" env -u VERSION bash -c 'source "$0"; resolve_kurmanci_version "$1"' "$SCRIPT_DIR/kurmanci-version.sh" "$ROOT"

# 4. Missing property, empty property, missing file: refused, naming the cause.
ROOT="$(fake_root noprop 'kurmanciMavenGroup=io.example')"
expect_fail "missing property" "missing or empty" env -u VERSION bash -c 'source "$0"; resolve_kurmanci_version "$1"' "$SCRIPT_DIR/kurmanci-version.sh" "$ROOT"
ROOT="$(fake_root empty 'kurmanciVersion=')"
expect_fail "empty property" "missing or empty" env -u VERSION bash -c 'source "$0"; resolve_kurmanci_version "$1"' "$SCRIPT_DIR/kurmanci-version.sh" "$ROOT"
ROOT="$(fake_root nofile)"
expect_fail "missing gradle.properties" "not found" env -u VERSION bash -c 'source "$0"; resolve_kurmanci_version "$1"' "$SCRIPT_DIR/kurmanci-version.sh" "$ROOT"

# 5. Non-semver values are refused, from either origin.
ROOT="$(fake_root badprop 'kurmanciVersion=0.1')"
expect_fail "non-semver property" "is not X.Y.Z" env -u VERSION bash -c 'source "$0"; resolve_kurmanci_version "$1"' "$SCRIPT_DIR/kurmanci-version.sh" "$ROOT"
expect_fail "non-semver VERSION" "is not X.Y.Z" env VERSION=v1.2.3 bash -c 'source "$0"; resolve_kurmanci_version "$1"' "$SCRIPT_DIR/kurmanci-version.sh" "$ROOT"

# 6. Every Android script sources the resolver and carries no version literal of its own.
for f in build-aar device-benchmark prepare-central-bundle test-consumers verify-central-bundle verify-clean-room-consumer; do
  s="$SCRIPT_DIR/$f.sh"
  grep -q 'source "$SCRIPT_DIR/kurmanci-version.sh"' "$s" || { echo "❌ $f.sh does not source kurmanci-version.sh" >&2; exit 1; }
  grep -q 'VERSION="$(resolve_kurmanci_version "$REPO_ROOT")"' "$s" || { echo "❌ $f.sh does not resolve VERSION through the resolver" >&2; exit 1; }
  if grep -qE "DEFAULT_VERSION|echo '[0-9]+\.[0-9]+\.[0-9]+'" "$s"; then echo "❌ $f.sh still carries a version fallback of its own" >&2; exit 1; fi
done
echo "✅ all six Android scripts resolve the version through kurmanci-version.sh with no literal fallback"
