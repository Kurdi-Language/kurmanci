#!/usr/bin/env python3
"""Rank Hunspell entries (pending human review) by Kurmancî Wikipedia attestation.

Usage: build_hunspell_queue.py <out.json> [top=5000] [queue_id=hunspell-kuwiki-001] [--root DIR]

The candidate pool is data/review-queues/kurdish-hunspell-kmr/hunspell-only.jsonl, produced
by the authoritative Rust review-queue generator, which already applies the default-pack
alphabet policy (data-builder/src/alphabet.rs): entries outside the policy are written to
alphabet-policy-excluded.jsonl instead. This script does not define or re-evaluate the
policy; it only refuses to run if those two authoritative artifacts overlap.

Review aid only. It makes no linguistic decisions. Counts are a local join of
data/imported/kuwiki/documents.jsonl (raw imported articles) with the repository
tokenizer rules (NFC, lowercase, split on whitespace / P* / S*, drop letterless and
pure-numeric tokens).
"""
import json, sys, unicodedata, collections, hashlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
_argv = list(sys.argv[1:])
if "--root" in _argv:
    i = _argv.index("--root"); ROOT = Path(_argv[i + 1]).resolve(); del _argv[i:i + 2]
OUT = Path(_argv[0])
TOP = int(_argv[1]) if len(_argv) > 1 else 5000
QUEUE_ID = _argv[2] if len(_argv) > 2 else "hunspell-kuwiki-001"

def is_boundary(c):
    if c.isspace(): return True
    cat = unicodedata.category(c)
    return cat[0] in ("P", "S")

def is_letter(c):
    return unicodedata.category(c)[0] == "L"

def tokenize(text):
    s = unicodedata.normalize("NFC", text).lower()
    out, cur = [], []
    for c in s:
        if is_boundary(c):
            if cur:
                t = "".join(cur); cur = []
                if any(is_letter(ch) for ch in t) and not all(ch.isnumeric() for ch in t):
                    out.append(t)
        else:
            cur.append(c)
    if cur:
        t = "".join(cur)
        if any(is_letter(ch) for ch in t) and not all(ch.isnumeric() for ch in t):
            out.append(t)
    return out

def normalize_text(text):
    clean = "".join(ch for ch in text if not unicodedata.category(ch).startswith("C") and ch not in "​﻿")
    return unicodedata.normalize("NFC", clean).lower()

# 1. Hunspell pending entries
decided = set()
for src in ["kurdish-hunspell-kmr"]:
    for l in open(ROOT / f"data/review-decisions/{src}/decisions.jsonl", encoding="utf-8"):
        if l.strip(): decided.add(json.loads(l)["target_id"])

conflict_members = set()
for l in open(ROOT / "data/review-queues/kurdish-hunspell-kmr/metadata-conflict-groups.jsonl", encoding="utf-8"):
    if l.strip(): conflict_members.update(json.loads(l)["member_entry_ids"])

audit_flags = collections.defaultdict(set)
for name in ["suspicious-entries", "rare-code-points", "short-and-long-forms", "capitalization-anomalies", "digit-only", "punctuation-only", "symbol-only", "parser-rejections"]:
    p = ROOT / f"data/review-queues/kurdish-hunspell-kmr/{name}.jsonl"
    if p.exists():
        for l in open(p, encoding="utf-8"):
            if l.strip():
                d = json.loads(l)
                if d.get("target_type") == "entry": audit_flags[d["target_id"]].add(name)

# Consistency of the authoritative artifacts: an entry the Rust generator excluded under the
# alphabet policy must not also be in the ordinary pool. This checks the artifacts against
# each other; it does not re-evaluate any character.
policy_excluded = set()
excl_path = ROOT / "data/review-queues/kurdish-hunspell-kmr/alphabet-policy-excluded.jsonl"
if excl_path.exists():
    for l in open(excl_path, encoding="utf-8"):
        if l.strip(): policy_excluded.add(json.loads(l)["target_id"])

entries = []
by_norm = collections.defaultdict(list)
n_total = 0
for l in open(ROOT / "data/review-queues/kurdish-hunspell-kmr/hunspell-only.jsonl", encoding="utf-8"):
    if not l.strip(): continue
    d = json.loads(l); n_total += 1
    if d["target_id"] in policy_excluded:
        sys.stderr.write(f"ERROR: target {d['target_id']} ({d['display']!r}) is in both hunspell-only.jsonl and alphabet-policy-excluded.jsonl; regenerate the review queues (generate-review-queues) before building a Review Desk queue\n")
        sys.exit(1)
    entries.append(d)
    by_norm[d["normalized"]].append(d)

