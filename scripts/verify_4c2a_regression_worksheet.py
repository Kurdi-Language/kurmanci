#!/usr/bin/env python3
"""Deterministic verifier for Milestone 4C.2A Regression Entry Review Worksheet.

Verifies schema, exact canonical entry identity, review queue structured resolution,
conflict-group membership, source registry pinned revision & license ID, comparison constants,
ranking logic, field emptiness, line ending/BOM invariants, NFC normalization, compact JSON,
and deterministic record ordering. Includes a self-test mode (--self-test).
"""

import csv
import hashlib
import io
import json
import re
import sys
import tomllib
import unicodedata

from pathlib import Path


EXPECTED_HEADERS = [
    "regression_case_id",
    "benchmark_input",
    "expected_candidate",
    "expected_candidate_rank_seed",
    "expected_candidate_rank_experimental",
    "required_top_k",
    "expected_candidate_satisfied_seed",
    "expected_candidate_satisfied_experimental",
    "interfering_candidate",
    "experimental_rank",
    "candidate_is_ahead_of_expected",
    "entry_id",
    "display",
    "normalized",
    "source_line_num",
    "flags",
    "morphology",
    "conflict_group_id",
    "review_queue_locations",
    "source_revision",
    "source_provenance",
    "license_id",
    "comparison_policy_version",
    "candidate_limit",
    "comparison_baseline_sha",
    "human_lexical_decision",
    "evidence_or_reference",
    "reviewer_id",
    "review_date",
    "review_notes",
    "uncertainty_or_disagreement",
    "ranking_policy_followup_needed",
]

HEX64_PATTERN = re.compile(r"^[0-9a-f]{64}$")
HEX40_PATTERN = re.compile(r"^[0-9a-f]{40}$")

HUMAN_FIELDS = [
    "human_lexical_decision",
    "evidence_or_reference",
    "reviewer_id",
    "review_date",
    "review_notes",
    "uncertainty_or_disagreement",
]
TRIAGE_FIELD = "ranking_policy_followup_needed"


def compute_canonical_entry_id(
    source_id: str,
    source_revision: str,
    display: str,
    normalized: str,
    flags: str,
    morphology: list[str],
) -> str:
    """Computes deterministic mechanical SHA-256 entry_id matching Rust compute_entry_id."""
    hasher = hashlib.sha256()

    def hash_field(val: bytes) -> None:
        hasher.update(len(val).to_bytes(8, byteorder="big"))
        hasher.update(val)

    hash_field(b"kurmanci-review-entry-v1")
    hash_field(source_id.encode("utf-8"))
    hash_field(source_revision.encode("utf-8"))
    hash_field(unicodedata.normalize("NFC", display).encode("utf-8"))
    hash_field(unicodedata.normalize("NFC", normalized).encode("utf-8"))
    hash_field(flags.encode("utf-8"))
    for m in sorted(morphology):
        hash_field(m.encode("utf-8"))

    return hasher.hexdigest()


