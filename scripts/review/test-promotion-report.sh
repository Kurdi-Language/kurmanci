#!/usr/bin/env bash
# Shell-level check of scripts/review/promotion-report.sh. `collect` and `compare` run on two
# fixture trees (fake pack manifests, decisions, queues, language-model manifest and pack
# comparison summary): the collected state has the expected numbers, the language model is
# the one the prediction packs reference and its manifest hash must match what the packs
# recorded, a fake bench run is attributed to the tree's commit, the comparison reports the
# deltas and the changed identities, trees without built packs or without the pack comparison
# are refused, and no word from a decision file reaches the output. `run` is driven against a
# temporary repository with a fake data-builder and a fake bench that log their calls: both
# sides are derived in clean clones (never this working tree) and measured by a bench
# invocation per tree, and --working-tree is refused unless verify-production-state passes.
# Needs bash, git and python3; no Rust.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPORT="$SCRIPT_DIR/promotion-report.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
bash -n "$REPORT" && echo "✅ promotion-report.sh passes bash -n"

sha() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi; }

# write_state <dir> <reviewed entries> <approved> <needs_linguist> <fingerprint> <bigrams>:
# the generated files of a built tree (packs, queues, LM manifest, pack comparison).
write_state() {
  local dir="$1" reviewed="$2" approved="$3" linguist="$4" fp="$5" bigrams="$6"
  mkdir -p "$dir/data/review-decisions/kurdish-hunspell-kmr" "$dir/data/review-queues/kurdish-hunspell-kmr" "$dir/data/language-model/kuwiki-x" "$dir/data/reports/pack-comparison"
  printf '{"model_id":"kuwiki-x","vocabulary_fingerprint":"%s","vocabulary_size":%s,"unigram_count":100,"bigram_count":%s,"trigram_count":%s}\n' "$fp" "$((reviewed + 40000))" "$bigrams" "$((bigrams + 100))" > "$dir/data/language-model/kuwiki-x/manifest.json"
  local lm_sha; lm_sha="$(sha "$dir/data/language-model/kuwiki-x/manifest.json")"
  local pack
  for pack in seed reviewed experimental-full; do
    mkdir -p "$dir/data/build/packs/$pack"
    local n=33; [[ "$pack" == reviewed ]] && n="$reviewed"; [[ "$pack" == experimental-full ]] && n=42000
    local model='"model_profile":"none"'; [[ "$pack" != seed ]] && model="\"model_profile\":\"prediction\",\"language_model_id\":\"kuwiki-x\",\"language_model_manifest_sha256\":\"$lm_sha\""
    # A distinct lexicon.bin per pack and per state; the manifest carries its real hash.
    printf '%s:%s:%s:' "$pack" "$n" "$fp" > "$dir/data/build/packs/$pack/lexicon.bin"
    head -c "$((n * 10))" /dev/zero >> "$dir/data/build/packs/$pack/lexicon.bin"
    printf '{"final_unique_entry_count":%s,"binary_sha256":"%s",%s,"bigram_count":%s,"trigram_count":%s,"manual_seed_selected_count":33,"external_approved_selected_count":%s,"external_metadata_replacement_selected_count":0,"external_experimental_selected_count":0,"external_excluded_by_status_count":0,"external_discarded_by_collision_count":0}\n' \
      "$n" "$(sha "$dir/data/build/packs/$pack/lexicon.bin")" "$model" "$bigrams" "$((bigrams + 100))" "$((n - 33))" > "$dir/data/build/packs/$pack/manifest.json"
  done
  {
    for ((i = 0; i < approved; i++)); do printf '{"target_id":"%s","review_status":"approved","review_notes":"SECRETWORD%s"}\n' "$i" "$i"; done
    for ((i = 0; i < linguist; i++)); do printf '{"target_id":"l%s","review_status":"needs_linguist","review_notes":"SECRETWORD"}\n' "$i"; done
  } > "$dir/data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl"
  for ((i = 0; i < 5; i++)); do printf '{"normalized":"held%s"}\n' "$i"; done > "$dir/data/review-queues/kurdish-hunspell-kmr/punctuation-policy-needs-linguist.jsonl"
  for ((i = 0; i < 7; i++)); do printf '{"normalized":"pool%s"}\n' "$i"; done > "$dir/data/review-queues/kurdish-hunspell-kmr/hunspell-only.jsonl"
  printf '{"total_reviewed_cases":357,"pairwise_summaries":{"reviewed_vs_seed":{"improvement_count":%s,"regression_count":4,"unchanged_count":%s},"experimental_vs_seed":{"improvement_count":250,"regression_count":8,"unchanged_count":99}}}\n' "$((reviewed / 30))" "$((357 - 4 - reviewed / 30))" > "$dir/data/reports/pack-comparison/summary.json"
}

