#!/usr/bin/env python3
"""Merge a Kurmancî Review Desk export into a decisions.jsonl store.

The export (schema review-desk-export-v1) already contains review-decision-v1
records written from explicit human choices on the review site. This script
only transcribes them: it never invents, changes, or infers a decision.

Safety rules (all checked before anything is written; any failure leaves the
store byte-for-byte unchanged):
  * every record's (target_type, target_id) must be a real target of the stated
    source's review queues under data/review-queues/<source_id>/ ; a record that
    is not in the queues, belongs to another source, or has an unsupported
    target_type is refused (the Review Desk reviews entries; conflict_group
    decisions are refused explicitly rather than half-supported)
  * refuses records whose review_status is not a repository status, or that
    lack reviewer_id / review_date, or whose replacement metadata is inconsistent
  * a target that already has a stored decision: identical decision -> reported
    as an idempotent no-op and not appended; different decision -> refused with
    the conflict named (the stored human decision is never overwritten, the new
    one is never silently dropped)
  * appends in export order, atomically (temp file, then rename)

Usage:
  python3 scripts/review-desk/merge_review_desk_export.py <export.json> [--apply] [--root DIR]

Without --apply it is a dry run. Afterwards run:
  cargo run -p kurmanci-data-builder -- validate-review-decisions kurdish-hunspell-kmr
"""
import json, os, sys, re
from pathlib import Path

DEFAULT_ROOT = Path(__file__).resolve().parents[2]
VALID = {"approved", "approved_with_metadata_change", "rejected_from_default_pack", "experimental_only", "needs_linguist", "needs_source_investigation"}
SUPPORTED_TARGET_TYPES = {"entry"}
SEMANTIC_FIELDS = ("target_type", "target_id", "source_id", "review_status", "reviewer_id", "review_date", "review_notes", "evidence", "replacement_metadata")


def normalize_text(text):
    import unicodedata
    clean = "".join(ch for ch in text if not unicodedata.category(ch).startswith("C") and ch not in "​﻿")
    return unicodedata.normalize("NFC", clean).lower()


def die(msg):
    raise SystemExit(f"ERROR: {msg}")


def load_queue_targets(root, source_id):
    """(target_type, target_id) of every record in the source's review queues."""
    qdir = root / "data/review-queues" / source_id
    if not qdir.is_dir():
        die(f"review queues not found for source {source_id!r}: {qdir}")
    targets = set()
    for path in sorted(qdir.glob("*.jsonl")):
        for line in path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            rec = json.loads(line)
            targets.add((rec.get("target_type"), rec.get("target_id")))
    return targets


def canonical_decision(rec, source_id):
    """The repository record for an export record (representation only; no content change)."""
    out = {
        "schema_version": "review-decision-v1",
        "target_type": rec["target_type"],
        "target_id": rec["target_id"],
        "source_id": source_id,
        "review_status": rec["review_status"],
        "reviewer_id": rec["reviewer_id"],
        "review_date": rec["review_date"],
        "review_notes": rec.get("review_notes") or None,
        "evidence": list(rec.get("evidence") or []),
    }
    if out["review_notes"] is None:
        del out["review_notes"]
    repl = rec.get("replacement_metadata")
    if repl is not None:
        out["replacement_metadata"] = {
            "display": repl["display"],
            "normalized": repl["normalized"],
            "flags": repl.get("flags") or "",
            "morphology": list(repl.get("morphology") or []),
            "part_of_speech": repl.get("part_of_speech"),
        }
    return out


def semantic_view(rec):
    """The human-decision fields, with only non-semantic representation differences removed
    (missing vs empty notes/evidence, key order)."""
    view = {}
    for f in SEMANTIC_FIELDS:
        v = rec.get(f)
        if f == "review_notes":
            v = (v or "").strip() or None
        elif f == "evidence":
            v = list(v or [])
        elif f == "replacement_metadata" and isinstance(v, dict):
            v = {
                "display": v.get("display"),
                "normalized": v.get("normalized"),
                "flags": v.get("flags") or "",
                "morphology": list(v.get("morphology") or []),
                "part_of_speech": v.get("part_of_speech"),
            }
        view[f] = v
    return view


