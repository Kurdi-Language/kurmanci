#!/usr/bin/env bash
# Post-review promotion report: the compact technical diff between two states of the
# repository, produced after human review decisions are merged and the production state is
# rebuilt. Numbers and identities only: entry counts per pack, decision counts per source and
# status, the held and excluded queue sizes, the language model's fingerprint and n-gram
# counts, pack sizes and hashes, the 357-case pairwise comparison, and optionally the engine
# heap and RSS from the bench. No word, no corpus text and no review note is ever printed.
#
# Usage:
#   scripts/review/promotion-report.sh run --base REF [--head REF | --working-tree] [--out FILE.md] [--json FILE.json] [--bench]
#       Derives the production state of REF and of --head REF (default: HEAD) in clean
#       clones exactly as scripts/release/verify-clean-checkout-determinism.sh does, each with
#       a data-builder (and, with --bench, a kurmanci-bench) built from that commit, runs
#       evaluate-packs in each, collects both states and writes the comparison (Markdown to
#       stdout or --out; JSON with both states and the deltas to --json). --working-tree uses
#       this working tree as the "after" state instead of a clone: it must pass
#       verify-production-state first, so that stale generated files are never reported.
#   scripts/review/promotion-report.sh collect TREE OUT.json [--bench-bin PATH]
#       Collects one state from a repository tree whose packs are built and whose
#       data/reports/pack-comparison/summary.json exists; --bench-bin runs that
#       kurmanci-bench (built from the same commit as the tree) on the prediction packs.
#   scripts/review/promotion-report.sh compare BEFORE.json AFTER.json [--out FILE.md] [--json FILE.json]
#       Compares two collected states.
# Needs bash, git, python3 and, for `run`, the Rust toolchain: each ref's state is derived
# and measured by binaries built from that ref's own commit (a later builder or engine must
# never be applied to older data; KURMANCI_DATA_BUILDER_BIN and KURMANCI_BENCH_BIN override
# this for the shell test only). Never assigns or changes a review status; it only reads the
# tracked decisions and the built artifacts.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PACKS=(seed reviewed experimental-full)

fail() { echo "❌ $*" >&2; exit 1; }

collect() {
  local tree="$1" out="$2" bench_bin="${3:-}"
  [[ -d "$tree" ]] || fail "not a directory: $tree"
  for pack in "${PACKS[@]}"; do
    [[ -f "$tree/data/build/packs/$pack/manifest.json" && -f "$tree/data/build/packs/$pack/lexicon.bin" ]] || fail "$tree: pack $pack is not built (data/build/packs/$pack); build the packs first"
  done
  [[ -f "$tree/data/reports/pack-comparison/summary.json" ]] || fail "$tree: data/reports/pack-comparison/summary.json missing; run evaluate-packs first"
  local commit dirty
  commit="$(git -C "$tree" rev-parse HEAD 2>/dev/null || echo unknown)"
  dirty="$( [[ -n "$(git -C "$tree" status --porcelain --untracked-files=no 2>/dev/null)" ]] && echo true || echo false)"
  local bench_json="{}"
  if [[ -n "$bench_bin" ]]; then
    [[ -x "$bench_bin" ]] || fail "kurmanci-bench not executable: $bench_bin"
    bench_json="{\"commit\": \"$commit\", \"bench_bin\": \"$bench_bin\","
    for pack in reviewed experimental-full; do
      local one
      one="$("$bench_bin" "$tree/data/build/packs/$pack/lexicon.bin" --json 2>/dev/null)" || fail "bench failed on $pack"
      bench_json="$bench_json\"$pack\": $(printf '%s' "$one" | python3 -c 'import json,sys; d=json.load(sys.stdin); l=d["load"]; print(json.dumps({"engine_heap_bytes": l["engine_heap_bytes"], "rss_after_load_bytes": l["rss_after_load_bytes"], "load_ms_median": l["load_ms_median"]}))'),"
    done
    bench_json="${bench_json%,}}"
  fi
  python3 - "$tree" "$out" "$commit" "$dirty" "$bench_json" <<'EOF'
import glob, hashlib, json, os, re, sys
tree, out, commit, dirty, bench = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4] == "true", json.loads(sys.argv[5])
def fail(msg):
    print(f"❌ {tree}: {msg}", file=sys.stderr); sys.exit(1)