make_tree() {
  local dir="$1"; shift
  rm -rf "$dir"; mkdir -p "$dir"
  git init -q "$dir"; git -C "$dir" config user.name t; git -C "$dir" config user.email t@example.invalid
  write_state "$dir" "$@"
  git -C "$dir" add -A >/dev/null 2>&1 || true; git -C "$dir" -c commit.gpgsign=false commit -q -m state >/dev/null 2>&1 || true
}

# A fake kurmanci-bench: logs the pack it measured and answers with fixed load numbers.
FAKE_BENCH="$TMP/fake-bench"; BENCH_LOG="$TMP/bench-calls.log"
cat > "$FAKE_BENCH" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$1" >> "$BENCH_LOG"
echo '{"load":{"engine_heap_bytes":2427841,"rss_after_load_bytes":7503872,"load_ms_median":9.93},"latencies":[]}'
EOF
chmod +x "$FAKE_BENCH"

make_tree "$TMP/before" 2000 800 400 "aaaaaaaaaaaaaaaa" 40000
make_tree "$TMP/after" 2143 810 467 "bbbbbbbbbbbbbbbb" 44820

# 1. collect: numbers, the referenced language model, bench attributed to the tree's commit.
"$REPORT" collect "$TMP/before" "$TMP/before.json" > /dev/null
"$REPORT" collect "$TMP/after" "$TMP/after.json" --bench-bin "$FAKE_BENCH" > /dev/null
python3 - "$TMP/after.json" "$(git -C "$TMP/after" rev-parse HEAD)" "$(sha "$TMP/after/data/language-model/kuwiki-x/manifest.json")" "$TMP/after" <<'EOF'
import json, sys
s, commit, lm_sha = json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3]
assert s["packs"]["reviewed"]["entries"] == 2143 and s["packs"]["reviewed"]["lexicon_bytes"] > 21430, s["packs"]["reviewed"]
import hashlib, os
for pack in ("seed", "reviewed", "experimental-full"):
    actual = hashlib.sha256(open(os.path.join(sys.argv[4], "data/build/packs", pack, "lexicon.bin"), "rb").read()).hexdigest()
    assert s["packs"][pack]["lexicon_sha256"] == actual, pack
assert s["decisions"]["kurdish-hunspell-kmr"] == {"approved": 810, "needs_linguist": 467}, s["decisions"]
assert s["queues"] == {"hunspell_ordinary_pool": 7, "alphabet_policy_excluded": None, "punctuation_policy_held": 5}, s["queues"]
assert s["language_model"]["model_id"] == "kuwiki-x" and s["language_model"]["manifest_sha256"] == lm_sha, s["language_model"]
assert s["language_model"]["bigram_count"] == 44820 and s["language_model"]["vocabulary_fingerprint"] == "bbbbbbbbbbbbbbbb"
assert s["pack_comparison"]["pairwise"]["reviewed_vs_seed"]["improvement_count"] == 71
assert s["bench"]["commit"] == commit and s["bench"]["reviewed"]["engine_heap_bytes"] == 2427841 and s["bench"]["experimental-full"]["load_ms_median"] == 9.93, s["bench"]
EOF
grep -c "after/data/build/packs/" "$BENCH_LOG" | grep -qx 2 || { echo "❌ bench was not run once per prediction pack of the tree" >&2; cat "$BENCH_LOG" >&2; exit 1; }
echo "✅ collect reads pack manifests, verifies every lexicon.bin against its manifest hash, reads decisions, queues, the referenced language model and the pack comparison, and attributes the bench to the tree's commit"

# 2. compare: deltas and changed identities, Markdown and JSON.
"$REPORT" compare "$TMP/before.json" "$TMP/after.json" --out "$TMP/report.md" --json "$TMP/report.json"
for needle in \
  '| reviewed entries | 2,000 | 2,143 | +143 |' \
  '| decisions kurdish-hunspell-kmr approved | 800 | 810 | +10 |' \
  '| decisions kurdish-hunspell-kmr needs_linguist | 400 | 467 | +67 |' \
  '| LM vocabulary fingerprint | aaaaaaaaaaaa… | bbbbbbbbbbbb… | changed |' \
  '| LM bigram_count | 40,000 | 44,820 | +4,820 |' \
  '| 357 reviewed_vs_seed improvement_count | 66 | 71 | +5 |' \
  '| queue punctuation_policy_held | 5 | 5 | +0 |' \
  '| bench reviewed engine heap bytes | – | 2,427,841 |'; do
  grep -qF -- "$needle" "$TMP/report.md" || { echo "❌ row missing: $needle" >&2; cat "$TMP/report.md" >&2; exit 1; }
