#!/usr/bin/env bash
# Proves that two clean checkouts of the same commit produce a byte-identical release bundle:
#
#   clean checkout A → review pipeline → packs → build-release-bundle
#   clean checkout B → review pipeline → packs → build-release-bundle
#   SHA256SUMS(A) == SHA256SUMS(B)   (and therefore every file, since SHA256SUMS lists them all)
#
# Only committed state takes part: each checkout is a fresh clone of this repository at the
# requested ref (default HEAD). The Kuwiki corpus is not needed; the committed language model
# is used as-is. One shared CARGO_TARGET_DIR keeps the Rust build to a single compilation.
#
# Usage: scripts/release/verify-clean-checkout-determinism.sh [REF] [--keep]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
REF="HEAD"
KEEP=0
for arg in "$@"; do
  case "$arg" in
    --keep) KEEP=1 ;;
    *) REF="$arg" ;;
  esac
done

REF_SHA="$(git -C "$REPO_ROOT" rev-parse --verify "${REF}^{commit}")"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/kurmanci-release-determinism.XXXXXX")"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$WORK/target}"
cleanup() {
  if [[ "$KEEP" -eq 0 ]]; then rm -rf "$WORK"; else echo "kept: $WORK"; fi
}
trap cleanup EXIT

echo "=== Release bundle clean-checkout determinism ==="
echo "  commit: $REF_SHA"
echo "  work:   $WORK"

build_one() {
  local dir="$1"
  git clone --quiet --no-hardlinks "$REPO_ROOT" "$dir"
  git -C "$dir" checkout --quiet --detach "$REF_SHA"
  if [[ -n "$(git -C "$dir" status --porcelain --untracked-files=no)" ]]; then
    echo "❌ clone at $dir is not clean" >&2
    exit 1
  fi
  # The log lives next to the clone, never inside it: an untracked file inside the clone
  # would make the tree dirty and the bundle an evaluation release.
  local log="$dir.derivation.log"
  step() {
    # Progress output of the builders goes to stderr; keep it in a log and show it on failure.
    if ! cargo run --quiet --release -p kurmanci-data-builder -- "$@" >>"$log" 2>&1; then
      echo "❌ step failed in $dir: $*" >&2
      tail -n 40 "$log" >&2
      exit 1
    fi
  }
  (
    cd "$dir"
    mkdir -p dist/release
    # The same steps CI runs before the packs exist: the Hunspell import, the quality audit,
    # the review queues and the validated decision reports are derived, not committed.
    step import-hunspell kurdish-hunspell-kmr
    step audit-lexicon kurdish-hunspell-kmr
    step generate-review-queues kurdish-hunspell-kmr
    step validate-review-decisions kurdish-hunspell-kmr
    for pack in seed reviewed experimental-full; do
      step build-pack "$pack"
    done
    step build-release-bundle --out dist/release --json
  )
}

echo "--- building in clean checkout A"
build_one "$WORK/A"
echo "--- building in clean checkout B"
build_one "$WORK/B"

BUNDLE_A="$(find "$WORK/A/dist/release" -mindepth 1 -maxdepth 1 -type d -name 'kurmanci-*' | head -n1)"
BUNDLE_B="$WORK/B/dist/release/$(basename "$BUNDLE_A")"
[[ -d "$BUNDLE_A" && -d "$BUNDLE_B" ]] || { echo "❌ bundle directories not found" >&2; exit 1; }

echo "--- verifying bundle A"
(cd "$WORK/A" && cargo run --quiet --release -p kurmanci-data-builder -- verify-release-bundle "$BUNDLE_A")

echo "--- comparing A and B"
if ! diff -q "$BUNDLE_A/SHA256SUMS" "$BUNDLE_B/SHA256SUMS" >/dev/null; then
  echo "❌ SHA256SUMS differ between clean checkouts:" >&2
  diff "$BUNDLE_A/SHA256SUMS" "$BUNDLE_B/SHA256SUMS" >&2 || true
  exit 1
fi
if ! diff -r -q "$BUNDLE_A" "$BUNDLE_B" >/dev/null; then
  echo "❌ bundle contents differ between clean checkouts:" >&2
  diff -r -q "$BUNDLE_A" "$BUNDLE_B" >&2 || true
  exit 1
fi

SUMS_SHA="$(shasum -a 256 "$BUNDLE_A/SHA256SUMS" 2>/dev/null | awk '{print $1}' || sha256sum "$BUNDLE_A/SHA256SUMS" | awk '{print $1}')"
echo "✅ byte-identical release bundle from two clean checkouts of $REF_SHA"
echo "   bundle:            $(basename "$BUNDLE_A")"
echo "   files:             $(wc -l < "$BUNDLE_A/SHA256SUMS" | tr -d ' ') listed in SHA256SUMS"
echo "   SHA256SUMS sha256: $SUMS_SHA"
grep -E '  packs/[^/]+/lexicon\.bin$|  language-model/[^/]+/manifest\.json$|  provenance\.json$|  compatibility\.json$' "$BUNDLE_A/SHA256SUMS" | sed 's/^/   /'
