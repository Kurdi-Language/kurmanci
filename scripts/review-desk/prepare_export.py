#!/usr/bin/env python3
"""Apply the default-pack alphabet policy to a Review Desk export before merging.

The Review Desk queue `hunspell-kuwiki-001` was built before the alphabet policy (#67)
existed, so reviewers could approve an entry that the authoritative review infrastructure
now classifies as outside the 31-letter Kurmancî alphabet. The Rust resolver refuses such
an approval (fail closed), so this step rewrites exactly those records, mechanically and
transparently, to `rejected_from_default_pack` with a note that names the policy, the
offending characters and the reviewer's original decision. Every other record is passed
through unchanged. Pending entries are not in the export and stay pending.

The policy is not defined here. An entry is "outside the alphabet" exactly when the
authoritative review-queue generator (data-builder, `default_pack_eligibility`) put it in
data/review-queues/<source>/alphabet-policy-excluded.jsonl; that artifact also carries the
offending code points in its reason codes. Only a plain `approved` decision is rewritten:
an `approved_with_metadata_change` decision is the human's correction of the source form,
its replacement form is what the authoritative Rust selector evaluates
(`default_pack_eligibility` on `replacement_metadata.normalized`), so it is passed through
exactly as reviewed and the Rust validation/resolution path fails closed if the replacement
violates the policy. No replacement eligibility is decided in Python.

No linguistic decision is made here: the rewrite implements the project owner's explicit
alphabet policy (docs/lexicon-review.md), the same correction applied to the 13 Kuwiki
approvals in #67.

The original Review Desk choice stays recoverable in machine-readable form: a rewritten
record carries an `evidence` entry `review-desk-original:status=<status>;reviewer=<id>;
date=<date>;queue=<queue_id>;policy=alphabet-2026-09-17` next to its note, and
`--audit-json` writes every rewrite (target, display, original and new status, characters).

Usage:
  python3 scripts/review-desk/prepare_export.py <export.json> <prepared.json> [--report report.md] [--audit-json audit.json] [--root DIR]

Then:
  python3 scripts/review-desk/merge_review_desk_export.py <prepared.json> [--apply]
"""
import json, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
if "--root" in sys.argv:
    ROOT = Path(sys.argv[sys.argv.index("--root") + 1]).resolve()
# Only a plain approval of an authoritatively excluded source entry is converted; a
# metadata change is judged on its replacement form by the Rust resolver.
MECHANICALLY_CONVERTED = {"approved"}
EXCLUDED_QUEUE = "alphabet-policy-excluded.jsonl"


def load_queue_classification(source_id):
    """target_id -> (display, normalized, outside_characters or None), from the authoritative
    review-queue artifacts; `outside_characters` is set only for entries the generator wrote
    to alphabet-policy-excluded.jsonl (its reason codes carry the code points)."""
    by_target = {}
    qdir = ROOT / "data/review-queues" / source_id
    for path in sorted(qdir.glob("*.jsonl")):
        if path.name == "metadata-conflict-groups.jsonl":
            continue
        excluded_file = path.name == EXCLUDED_QUEUE
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.strip():
                rec = json.loads(line)
                display, normalized, outside = by_target.get(rec["target_id"], (rec["display"], rec["normalized"], None))
                if excluded_file:
                    codes = [c for c in rec.get("reason_codes", []) if "U+" in c]
                    outside = codes[0] if codes else "characters outside the alphabet (see alphabet-policy-excluded.jsonl)"
                by_target[rec["target_id"]] = (display, normalized, outside)
    return by_target