state = {"schema_version": "promotion-report-state-v2", "commit": commit, "worktree_dirty": dirty, "packs": {}, "decisions": {}, "queues": {}, "language_model": {}, "pack_comparison": {}, "bench": bench}
model_refs = set()
for pack in ("seed", "reviewed", "experimental-full"):
    m = json.load(open(os.path.join(tree, "data/build/packs", pack, "manifest.json")))
    lexicon_path = os.path.join(tree, "data/build/packs", pack, "lexicon.bin")
    # The identity reported is the hash of the actual lexicon.bin, and it must be the one
    # the manifest recorded: a modified or corrupted pack is never reported under a stale hash.
    actual_sha = hashlib.sha256(open(lexicon_path, "rb").read()).hexdigest()
    if actual_sha != m.get("binary_sha256"):
        fail(f"pack {pack}: lexicon.bin sha256 {actual_sha} differs from the manifest's binary_sha256 {m.get('binary_sha256')}; the built pack and its manifest do not belong together")
    state["packs"][pack] = {
        "entries": m["final_unique_entry_count"], "lexicon_bytes": os.path.getsize(lexicon_path),
        "lexicon_sha256": actual_sha, "model_profile": m["model_profile"], "bigram_count": m.get("bigram_count", 0), "trigram_count": m.get("trigram_count", 0),
        "manual_seed_selected": m.get("manual_seed_selected_count", 0), "external_approved_selected": m.get("external_approved_selected_count", 0),
        "external_metadata_replacement_selected": m.get("external_metadata_replacement_selected_count", 0), "external_experimental_selected": m.get("external_experimental_selected_count", 0),
        "external_excluded_by_status": m.get("external_excluded_by_status_count", 0), "external_discarded_by_collision": m.get("external_discarded_by_collision_count", 0),
        "language_model_id": m.get("language_model_id"), "language_model_manifest_sha256": m.get("language_model_manifest_sha256"),
    }
    # Model-profile consistency: a pack without a model carries no model reference; a
    # model-backed pack carries both the model id and the model manifest hash it was built
    # against. Missing provenance is never reconstructed from whatever model is on disk.
    lm_id, lm_sha = m.get("language_model_id"), m.get("language_model_manifest_sha256")
    if m["model_profile"] == "none":
        if lm_id or lm_sha:
            fail(f"pack {pack}: model_profile 'none' but the manifest carries a language model reference ({lm_id!r}, {lm_sha!r})")
    else:
        if not lm_id or not lm_sha:
            fail(f"pack {pack}: model profile '{m['model_profile']}' requires language_model_id and language_model_manifest_sha256 in the manifest (got {lm_id!r}, {lm_sha!r}); provenance is never reconstructed from the model on disk")
        model_refs.add((lm_id, lm_sha))
for d in sorted(glob.glob(os.path.join(tree, "data/review-decisions", "*"))):
    path = os.path.join(d, "decisions.jsonl")
    if not os.path.isfile(path):
        continue
    counts = {}
    for line in open(path, encoding="utf-8"):
        if line.strip():
            status = json.loads(line).get("review_status", "?")
            counts[status] = counts.get(status, 0) + 1
    state["decisions"][os.path.basename(d)] = dict(sorted(counts.items()))
qdir = os.path.join(tree, "data/review-queues/kurdish-hunspell-kmr")
for name, key in (("hunspell-only.jsonl", "hunspell_ordinary_pool"), ("alphabet-policy-excluded.jsonl", "alphabet_policy_excluded"), ("punctuation-policy-needs-linguist.jsonl", "punctuation_policy_held")):
    path = os.path.join(qdir, name)
    state["queues"][key] = sum(1 for l in open(path, encoding="utf-8") if l.strip()) if os.path.isfile(path) else None
# The language model is the one the prediction packs reference, verified by the manifest hash
# the packs recorded, never a guessed directory.
if len(model_refs) > 1:
    fail(f"the model-backed packs reference different language models: {sorted(model_refs)}")