done
grep -q 'reviewed lexicon sha256 .* changed' "$TMP/report.md" || { echo "❌ pack identity row missing" >&2; exit 1; }
grep -q 'LM manifest sha256 .* changed' "$TMP/report.md" || { echo "❌ LM manifest identity row missing" >&2; exit 1; }
grep -q 'bench engine commit | – | .* | each side measured by its own engine' "$TMP/report.md" || { echo "❌ bench commit row missing" >&2; cat "$TMP/report.md" >&2; exit 1; }
python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); assert r["schema_version"]=="promotion-report-v2" and r["after"]["packs"]["reviewed"]["entries"]==2143 and any(x["metric"]=="reviewed entries" and x["delta"]=="+143" for x in r["rows"])' "$TMP/report.json"
echo "✅ compare reports the deltas, the changed identities and the bench commit per side"

# 3. Nothing from a decision note or a queue reaches any output.
if grep -q 'SECRETWORD\|held0\|pool0' "$TMP/report.md" "$TMP/report.json" "$TMP/before.json" "$TMP/after.json"; then
  echo "❌ a decision note or queue word leaked into the report" >&2; exit 1
fi
echo "✅ no decision note or queue word reaches the report"

# 4. Refusals: no packs; no pack comparison; the packs reference a model whose manifest hash
#    differs from the one they recorded (stale packs).
rm -rf "$TMP/nopacks"; mkdir -p "$TMP/nopacks"
if "$REPORT" collect "$TMP/nopacks" "$TMP/nopacks.json" > "$TMP/nopacks.log" 2>&1; then echo "❌ collect accepted a tree without built packs" >&2; exit 1; fi
grep -q "is not built" "$TMP/nopacks.log" || { echo "❌ refusal does not name the missing packs" >&2; cat "$TMP/nopacks.log" >&2; exit 1; }
make_tree "$TMP/stale" 2143 810 467 "cccccccccccccccc" 44820
printf '{"model_id":"kuwiki-x","vocabulary_fingerprint":"dddd","vocabulary_size":1,"unigram_count":1,"bigram_count":1,"trigram_count":1}\n' > "$TMP/stale/data/language-model/kuwiki-x/manifest.json"
if "$REPORT" collect "$TMP/stale" "$TMP/stale.json" > "$TMP/stale.log" 2>&1; then echo "❌ collect accepted packs whose recorded model manifest hash differs from the model" >&2; exit 1; fi
grep -q "differs from the one the packs recorded" "$TMP/stale.log" || { echo "❌ stale-pack refusal not reported" >&2; cat "$TMP/stale.log" >&2; exit 1; }
# A lexicon.bin modified without its manifest (same size): refused, naming the pack, and no
# state JSON is written.
make_tree "$TMP/tampered" 2143 810 467 "eeeeeeeeeeeeeeee" 44820
printf 'X' | dd of="$TMP/tampered/data/build/packs/reviewed/lexicon.bin" bs=1 seek=100 conv=notrunc status=none
if "$REPORT" collect "$TMP/tampered" "$TMP/tampered.json" > "$TMP/tampered.log" 2>&1; then echo "❌ collect accepted a lexicon.bin that does not match its manifest hash" >&2; exit 1; fi
grep -q "pack reviewed: lexicon.bin sha256 .* differs from the manifest's binary_sha256" "$TMP/tampered.log" || { echo "❌ tampered-pack refusal not reported" >&2; cat "$TMP/tampered.log" >&2; exit 1; }
[[ ! -f "$TMP/tampered.json" ]] || { echo "❌ a state JSON was written for a tampered pack" >&2; exit 1; }
# Model-profile consistency: prediction packs without the model manifest hash, prediction
# packs without the model id, and a model-less pack carrying a model reference are refused,
# and no state JSON is written.
strip_field() {  # <tree> <pack> <field>
  python3 - "$1/data/build/packs/$2/manifest.json" "$3" <<'PY'
import json, sys
p = sys.argv[1]; m = json.load(open(p)); m.pop(sys.argv[2], None); json.dump(m, open(p, "w"))
PY
}
make_tree "$TMP/nosha" 2143 810 467 "ffffffffffffffff" 44820
strip_field "$TMP/nosha" reviewed language_model_manifest_sha256; strip_field "$TMP/nosha" experimental-full language_model_manifest_sha256
if "$REPORT" collect "$TMP/nosha" "$TMP/nosha.json" > "$TMP/nosha.log" 2>&1; then echo "❌ collect accepted prediction packs without the model manifest hash" >&2; exit 1; fi
grep -q "requires language_model_id and language_model_manifest_sha256" "$TMP/nosha.log" || { echo "❌ missing-hash refusal not reported" >&2; cat "$TMP/nosha.log" >&2; exit 1; }
[[ ! -f "$TMP/nosha.json" ]] || { echo "❌ a state JSON was written without the model hash" >&2; exit 1; }
make_tree "$TMP/noid" 2143 810 467 "0000000000000000" 44820
strip_field "$TMP/noid" reviewed language_model_id; strip_field "$TMP/noid" experimental-full language_model_id
if "$REPORT" collect "$TMP/noid" "$TMP/noid.json" > "$TMP/noid.log" 2>&1; then echo "❌ collect accepted prediction packs without the model id" >&2; exit 1; fi
grep -q "requires language_model_id and language_model_manifest_sha256" "$TMP/noid.log" || { echo "❌ missing-id refusal not reported" >&2; cat "$TMP/noid.log" >&2; exit 1; }
[[ ! -f "$TMP/noid.json" ]] || { echo "❌ a state JSON was written without the model id" >&2; exit 1; }
make_tree "$TMP/seedref" 2143 810 467 "1111111111111111" 44820
python3 - "$TMP/seedref/data/build/packs/seed/manifest.json" <<'PY'
import json, sys
p = sys.argv[1]; m = json.load(open(p)); m["language_model_id"] = "kuwiki-x"; json.dump(m, open(p, "w"))
PY
if "$REPORT" collect "$TMP/seedref" "$TMP/seedref.json" > "$TMP/seedref.log" 2>&1; then echo "❌ collect accepted a model-less pack carrying a model reference" >&2; exit 1; fi
grep -q "model_profile 'none' but the manifest carries a language model reference" "$TMP/seedref.log" || { echo "❌ seed-reference refusal not reported" >&2; cat "$TMP/seedref.log" >&2; exit 1; }
[[ ! -f "$TMP/seedref.json" ]] || { echo "❌ a state JSON was written for a model-less pack with a model reference" >&2; exit 1; }
echo "✅ model-profile consistency is enforced: missing model hash, missing model id and a model reference on a model-less pack are refused without writing a state"
rm -f "$TMP/after/data/reports/pack-comparison/summary.json"
if "$REPORT" collect "$TMP/after" "$TMP/x.json" > "$TMP/nocmp.log" 2>&1; then echo "❌ collect accepted a tree without the pack comparison" >&2; exit 1; fi
grep -q "run evaluate-packs first" "$TMP/nocmp.log" || { echo "❌ refusal does not name the missing comparison" >&2; exit 1; }
[[ ! -f "$TMP/x.json" ]] || { echo "❌ a state JSON was written without the pack comparison" >&2; exit 1; }
echo "✅ trees without built packs, without the pack comparison, with packs stale relative to their model, or with a lexicon.bin that does not match its manifest are refused without writing a state"

