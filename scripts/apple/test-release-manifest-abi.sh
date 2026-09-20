#!/usr/bin/env bash
# Shell-level check that the Apple release manifest's C ABI fields are derived from the
# authoritative public header, not from a literal: the rendered manifest must carry exactly
# the header's KMR_ABI_VERSION_MAJOR / KMR_ABI_VERSION_MINOR, a checkout whose header says
# something else must yield that other value, a header without the defines must fail, and
# no Apple script may carry a c_abi literal of its own. Needs only bash and python3.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=scripts/apple/c-abi-version.sh
source "$SCRIPT_DIR/c-abi-version.sh"
# shellcheck source=scripts/apple/release-manifest.sh
source "$SCRIPT_DIR/release-manifest.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# 1. The repository header is the source of truth; the manifest must agree with it.
read -r MAJOR MINOR < <(read_c_abi_version "$REPO_ROOT")
[[ "$MAJOR" =~ ^[0-9]+$ && "$MINOR" =~ ^[0-9]+$ ]] || { echo "❌ header ABI not numeric: $MAJOR.$MINOR" >&2; exit 1; }
write_release_manifest "$REPO_ROOT" "9.9.9" "0000000000000000000000000000000000000000" "$(printf '0%.0s' {1..64})" "$(printf '0%.0s' {1..64})" "1.85.0" "Xcode" "Swift" "$TMP/manifest.json"
python3 - "$TMP/manifest.json" "$MAJOR" "$MINOR" <<'EOF'
import json, sys
m = json.load(open(sys.argv[1]))
assert m["schema_version"] == "apple-sdk-release-v1", m
assert m["c_abi_major"] == int(sys.argv[2]) and m["c_abi_minor"] == int(sys.argv[3]), (m["c_abi_major"], m["c_abi_minor"], sys.argv[2:])
assert m["sdk_version"] == "9.9.9" and m["source_tag"] == "swift-v9.9.9" and m["distribution_tag"] == "9.9.9"
EOF
echo "✅ rendered manifest carries the header's C ABI $MAJOR.$MINOR"

# 2. A checkout whose header declares another ABI yields that ABI (no literal anywhere).
mkdir -p "$TMP/other/ffi/include"
cp "$REPO_ROOT/rust-toolchain.toml" "$TMP/other/" 2>/dev/null || true
printf '#define KMR_ABI_VERSION_MAJOR 7U\n#define KMR_ABI_VERSION_MINOR 3U\n' > "$TMP/other/ffi/include/kurmanci.h"
write_release_manifest "$TMP/other" "9.9.9" "0000000000000000000000000000000000000000" "$(printf '0%.0s' {1..64})" "$(printf '0%.0s' {1..64})" "1.85.0" "Xcode" "Swift" "$TMP/other.json"
python3 -c 'import json,sys; m=json.load(open(sys.argv[1])); assert (m["c_abi_major"], m["c_abi_minor"]) == (7, 3), m' "$TMP/other.json"
echo "✅ a header declaring ABI 7.3 renders 7.3"

# 3. A header without the defines fails closed.
mkdir -p "$TMP/broken/ffi/include"
printf '/* no defines */\n' > "$TMP/broken/ffi/include/kurmanci.h"
if write_release_manifest "$TMP/broken" "9.9.9" "0" "0" "0" "1" "x" "s" "$TMP/broken.json" 2>/dev/null; then
  echo "❌ manifest rendered without ABI defines" >&2; exit 1
fi
echo "✅ a header without the ABI defines is refused"

# 4. No Apple script carries a c_abi literal.
if grep -nE '"c_abi_(major|minor)": *[0-9]' "$SCRIPT_DIR"/*.sh | grep -v 'release-manifest.sh'; then
  echo "❌ an Apple script hard-codes a C ABI value" >&2; exit 1
fi
grep -q '"c_abi_major": \${abi_major}' "$SCRIPT_DIR/release-manifest.sh" || { echo "❌ release-manifest.sh does not substitute the header ABI" >&2; exit 1; }
echo "✅ no Apple script hard-codes the C ABI"
