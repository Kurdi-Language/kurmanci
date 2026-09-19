#!/usr/bin/env bash
# Exercises the tracked-corpus pipeline (import, inventory, audit, partition, frequencies,
# n-grams, legacy compile, next-word evaluation) twice on an ISOLATED temporary root and
# fails unless every artifact is byte-identical across the two passes and the next-word
# evaluation accepts. The root holds a test-only corpus registry with exactly one corpus,
# `test-corpus`, whose text is data-builder/tests/fixtures/synthetic-corpus.txt (bytes for
# exercising deterministic code; no linguistic claim), plus copies of the repository's seed
# lexicon, source registry, pack policy, n-gram configuration and next-word evaluation
# cases. Nothing here touches the repository's data/, and the production corpus registry
# lists no test corpus: the external Kuwiki corpus is absent in CI, so this is how CI keeps
# covering the corpus pipeline without inventing a production corpus.
#
# Usage: scripts/corpus/verify-pipeline-on-test-fixture.sh [--keep]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIXTURES="$REPO_ROOT/data-builder/tests/fixtures"
KEEP=0
[[ "${1:-}" == "--keep" ]] && KEEP=1

ROOT="$(mktemp -d)"
cleanup() { [[ $KEEP -eq 1 ]] && { echo "kept: $ROOT"; return; }; rm -rf "$ROOT"; }
trap cleanup EXIT

# 1. The isolated root.
for rel in data/source-registry/sources.toml data/reviewed/lexicon.jsonl data/pack-policy.toml \
           data-builder/config/ngrams.toml data-builder/config/builder.toml \
           evaluation/next-word/cases.jsonl evaluation/next-word/trigram-cases.jsonl; do
  if [[ -f "$REPO_ROOT/$rel" ]]; then
    mkdir -p "$ROOT/$(dirname "$rel")"
    cp "$REPO_ROOT/$rel" "$ROOT/$rel"
  fi
done
TEXT="$FIXTURES/synthetic-corpus.txt"
SHA="$(shasum -a 256 "$TEXT" | cut -d' ' -f1)"
grep -q "sha256 = \"$SHA\"" "$FIXTURES/corpora.toml" || {
  echo "❌ tests/fixtures/corpora.toml does not pin the SHA-256 of synthetic-corpus.txt ($SHA)" >&2; exit 1; }
for d in data/original data/imported; do
  mkdir -p "$ROOT/$d/test-corpus"
  cp "$TEXT" "$ROOT/$d/test-corpus/corpus.txt"
done
cp "$FIXTURES/corpora.toml" "$ROOT/data/source-registry/corpora.toml"
echo "isolated root: $ROOT (test-corpus only; the repository's data/ is untouched)"

# 2. One pass of the pipeline, run from the isolated root (the commands resolve ./data).
run_pass() {
  local pass="$1"
  ( cd "$ROOT" && for cmd in import-all-corpora inventory-corpora audit-corpora partition-corpora \
                         build-frequencies build-ngrams build evaluate-next-word; do
      echo "--- pass $pass: $cmd"
      cargo run --quiet --manifest-path "$REPO_ROOT/Cargo.toml" -p kurmanci-data-builder -- "$cmd" \
        > "$ROOT/$cmd.pass$pass.log" 2>&1 || { echo "❌ $cmd failed in pass $pass:" >&2; tail -n 30 "$ROOT/$cmd.pass$pass.log" >&2; exit 1; }
    done )
  grep -q "Acceptance Passed: *true" "$ROOT/evaluate-next-word.pass$pass.log" || {
    echo "❌ next-word evaluation did not accept in pass $pass:" >&2; tail -n 20 "$ROOT/evaluate-next-word.pass$pass.log" >&2; exit 1; }
  ( cd "$ROOT" && find data/imported-canonical data/build data/reports -type f \
      ! -name '*.log' -exec shasum -a 256 {} + | sort ) > "$ROOT/checksums.pass$pass.txt"
}
run_pass 1
run_pass 2
if ! diff -u "$ROOT/checksums.pass1.txt" "$ROOT/checksums.pass2.txt"; then
  echo "❌ corpus pipeline artifacts differ between the two passes" >&2; exit 1
fi
COUNT="$(wc -l < "$ROOT/checksums.pass1.txt" | tr -d ' ')"
echo "✅ corpus pipeline on the test fixture: $COUNT artifacts byte-identical across two passes; next-word evaluation accepted in both"