# --- run: clean clones for both sides, binaries per tree, working tree only when verified --
# A temporary repository standing in for the project: the fake data-builder writes the
# generated state into whatever tree it runs in, and logs every call with its cwd.
REPO="$TMP/repo"; rm -rf "$REPO"; mkdir -p "$REPO"
git init -q -b main "$REPO"; git -C "$REPO" config user.name t; git -C "$REPO" config user.email t@example.invalid
mkdir -p "$REPO/scripts/review"; cp "$REPORT" "$REPO/scripts/review/promotion-report.sh"
printf 'data/build/\ndata/reports/pack-comparison/\n' > "$REPO/.gitignore"
printf 'v1\n' > "$REPO/marker.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -q -m "base"
BASE="$(git -C "$REPO" rev-parse HEAD)"
printf 'v2\n' > "$REPO/marker.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -q -m "head"
HEAD_SHA="$(git -C "$REPO" rev-parse HEAD)"
FAKE_BUILDER="$TMP/fake-data-builder"; BUILDER_LOG="$TMP/builder-calls.log"
cat > "$FAKE_BUILDER" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf '%s %s\n' "\$(pwd)" "\$1" >> "$BUILDER_LOG"
case "\$1" in
  build-pack) if [[ ! -d data/build/packs ]]; then source "$TMP/write-state.sh"; write_state "\$(pwd)" 2143 810 467 "\$(git rev-parse HEAD)" 44820; fi ;;
  verify-production-state) [[ "\${FAKE_VERIFY:-ok}" == ok ]] || { echo "production state: NOT OK"; exit 1; }; echo "production state: OK" ;;
  *) : ;;