if model_refs:
    model_id, recorded_sha = next(iter(model_refs))
    if not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}", model_id):
        fail(f"invalid language model id {model_id!r} in the pack manifests")
    path = os.path.join(tree, "data/language-model", model_id, "manifest.json")
    if not os.path.isfile(path):
        fail(f"language model {model_id} referenced by the packs has no manifest at data/language-model/{model_id}/manifest.json")
    actual_sha = hashlib.sha256(open(path, "rb").read()).hexdigest()
    if actual_sha != recorded_sha:
        fail(f"language model {model_id}: manifest sha256 {actual_sha} differs from the one the packs recorded ({recorded_sha}); the packs are stale relative to the model")
    lm = json.load(open(path))
    state["language_model"] = {k: lm.get(k) for k in ("model_id", "vocabulary_fingerprint", "vocabulary_size", "unigram_count", "bigram_count", "trigram_count")}
    state["language_model"]["manifest_sha256"] = actual_sha
pc = json.load(open(os.path.join(tree, "data/reports/pack-comparison/summary.json")))
state["pack_comparison"] = {"total_reviewed_cases": pc.get("total_reviewed_cases"), "pairwise": {k: {kk: v[kk] for kk in ("improvement_count", "regression_count", "unchanged_count") if kk in v} for k, v in pc.get("pairwise_summaries", {}).items()}}
json.dump(state, open(out, "w", encoding="utf-8"), indent=1, sort_keys=True)
open(out, "a").write("\n")
print(f"✅ collected {tree} ({commit[:7]}{', dirty' if dirty else ''}) → {out}")
EOF
}

compare() {
  local before="$1" after="$2" out="${3:-}" json_out="${4:-}"
  python3 - "$before" "$after" "$out" "$json_out" <<'EOF'
import json, sys
b, a = json.load(open(sys.argv[1])), json.load(open(sys.argv[2]))
out_md, out_json = sys.argv[3], sys.argv[4]
rows = []
def num(label, x, y):
    if x is None and y is None:
        return
    d = (y or 0) - (x or 0)
    rows.append((label, f"{x:,}" if isinstance(x, int) else ("–" if x is None else f"{x:.2f}"), f"{y:,}" if isinstance(y, int) else ("–" if y is None else f"{y:.2f}"), (f"{d:+,}" if isinstance(d, int) else f"{d:+.2f}")))
def ident(label, x, y):
    rows.append((label, (x or "–")[:12] + ("…" if x and len(x) > 12 else ""), (y or "–")[:12] + ("…" if y and len(y) > 12 else ""), "same" if x == y else "changed"))
rows.append(("commit", b["commit"][:7] + (" (dirty)" if b.get("worktree_dirty") else ""), a["commit"][:7] + (" (dirty)" if a.get("worktree_dirty") else ""), ""))
for pack in ("seed", "reviewed", "experimental-full"):
    pb, pa = b["packs"].get(pack, {}), a["packs"].get(pack, {})
    num(f"{pack} entries", pb.get("entries"), pa.get("entries"))
    num(f"{pack} lexicon.bin bytes", pb.get("lexicon_bytes"), pa.get("lexicon_bytes"))
    ident(f"{pack} lexicon sha256", pb.get("lexicon_sha256"), pa.get("lexicon_sha256"))
    if pack != "seed":
        num(f"{pack} external approved selected", pb.get("external_approved_selected"), pa.get("external_approved_selected"))
        num(f"{pack} bigrams", pb.get("bigram_count"), pa.get("bigram_count"))
        num(f"{pack} trigrams", pb.get("trigram_count"), pa.get("trigram_count"))
for source in sorted(set(b["decisions"]) | set(a["decisions"])):
    sb, sa = b["decisions"].get(source, {}), a["decisions"].get(source, {})
    for status in sorted(set(sb) | set(sa)):
        num(f"decisions {source} {status}", sb.get(status, 0), sa.get(status, 0))
for key in ("hunspell_ordinary_pool", "alphabet_policy_excluded", "punctuation_policy_held"):
    num(f"queue {key}", b["queues"].get(key), a["queues"].get(key))
lb, la = b.get("language_model", {}), a.get("language_model", {})
ident("language model id", lb.get("model_id"), la.get("model_id"))
ident("LM manifest sha256", lb.get("manifest_sha256"), la.get("manifest_sha256"))
ident("LM vocabulary fingerprint", lb.get("vocabulary_fingerprint"), la.get("vocabulary_fingerprint"))
for key in ("vocabulary_size", "unigram_count", "bigram_count", "trigram_count"):
    num(f"LM {key}", lb.get(key), la.get(key))
num("357-case reviewed cases", b["pack_comparison"].get("total_reviewed_cases"), a["pack_comparison"].get("total_reviewed_cases"))
for pair in sorted(set(b["pack_comparison"].get("pairwise", {})) | set(a["pack_comparison"].get("pairwise", {}))):
    for key in ("improvement_count", "regression_count", "unchanged_count"):
        num(f"357 {pair} {key}", b["pack_comparison"].get("pairwise", {}).get(pair, {}).get(key), a["pack_comparison"].get("pairwise", {}).get(pair, {}).get(key))
bb, ba = b.get("bench", {}), a.get("bench", {})
if bb or ba:
    rows.append(("bench engine commit", (bb.get("commit") or "–")[:7], (ba.get("commit") or "–")[:7], "each side measured by its own engine"))
    for pack in ("reviewed", "experimental-full"):
        num(f"bench {pack} engine heap bytes", bb.get(pack, {}).get("engine_heap_bytes"), ba.get(pack, {}).get("engine_heap_bytes"))
        num(f"bench {pack} RSS after load bytes", bb.get(pack, {}).get("rss_after_load_bytes"), ba.get(pack, {}).get("rss_after_load_bytes"))
        num(f"bench {pack} load ms median", bb.get(pack, {}).get("load_ms_median"), ba.get(pack, {}).get("load_ms_median"))
md = ["| Metric | Before | After | Delta |", "|---|---|---|---|"] + [f"| {r[0]} | {r[1]} | {r[2]} | {r[3]} |" for r in rows]
text = "\n".join(md) + "\n"
if out_md:
    open(out_md, "w", encoding="utf-8").write(text)
else:
    sys.stdout.write(text)
if out_json:
    json.dump({"schema_version": "promotion-report-v2", "before": b, "after": a, "rows": [{"metric": r[0], "before": r[1], "after": r[2], "delta": r[3]} for r in rows]}, open(out_json, "w", encoding="utf-8"), indent=1, sort_keys=True)
    open(out_json, "a").write("\n")
EOF
}