# 2. Wikipedia attestation
doc_counts = collections.Counter()
tok_counts = collections.Counter()
ndocs = 0
with open(ROOT / "data/imported/kuwiki/documents.jsonl", encoding="utf-8") as f:
    for line in f:
        if not line.strip(): continue
        doc = json.loads(line); ndocs += 1
        toks = tokenize(doc.get("text", "") + "\n" + doc.get("title", ""))
        tok_counts.update(toks)
        doc_counts.update(set(toks))
sys.stderr.write(f"documents tokenized: {ndocs}\n")

# 3. Join, filter mechanically, rank
skip = collections.Counter()
rows = []
for d in entries:
    tid = d["target_id"]
    if tid in decided: skip["already_decided"] += 1; continue
    if tid in conflict_members: skip["conflict_group_member"] += 1; continue
    disp = d["display"]; norm = d["normalized"]
    if d.get("part_of_speech") == "punctuation" or not any(is_letter(c) for c in disp): skip["no_letters_or_punctuation"] += 1; continue
    if any(c.isdigit() for c in disp): skip["contains_digit"] += 1; continue
    dc = doc_counts.get(norm, 0); tc = tok_counts.get(norm, 0)
    if dc == 0: skip["not_attested"] += 1; continue
    rows.append({
        "target_id": tid,
        "display": disp,
        "normalized": norm,
        "pos": d.get("part_of_speech") or "unknown",
        "morphology": d.get("morphology") or [],
        "flags": d.get("flags") or "",
        "source_lines": d.get("source_lines") or [],
        "doc_count": dc,
        "token_count": tc,
        "audit": sorted(audit_flags.get(tid, [])),
        "capitalized": disp[:1].isupper(),
    })
rows.sort(key=lambda r: (-r["doc_count"], -r["token_count"], r["normalized"], r["display"]))
for i, r in enumerate(rows, 1): r["rank"] = i

summary = {
    "queue_id": QUEUE_ID,
    "source_id": "kurdish-hunspell-kmr",
    "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
    "corpus": "kuwiki (data/imported/kuwiki/documents.jsonl, local join, raw imported articles)",
    "documents_tokenized": ndocs,
    "hunspell_records_total": n_total,
    "alphabet_policy_excluded_by_review_infrastructure": len(policy_excluded),
    "skipped": dict(skip),
    "attested_pending_entries": len(rows),
    "selected": min(TOP, len(rows)),
    "doc_count_at_selected_cutoff": rows[min(TOP, len(rows)) - 1]["doc_count"] if rows else 0,
}
sys.stderr.write(json.dumps(summary, indent=2, ensure_ascii=False) + "\n")
OUT.write_text(json.dumps({"summary": summary, "candidates": rows[:TOP]}, ensure_ascii=False), encoding="utf-8")
(OUT.with_suffix(".full.json")).write_text(json.dumps({"summary": summary, "candidates": rows}, ensure_ascii=False), encoding="utf-8")

# 4. Review Desk file: the site (index.html) reads compact candidate arrays
#    [rank, target_id, display, normalized ("" when it is display.lower()), pos, morphology
#    joined by ";", flags, first source line, doc_count, token_count, audit flags joined by ";"]
batch_no = QUEUE_ID.rsplit("-", 1)[-1]
desk_summary = dict(summary, queue_label=f"Hunspell core × Wikipedia attestation · batch {batch_no}")
compact = [[r["rank"], r["target_id"], r["display"], "" if r["normalized"] == r["display"].lower() else r["normalized"],
            r["pos"], ";".join(r["morphology"]), r["flags"], (r["source_lines"] or [0])[0], r["doc_count"], r["token_count"],
            ",".join(r["audit"])] for r in rows[:TOP]]
js = ("// Generated review queue. Do not edit by hand; regenerate with scripts/review-desk/build_hunspell_queue.py in the kurmanci repo.\n"
      "window.REVIEW_QUEUE = " + json.dumps({"summary": desk_summary, "candidates": compact}, ensure_ascii=False, separators=(",", ":")) + ";\n")
(OUT.with_suffix(".js")).write_text(js, encoding="utf-8")
sys.stderr.write(f"review desk file: {OUT.with_suffix('.js')}\n")