esac
EOF
chmod +x "$FAKE_BUILDER"
declare -f sha write_state > "$TMP/write-state.sh"
cd "$REPO"
: > "$BUILDER_LOG"; : > "$BENCH_LOG"
KURMANCI_DATA_BUILDER_BIN="$FAKE_BUILDER" KURMANCI_BENCH_BIN="$FAKE_BENCH" \
  scripts/review/promotion-report.sh run --base "$BASE" --bench --json "$TMP/run.json" > "$TMP/run.log" 2>&1 || { echo "❌ run failed" >&2; cat "$TMP/run.log" >&2; exit 1; }
python3 - "$TMP/run.json" "$BASE" "$HEAD_SHA" "$REPO" "$BUILDER_LOG" "$BENCH_LOG" <<'EOF'
import json, sys
r, base, head, repo, builder_log, bench_log = json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5], sys.argv[6]
assert r["before"]["commit"] == base and r["after"]["commit"] == head, (r["before"]["commit"], r["after"]["commit"])
assert r["before"]["language_model"]["vocabulary_fingerprint"] == base and r["after"]["language_model"]["vocabulary_fingerprint"] == head
assert r["before"]["bench"]["commit"] == base and r["after"]["bench"]["commit"] == head, (r["before"]["bench"], r["after"]["bench"])
calls = [l.split(" ", 1) for l in open(builder_log).read().splitlines()]
trees = {cwd for cwd, _ in calls}
assert repo not in trees and all(t != repo for t in trees), f"the working tree was used as a state: {trees}"
assert len(trees) == 2, trees
for t in trees:
    steps = [s for cwd, s in calls if cwd == t]
    assert steps[:4] == ["import-hunspell", "audit-lexicon", "generate-review-queues", "validate-review-decisions"] and steps.count("build-pack") == 3 and steps[-1] == "evaluate-packs", steps
bench_trees = {l.rsplit("/data/build/packs/", 1)[0] for l in open(bench_log).read().splitlines()}
assert bench_trees == trees, (bench_trees, trees)
EOF
echo "✅ run derives base and HEAD in clean clones (never the working tree), in the determinism order, and benches each tree separately"

# --working-tree: refused when verify-production-state fails; accepted (and verified first)
# when it passes.
: > "$BUILDER_LOG"
if FAKE_VERIFY=fail KURMANCI_DATA_BUILDER_BIN="$FAKE_BUILDER" scripts/review/promotion-report.sh run --base "$BASE" --working-tree --json "$TMP/wt.json" > "$TMP/wt-fail.log" 2>&1; then
  echo "❌ --working-tree accepted a tree that fails verify-production-state" >&2; exit 1
fi
grep -q "does not pass verify-production-state" "$TMP/wt-fail.log" || { echo "❌ working-tree refusal not reported" >&2; cat "$TMP/wt-fail.log" >&2; exit 1; }
[[ ! -f "$TMP/wt.json" ]] || { echo "❌ a report was written for an unverified working tree" >&2; exit 1; }
write_state "$REPO" 2143 810 467 "$HEAD_SHA" 44820
: > "$BUILDER_LOG"
KURMANCI_DATA_BUILDER_BIN="$FAKE_BUILDER" scripts/review/promotion-report.sh run --base "$BASE" --working-tree --json "$TMP/wt.json" > "$TMP/wt-ok.log" 2>&1 || { echo "❌ verified working tree refused" >&2; cat "$TMP/wt-ok.log" >&2; exit 1; }
grep -q "^$REPO verify-production-state" "$BUILDER_LOG" || { echo "❌ the working tree was not verified before collection" >&2; cat "$BUILDER_LOG" >&2; exit 1; }
python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); assert r["after"]["commit"]==sys.argv[2]' "$TMP/wt.json" "$HEAD_SHA"
echo "✅ --working-tree is refused unless verify-production-state passes, and verified first when it does"
if scripts/review/promotion-report.sh run --base "$BASE" --head "$HEAD_SHA" --working-tree > "$TMP/excl.log" 2>&1; then echo "❌ --head and --working-tree accepted together" >&2; exit 1; fi
grep -q "exclude each other" "$TMP/excl.log" || { echo "❌ exclusivity not reported" >&2; exit 1; }
echo "✅ --head and --working-tree exclude each other"