# Builds a workspace binary from a checkout, under that checkout's own target directory.
build_bin() {  # <tree> <crate> <binary name>
  local tree="$1" crate="$2" name="$3"
  local target="${CARGO_TARGET_DIR:-$tree/target}"
  ( cd "$tree" && CARGO_TARGET_DIR="$target" cargo build --quiet --release -p "$crate" ) || fail "cargo build of $crate failed in $tree"
  printf '%s\n' "$target/release/$name"
}

# Derives the production state of a git ref in a clean clone and runs evaluate-packs there.
# The data-builder (and the bench) are built from the clone's own commit: a state is always
# derived and measured by the code of its commit, never by a later builder or engine applied
# to older data (KURMANCI_DATA_BUILDER_BIN / KURMANCI_BENCH_BIN: shell test only).
derive_tree() {  # <ref> <dir> <bench 0|1>; prints the bench binary path (or nothing)
  local ref="$1" dir="$2" bench="$3"
  local sha; sha="$(git -C "$REPO_ROOT" rev-parse --verify "$ref^{commit}")" || fail "unknown ref $ref"
  rm -rf "$dir"; git clone --quiet --no-hardlinks "$REPO_ROOT" "$dir"; git -C "$dir" checkout --quiet --detach "$sha"
  local bin="${KURMANCI_DATA_BUILDER_BIN:-}"
  [[ -n "$bin" ]] || bin="$(build_bin "$dir" kurmanci-data-builder data-builder)"
  ( cd "$dir"
    step() { "$bin" "$@" > /dev/null 2>"$dir.step.err" || { echo "❌ step failed in $dir: $*" >&2; tail -n 30 "$dir.step.err" >&2; exit 1; }; }
    step import-hunspell kurdish-hunspell-kmr
    step audit-lexicon kurdish-hunspell-kmr
    step generate-review-queues kurdish-hunspell-kmr
    step validate-review-decisions kurdish-hunspell-kmr
    for pack in "${PACKS[@]}"; do step build-pack "$pack"; done
    step evaluate-packs )
  if [[ "$bench" == "1" ]]; then
    local bench_bin="${KURMANCI_BENCH_BIN:-}"
    [[ -n "$bench_bin" ]] || bench_bin="$(build_bin "$dir" kurmanci-bench kurmanci-bench)"
    printf '%s\n' "$bench_bin"
  fi
}