def main():
    argv = sys.argv[1:]
    args = [a for i, a in enumerate(argv) if not a.startswith("--") and (i == 0 or argv[i - 1] not in ("--report", "--audit-json", "--root"))]
    if len(args) < 2:
        raise SystemExit(__doc__)
    export_path, out_path = Path(args[0]), Path(args[1])
    report_path = Path(argv[argv.index("--report") + 1]) if "--report" in argv else None
    audit_path = Path(argv[argv.index("--audit-json") + 1]) if "--audit-json" in argv else None
    exp = json.loads(export_path.read_text(encoding="utf-8"))
    if exp.get("schema_version") != "review-desk-export-v1":
        raise SystemExit("ERROR: not a review-desk-export-v1 file")
    source_id = exp["source_id"]
    by_target = load_queue_classification(source_id)
    # The rewrite is dated by the export it was applied to (deterministic; no wall clock).
    today = str(exp.get("exported_at") or "")[:10]
    if not (len(today) == 10 and today[4] == "-" and today[7] == "-"):
        raise SystemExit("ERROR: export lacks a valid exported_at date")

    counts = {"passed_through": 0, "policy_rejected": 0, "unknown_target": 0}
    by_status = {}
    rewritten = []
    audit_rows = []
    out_decisions = []
    for rec in exp["decisions"]:
        status = rec.get("review_status")
        by_status[status] = by_status.get(status, 0) + 1
        known = by_target.get(rec.get("target_id"))
        if known is None:
            counts["unknown_target"] += 1
            out_decisions.append(rec)
            continue
        display, normalized, bad = known
        # approved_with_metadata_change passes through untouched: its replacement form is
        # classified by the authoritative Rust validation/resolution, which fails closed.
        if status in MECHANICALLY_CONVERTED and bad:
            new = dict(rec)
            new["review_status"] = "rejected_from_default_pack"
            new["review_date"] = today
            new["review_notes"] = (
                f"Rejected from the default pack under the explicit production alphabet policy "
                f"(project owner, 2026-09-17): the review infrastructure classifies this entry as "
                f"outside the 31-letter Kurmancî alphabet ({bad}). Mechanical policy application, "
                f"not a new lexical judgement. Reviewer's decision on the Review Desk: {status} "
                f"({rec.get('reviewer_id')}, {rec.get('review_date')}); source evidence retained."
            )
            new["evidence"] = list(rec.get("evidence") or []) + [
                f"review-desk-original:status={status};reviewer={rec.get('reviewer_id')};date={rec.get('review_date')};queue={exp.get('queue_id')};policy=alphabet-2026-09-17"
            ]
            counts["policy_rejected"] += 1
            rewritten.append((display, normalized, status, rec.get("reviewer_id"), rec.get("review_date"), bad))
            audit_rows.append({
                "target_id": rec.get("target_id"),
                "display": display,
                "normalized": normalized,
                "review_desk_status": status,
                "review_desk_reviewer": rec.get("reviewer_id"),
                "review_desk_date": rec.get("review_date"),
                "review_desk_notes": rec.get("review_notes"),
                "prepared_status": "rejected_from_default_pack",
                "policy": "default-pack alphabet policy (project owner, 2026-09-17)",
                "outside_characters": bad,
            })
            out_decisions.append(new)
        else:
            counts["passed_through"] += 1
            out_decisions.append(rec)

    prepared = dict(exp)
    prepared["decisions"] = out_decisions
    prepared["prepared_by"] = "scripts/review-desk/prepare_export.py (alphabet policy applied from alphabet-policy-excluded.jsonl)"
    out_path.write_text(json.dumps(prepared, ensure_ascii=False, indent=1), encoding="utf-8")

    lines = [
        f"# Review Desk export preparation ({exp.get('queue_id')})",
        "",
        f"- exported: {exp.get('exported_at')}",
        f"- decisions in export: {len(exp['decisions'])}",
        f"- by status as exported: {json.dumps(by_status, ensure_ascii=False)}",
        f"- passed through unchanged: {counts['passed_through']}",
        f"- approved but classified outside the alphabet by the review infrastructure, rewritten to rejected_from_default_pack: {counts['policy_rejected']}",
        f"- unknown target ids (passed through for the merge script to refuse): {counts['unknown_target']}",
        "",
    ]
    if rewritten:
        lines += ["| Display | Normalized | Desk decision | Reviewer | Date | Characters |", "|---|---|---|---|---|---|"]
        for d, n, s, r, dt, ch in rewritten:
            lines.append(f"| `{d}` | `{n}` | {s} | {r} | {dt} | {ch} |")
    text = "\n".join(lines) + "\n"
    sys.stderr.write(text)
    if report_path:
        report_path.write_text(text, encoding="utf-8")
    if audit_path:
        audit_path.write_text(json.dumps({
            "schema_version": "review-desk-prepare-audit-v1",
            "queue_id": exp.get("queue_id"),
            "source_id": source_id,
            "exported_at": exp.get("exported_at"),
            "counts": counts,
            "rewritten": audit_rows,
        }, ensure_ascii=False, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