def describe_conflict(stored, new):
    a, b = semantic_view(stored), semantic_view(new)
    diffs = [f for f in SEMANTIC_FIELDS if a[f] != b[f]]
    return ", ".join(f"{f}: stored={a[f]!r} export={b[f]!r}" for f in diffs) or "no semantic difference"


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv:
        die(__doc__)
    apply = "--apply" in argv
    root = DEFAULT_ROOT
    if "--root" in argv:
        root = Path(argv[argv.index("--root") + 1]).resolve()
    positional = [a for i, a in enumerate(argv) if not a.startswith("--") and (i == 0 or argv[i - 1] != "--root")]
    export_path = Path(positional[0])
    exp = json.loads(export_path.read_text(encoding="utf-8"))
    if exp.get("schema_version") != "review-desk-export-v1":
        die("not a review-desk-export-v1 file")
    source_id = exp["source_id"]
    store = root / "data/review-decisions" / source_id / "decisions.jsonl"
    if not store.exists():
        die(f"decision store not found: {store}")
    store_bytes = store.read_bytes()

    queue_targets = load_queue_targets(root, source_id)
    existing = {}
    for n, line in enumerate(store_bytes.decode("utf-8").splitlines(), 1):
        if line.strip():
            rec = json.loads(line)
            existing[(rec["target_type"], rec["target_id"])] = rec

    new, identical, conflicts, bad = [], [], [], []
    seen_in_export = set()
    for rec in exp["decisions"]:
        key = (rec.get("target_type"), rec.get("target_id"))
        if rec.get("schema_version") != "review-decision-v1" or rec.get("source_id") != source_id:
            bad.append((key, "schema/source mismatch")); continue
        if key[0] not in SUPPORTED_TARGET_TYPES:
            bad.append((key, f"unsupported target_type {key[0]!r} (the Review Desk merges entry decisions only)")); continue
        if not isinstance(key[1], str) or not re.fullmatch(r"[0-9a-f]{64}", key[1]):
            bad.append((key, "invalid target_id")); continue
        if key not in queue_targets:
            bad.append((key, f"target is not in the {source_id} review queues")); continue
        if key in seen_in_export:
            bad.append((key, "duplicate target in export")); continue
        seen_in_export.add(key)
        if rec.get("review_status") not in VALID:
            bad.append((key, f"invalid status {rec.get('review_status')!r}")); continue
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]{1,39}", rec.get("reviewer_id") or ""):
            bad.append((key, "missing/invalid reviewer_id")); continue
        if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", rec.get("review_date") or ""):
            bad.append((key, "missing/invalid review_date")); continue
        if rec["review_status"] == "rejected_from_default_pack" and not (rec.get("review_notes") or rec.get("evidence")):
            bad.append((key, "rejected record needs notes or evidence")); continue
        repl = rec.get("replacement_metadata")
        if rec["review_status"] == "approved_with_metadata_change":
            if not isinstance(repl, dict) or not repl.get("display") or repl.get("normalized") != normalize_text(repl["display"]):
                bad.append((key, "approved_with_metadata_change needs consistent replacement_metadata")); continue
        elif repl is not None:
            bad.append((key, "replacement_metadata only allowed with approved_with_metadata_change")); continue
        out = canonical_decision(rec, source_id)
        if key in existing:
            if semantic_view(existing[key]) == semantic_view(out):
                identical.append(key)
            else:
                conflicts.append((key, describe_conflict(existing[key], out)))
            continue
        new.append(out)

    print(f"store: {store}\nexisting decisions: {len(existing)}\nqueue targets: {len(queue_targets)}\nexport records: {len(exp['decisions'])}")
    print(f"new: {len(new)}  identical-already-stored (no-op): {len(identical)}  conflicting-already-stored: {len(conflicts)}  invalid: {len(bad)}")
    for key, why in bad[:20]:
        print(f"  invalid {str(key[1])[:12]}…: {why}")
    for key, why in conflicts[:20]:
        print(f"  conflict {key[1][:12]}…: {why}")
    if bad or conflicts:
        assert store.read_bytes() == store_bytes
        die("nothing written: fix the invalid or conflicting records on the review site before merging")
    if not apply:
        print("dry run only; re-run with --apply to append"); return
    if not new:
        print("nothing to append; store unchanged"); return
    tmp = store.with_suffix(".jsonl.tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        text = store_bytes.decode("utf-8")
        f.write(text.rstrip("\n") + ("\n" if text.strip() else ""))
        for rec in new:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
    os.replace(tmp, store)
    print(f"appended {len(new)} decisions to {store}\nnext: cargo run -p kurmanci-data-builder -- validate-review-decisions {source_id}")


if __name__ == "__main__":
    main()