run() {
  local base="" head="" working_tree=0 out="" json_out="" bench=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --base) base="$2"; shift 2 ;;
      --head) head="$2"; shift 2 ;;
      --working-tree) working_tree=1; shift ;;
      --out) out="$2"; shift 2 ;;
      --json) json_out="$2"; shift 2 ;;
      --bench) bench=1; shift ;;
      *) fail "unknown argument: $1" ;;
    esac
  done
  [[ -n "$base" ]] || fail "--base REF is required"
  [[ -z "$head" || $working_tree -eq 0 ]] || fail "--head and --working-tree exclude each other"
  local work; work="$(mktemp -d "${TMPDIR:-/tmp}/kurmanci-promotion-report.XXXXXX")"
  work="$(cd "$work" && pwd -P)"
  # shellcheck disable=SC2064  # expand now: the trap runs outside this function's scope
  trap "rm -rf '$work'" EXIT
  echo "=== before: $base (clean clone, derived by its own commit)"
  local before_bench; before_bench="$(derive_tree "$base" "$work/before" "$bench")"
  collect "$work/before" "$work/before.json" "$before_bench"
  if [[ $working_tree -eq 1 ]]; then
    echo "=== after: working tree $REPO_ROOT (must verify first)"
    local bin="${KURMANCI_DATA_BUILDER_BIN:-}"
    [[ -n "$bin" ]] || bin="$(build_bin "$REPO_ROOT" kurmanci-data-builder data-builder)"
    ( cd "$REPO_ROOT" && "$bin" verify-production-state > "$work/verify.log" 2>&1 ) || { echo "❌ the working tree does not pass verify-production-state; its generated files may be stale (rebuild-production), refusing to report it" >&2; tail -n 20 "$work/verify.log" >&2; exit 1; }
    ( cd "$REPO_ROOT" && "$bin" evaluate-packs > /dev/null )
    local after_bench=""
    if [[ $bench -eq 1 ]]; then after_bench="${KURMANCI_BENCH_BIN:-}"; [[ -n "$after_bench" ]] || after_bench="$(build_bin "$REPO_ROOT" kurmanci-bench kurmanci-bench)"; fi
    collect "$REPO_ROOT" "$work/after.json" "$after_bench"
  else
    head="${head:-HEAD}"
    echo "=== after: $head (clean clone, derived by its own commit)"
    local after_bench; after_bench="$(derive_tree "$head" "$work/after" "$bench")"
    collect "$work/after" "$work/after.json" "$after_bench"
  fi
  compare "$work/before.json" "$work/after.json" "$out" "$json_out"
  [[ -z "$out" ]] || echo "✅ report written to $out"
}

case "${1:-}" in
  run) shift; run "$@" ;;
  collect) shift; [[ $# -ge 2 ]] || fail "collect TREE OUT.json [--bench-bin PATH]"; t="$1"; o="$2"; shift 2; bb=""
    while [[ $# -gt 0 ]]; do case "$1" in --bench-bin) bb="$2"; shift 2 ;; *) fail "unknown argument: $1" ;; esac; done
    collect "$t" "$o" "$bb" ;;
  compare) shift; [[ $# -ge 2 ]] || fail "compare BEFORE.json AFTER.json [--out FILE.md] [--json FILE.json]"; bf="$1"; af="$2"; shift 2; o=""; j=""
    while [[ $# -gt 0 ]]; do case "$1" in --out) o="$2"; shift 2 ;; --json) j="$2"; shift 2 ;; *) fail "unknown argument: $1" ;; esac; done
    compare "$bf" "$af" "$o" "$j" ;;
  ""|-h|--help) sed -n '2,29p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' ;;
  *) fail "unknown command '$1' (run | collect | compare)" ;;
esac