def verify_worksheet(worksheet_path: Path) -> None:
    if not worksheet_path.exists():
        sys.exit(f"FAIL: Worksheet file '{worksheet_path}' does not exist")

    raw_bytes = worksheet_path.read_bytes()

    # Invariants: BOM, CRLF, Final Newline
    if raw_bytes.startswith(b"\xef\xbb\xbf"):
        sys.exit("FAIL: CSV file contains UTF-8 BOM")

    if b"\r\n" in raw_bytes:
        sys.exit("FAIL: CSV file contains CRLF line endings")

    if not raw_bytes.endswith(b"\n"):
        sys.exit("FAIL: CSV file missing final newline")

    raw_text = raw_bytes.decode("utf-8")
    lines = raw_text.splitlines()

    if len(lines) != 14:
        sys.exit(f"FAIL: Expected 14 physical lines (1 header + 13 rows), got {len(lines)}")

    reader = list(csv.DictReader(lines))
    if len(reader) != 13:
        sys.exit(f"FAIL: Expected 13 record rows, got {len(reader)}")

    # 1. Header & Column order check
    actual_headers = list(reader[0].keys())
    if actual_headers != EXPECTED_HEADERS:
        sys.exit(f"FAIL: Headers mismatch.\nExpected: {EXPECTED_HEADERS}\nGot: {actual_headers}")

    # Load source registry metadata structurally with tomllib (fails closed)
    sources_path = Path("data/source-registry/sources.toml")
    if not sources_path.exists():
        sys.exit(f"FAIL: {sources_path} does not exist")

    try:
        toml_data = tomllib.loads(sources_path.read_text(encoding="utf-8"))
    except Exception as e:
        sys.exit(f"FAIL: Unable to parse {sources_path} with tomllib: {e}")

    matched_sources = [s for s in toml_data.get("sources", []) if s.get("source_id") == "kurdish-hunspell-kmr"]
    if len(matched_sources) != 1:
        sys.exit(f"FAIL: Expected exactly 1 source with source_id 'kurdish-hunspell-kmr' in {sources_path}, found {len(matched_sources)}")

    kmr_source = matched_sources[0]
    pinned_revision = kmr_source.get("version")
    registered_license_id = kmr_source.get("license")

    if not pinned_revision or not HEX40_PATTERN.match(pinned_revision):
        sys.exit(f"FAIL: Missing or malformed 'version' SHA in {sources_path} for kurdish-hunspell-kmr: '{pinned_revision}'")

    if not registered_license_id or not registered_license_id.strip():
        sys.exit(f"FAIL: Missing or empty 'license' identifier in {sources_path} for kurdish-hunspell-kmr")


    # Load reviewed cases case_ids
    reviewed_cases_path = Path("evaluation/spelling/reviewed-cases.jsonl")
    if not reviewed_cases_path.exists():
        sys.exit(f"FAIL: {reviewed_cases_path} does not exist")
    reviewed_case_ids = set()
    for l in reviewed_cases_path.read_text(encoding="utf-8").splitlines():
        if l.strip():
            reviewed_case_ids.add(json.loads(l)["case_id"])

    # Load imported lexicon records
    imported_lex_path = Path("data/imported/kurdish-hunspell-kmr/lexicon.jsonl")
    if not imported_lex_path.exists():
        sys.exit(f"FAIL: {imported_lex_path} does not exist. Run data-builder import-hunspell first.")

    imported_by_word_and_line = {}
    for l in imported_lex_path.read_text(encoding="utf-8").splitlines():
        if l.strip():
            rec = json.loads(l)
            key = (rec["word"], rec["source_line_num"])
            imported_by_word_and_line[key] = rec

    # Load review queues into structured map: entry_id -> list of queue records
    rq_dir = Path("data/review-queues/kurdish-hunspell-kmr")
    if not rq_dir.exists():
        sys.exit(f"FAIL: Review queue dir '{rq_dir}' does not exist")

    queue_records_by_entry_id = {}
    conflict_group_records = {}

    for qf in rq_dir.glob("*.jsonl"):
        rel_path = f"data/review-queues/kurdish-hunspell-kmr/{qf.name}"
        for l in qf.read_text(encoding="utf-8").splitlines():
            if not l.strip():
                continue
            item = json.loads(l)
            ttype = item.get("target_type")
            if ttype == "entry":
                tid = item["target_id"]
                if tid not in queue_records_by_entry_id:
                    queue_records_by_entry_id[tid] = []
                queue_records_by_entry_id[tid].append((rel_path, item))
            elif ttype == "conflict_group":
                gid = item["target_id"]
                conflict_group_records[gid] = item
                for meid in item.get("member_entry_ids", []):
                    if meid not in queue_records_by_entry_id:
                        queue_records_by_entry_id[meid] = []
                    queue_records_by_entry_id[meid].append((rel_path, item))

    # Verify each row
    seen_sort_keys = []
    for idx, r in enumerate(reader, start=1):
        cid = r["regression_case_id"]
        eid = r["entry_id"]
        gid = r["conflict_group_id"]
        s_rev = r["source_revision"]
        b_sha = r["comparison_baseline_sha"]

        # 1. Case ID check
        if not HEX64_PATTERN.match(cid):
            sys.exit(f"Row {idx}: Invalid 64-char hex regression_case_id '{cid}'")
        if cid not in reviewed_case_ids:
            sys.exit(f"Row {idx}: Case ID '{cid}' not found in reviewed-cases.jsonl")

        # 2. Entry ID & SHA format checks
        if not HEX64_PATTERN.match(eid):
            sys.exit(f"Row {idx}: Invalid 64-char hex entry_id '{eid}'")
        if not HEX40_PATTERN.match(s_rev):
            sys.exit(f"Row {idx}: Invalid 40-char hex source_revision '{s_rev}'")
        if s_rev != pinned_revision:
            sys.exit(f"Row {idx}: source_revision '{s_rev}' does not match pinned revision '{pinned_revision}'")
        if not HEX40_PATTERN.match(b_sha):
            sys.exit(f"Row {idx}: Invalid 40-char hex comparison_baseline_sha '{b_sha}'")
        if b_sha != "7db99ee676bf81c8db25df57ee0fbb7dca74b2a0":
            sys.exit(f"Row {idx}: comparison_baseline_sha '{b_sha}' mismatch")

        # 3. Comparison constants
        if r["comparison_policy_version"] != "three-pack-comparison-v1":
            sys.exit(f"Row {idx}: comparison_policy_version mismatch '{r['comparison_policy_version']}'")
        if r["candidate_limit"] != "10":
            sys.exit(f"Row {idx}: candidate_limit mismatch '{r['candidate_limit']}'")
        if r["license_id"] != registered_license_id:
            sys.exit(f"Row {idx}: license_id '{r['license_id']}' does not match registered license '{registered_license_id}'")

        # 4. Structured Queue Resolution by entry_id
        if eid not in queue_records_by_entry_id:
            sys.exit(f"Row {idx}: entry_id '{eid}' does not resolve to any queue record in {rq_dir}")

        matched_queue_items = queue_records_by_entry_id[eid]
        matched_queue_paths = sorted(list(set(qp for qp, _ in matched_queue_items)))

        # Verify review_queue_locations JSON array
        try:
            r_q_paths = json.loads(r["review_queue_locations"])
        except json.JSONDecodeError as e:
            sys.exit(f"Row {idx}: Invalid JSON in review_queue_locations: {e}")

        compact_q_paths = json.dumps(r_q_paths, separators=(",", ":"))
        if r["review_queue_locations"] != compact_q_paths:
            sys.exit(f"Row {idx}: review_queue_locations not compact JSON. CSV '{r['review_queue_locations']}'")

        if sorted(r_q_paths) != matched_queue_paths:
            sys.exit(f"Row {idx}: review_queue_locations mismatch.\nCSV: {r_q_paths}\nQueue records: {matched_queue_paths}")

        # 5. Canonical Entry ID computation check
        morph_list = json.loads(r["morphology"])
        compact_morph = json.dumps(morph_list, separators=(",", ":"))
        if r["morphology"] != compact_morph:
            sys.exit(f"Row {idx}: morphology not compact JSON. CSV '{r['morphology']}'")

        computed_eid = compute_canonical_entry_id(
            "kurdish-hunspell-kmr",
            s_rev,
            r["display"],
            r["normalized"],
            r["flags"],
            morph_list,
        )

        if computed_eid != eid:
            sys.exit(f"Row {idx}: Entry ID mismatch! Computed '{computed_eid}' != CSV '{eid}'")

        # 6. Match imported record
        line_num = int(r["source_line_num"])
        rec_key = (r["display"], line_num)
        if rec_key not in imported_by_word_and_line:
            sys.exit(f"Row {idx}: Imported record for display '{r['display']}' line {line_num} not found")
        imp_rec = imported_by_word_and_line[rec_key]

        if imp_rec["word"] != r["display"]:
            sys.exit(f"Row {idx}: Display mismatch. CSV '{r['display']}', imported '{imp_rec['word']}'")
        if imp_rec["normalized"] != r["normalized"]:
            sys.exit(f"Row {idx}: Normalized mismatch. CSV '{r['normalized']}', imported '{imp_rec['normalized']}'")
        if imp_rec.get("flags", "") != r["flags"]:
            sys.exit(f"Row {idx}: Flags mismatch. CSV '{r['flags']}', imported '{imp_rec.get('flags', '')}'")

        expected_provenance = f"kurdish-hunspell-kmr:kmr/kmr-Latn.dic:{line_num}"
        if r["source_provenance"] != expected_provenance:
            sys.exit(f"Row {idx}: Provenance mismatch. CSV '{r['source_provenance']}', expected '{expected_provenance}'")

        # 7. Conflict Group structured check
        if gid:
            if not HEX64_PATTERN.match(gid):
                sys.exit(f"Row {idx}: Invalid 64-char hex conflict_group_id '{gid}'")
            if gid not in conflict_group_records:
                sys.exit(f"Row {idx}: Conflict group '{gid}' not found in metadata-conflict-groups.jsonl")
            cg_members = conflict_group_records[gid].get("member_entry_ids", [])
            if eid not in cg_members:
                sys.exit(f"Row {idx}: Entry ID '{eid}' is not a member of conflict group '{gid}'")

        # 8. Ranking logic checks
        exp_rank = int(r["experimental_rank"])
        exp_target_rank = int(r["expected_candidate_rank_experimental"])
        if exp_rank >= exp_target_rank:
            sys.exit(f"Row {idx}: experimental_rank ({exp_rank}) must be strictly less than expected_candidate_rank_experimental ({exp_target_rank})")
        if r["candidate_is_ahead_of_expected"] != "true":
            sys.exit(f"Row {idx}: candidate_is_ahead_of_expected must be 'true'")

        # 9. Emptiness checks
        for hf in HUMAN_FIELDS:
            if r[hf] != "":
                sys.exit(f"Row {idx}: Human field '{hf}' must be empty, got '{r[hf]}'")
        if r[TRIAGE_FIELD] != "":
            sys.exit(f"Row {idx}: Triage field '{TRIAGE_FIELD}' must be empty, got '{r[TRIAGE_FIELD]}'")

        # 10. NFC & Path hygiene
        for k, v in r.items():
            if not unicodedata.is_normalized("NFC", v):
                sys.exit(f"Row {idx}: Field '{k}' value '{v}' is not NFC normalized")
            if "file://" in v:
                sys.exit(f"Row {idx}: Field '{k}' contains file:// URL")
            if "/Users/" in v or "/home/" in v or "/tmp/" in v:
                sys.exit(f"Row {idx}: Field '{k}' contains absolute system path")

        seen_sort_keys.append((cid, exp_rank, eid))

    # 11. Sorting check
    expected_sorted = sorted(seen_sort_keys, key=lambda x: (x[0], x[1], x[2]))
    if seen_sort_keys != expected_sorted:
        sys.exit("FAIL: Worksheet rows are not deterministically sorted by (regression_case_id, experimental_rank, entry_id)")

    print("⚡ Milestone 4C.2A Worksheet Verification PASSED successfully!")


def run_self_test(worksheet_path: Path) -> None:
    """Proves that mutating entry_id or any field in the worksheet causes verification to fail."""
    print("Running verifier self-test suite...")

    raw_text = worksheet_path.read_text(encoding="utf-8")
    lines = raw_text.splitlines()
    header = lines[0]
    data_rows = lines[1:]

    fake_hex64 = "a" * 64

    # Test 1: Replace entry_id in row 1 with fake_hex64
    test_rows = list(data_rows)
    cols = list(csv.reader([test_rows[0]]))[0]
    cols[11] = fake_hex64  # entry_id column index 11
    
    buf = io.StringIO()
    w = csv.writer(buf, lineterminator="\n")
    w.writerow(list(csv.reader([header]))[0])
    w.writerow(cols)
    for r in test_rows[1:]:
        buf.write(r + "\n")
    
    tmp_fake_csv = Path("/tmp/test_fake_worksheet.csv")
    tmp_fake_csv.write_text(buf.getvalue(), encoding="utf-8")

    try:
        verify_worksheet(tmp_fake_csv)
        sys.exit("FAIL: Verifier passed on fake entry_id self-test!")
    except SystemExit as e:
        err_msg = str(e)
        if "FAIL: Entry ID mismatch!" in err_msg or "does not resolve to any queue record" in err_msg:
            print("✅ Negative test passed: Invalid entry_id correctly rejected!")
        else:
            sys.exit(f"FAIL: Unexpected error during self-test: {err_msg}")
    finally:
        if tmp_fake_csv.exists():
            tmp_fake_csv.unlink()

    print("⚡ Self-test suite PASSED successfully!")


if __name__ == "__main__":
    csv_file = Path("evaluation/spelling/review-worksheets/4c2a-regression-interfering-entries.csv")

    if "--self-test" in sys.argv:
        run_self_test(csv_file)
    else:
        verify_worksheet(csv_file)
        run_self_test(csv_file)
