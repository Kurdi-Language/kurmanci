#!/usr/bin/env python3
"""Deterministic verifier for Milestone 4C.2B Human Lexical Review Guide.

Verifies:
1. File invariants (UTF-8, no BOM, LF-only, final newline).
2. Path hygiene (no file:// URLs or local absolute paths).
3. Rust schema alignment by extracting enums directly from data-builder/src/review/schema.rs.
4. Structural Markdown reference table parsing matching canonical CSV field-by-field.
5. JSON example block safety (placeholders only, no concrete dates or canonical IDs).
"""

import csv
import json
import re
import sys
import unicodedata
from pathlib import Path

HEX64_PATTERN = re.compile(r"\b[0-9a-f]{64}\b")
CONCRETE_DATE_PATTERN = re.compile(r"\b20[0-9]{2}-[0-1][0-9]-[0-3][0-9]\b")


def to_snake_case(s: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", s).lower()


def load_rust_schema_enums(schema_path: Path) -> tuple[set[str], set[str], set[str]]:
    if not schema_path.exists():
        sys.exit(f"FAIL: Rust schema file '{schema_path}' does not exist")

    schema_text = schema_path.read_text(encoding="utf-8")

    # 1. ReviewDecisionStatus
    m_status = re.search(r"pub enum ReviewDecisionStatus \{(.*?)\}", schema_text, re.DOTALL)
    if not m_status:
        sys.exit("FAIL: Unable to locate ReviewDecisionStatus enum in schema.rs")
    status_variants = set(
        to_snake_case(v.strip())
        for v in m_status.group(1).split(",")
        if v.strip() and not v.strip().startswith("//")
    )

    # 2. ReviewTargetType
    m_target = re.search(r"pub enum ReviewTargetType \{(.*?)\}", schema_text, re.DOTALL)
    if not m_target:
        sys.exit("FAIL: Unable to locate ReviewTargetType enum in schema.rs")

    target_types = set()
    for v in m_target.group(1).split(","):
        v_clean = v.strip().split("{")[0].strip()
        if v_clean and not v_clean.startswith("//"):
            # Handle serde rename if present (e.g. rename_all = "snake_case")
            target_types.add(to_snake_case(v_clean))

    # 3. GroupResolution
    m_group = re.search(r"pub enum GroupResolution \{(.*?)\}\n\n", schema_text, re.DOTALL)
    if not m_group:
        sys.exit("FAIL: Unable to locate GroupResolution enum in schema.rs")
    group_resolutions = set(
        to_snake_case(m.group(1))
        for m in re.finditer(r"([A-Z][a-zA-Z0-9]+)\s*\{", m_group.group(1))
    )

    return status_variants, target_types, group_resolutions


def verify_review_guide(guide_path: Path, csv_path: Path, schema_path: Path) -> None:
    if not guide_path.exists():
        sys.exit(f"FAIL: Guide file '{guide_path}' does not exist")
    if not csv_path.exists():
        sys.exit(f"FAIL: CSV worksheet '{csv_path}' does not exist")

    raw_bytes = guide_path.read_bytes()

    # 1. Line ending & BOM invariants
    if raw_bytes.startswith(b"\xef\xbb\xbf"):
        sys.exit("FAIL: Guide contains UTF-8 BOM")
    if b"\r\n" in raw_bytes:
        sys.exit("FAIL: Guide contains CRLF line endings")
    if not raw_bytes.endswith(b"\n"):
        sys.exit("FAIL: Guide missing final newline")

    text = raw_bytes.decode("utf-8")

    # 2. Path hygiene
    if "file://" in text:
        sys.exit("FAIL: Guide contains file:// URL")
    if "/Users/" in text or "/home/" in text or "/tmp/" in text:
        sys.exit("FAIL: Guide contains local absolute path")

    # 3. Verify Rust schema alignment
    rust_statuses, rust_targets, rust_resolutions = load_rust_schema_enums(schema_path)

    for st in rust_statuses:
        if f"`{st}`" not in text:
            sys.exit(f"FAIL: Guide does not document Rust ReviewDecisionStatus variant `{st}`")

    for tt in rust_targets:
        if f'"{tt}"' not in text:
            sys.exit(f"FAIL: Guide does not document Rust ReviewTargetType variant '{tt}'")

    for gr in rust_resolutions:
        if gr not in text:
            sys.exit(f"FAIL: Guide does not document Rust GroupResolution variant '{gr}'")

    # 4. Load CSV entry IDs & metadata map
    csv_rows = list(csv.DictReader(csv_path.read_text(encoding="utf-8").splitlines()))
    if len(csv_rows) != 13:
        sys.exit(f"FAIL: CSV worksheet expected 13 rows, got {len(csv_rows)}")

    csv_map_by_eid = {}
    for r in csv_rows:
        eid = r["entry_id"]
        csv_map_by_eid[eid] = r

    csv_entry_ids = set(csv_map_by_eid.keys())
    csv_conflict_gids = set(r["conflict_group_id"] for r in csv_rows if r["conflict_group_id"])

    # 5. Parse Markdown Compact Reference Table structurally
    table_lines = [
        line.strip()
        for line in text.splitlines()
        if line.strip().startswith("|") and not line.strip().startswith("| :---") and not line.strip().startswith("| Benchmark Input")
    ]

    if len(table_lines) != 13:
        sys.exit(f"FAIL: Compact reference table expected exactly 13 data rows, found {len(table_lines)}")

    parsed_table_eids = set()

    for idx, tline in enumerate(table_lines, start=1):
        cols = [c.strip().strip("`") for c in tline.split("|")[1:-1]]
        if len(cols) != 6:
            sys.exit(f"Table Row {idx}: Expected 6 columns, got {len(cols)} in '{tline}'")

        t_input, t_display, t_norm, t_eid, t_line_str, t_gid_str = cols

        if t_eid in parsed_table_eids:
            sys.exit(f"Table Row {idx}: Duplicate entry_id '{t_eid}' in reference table")
        parsed_table_eids.add(t_eid)

        if t_eid not in csv_map_by_eid:
            sys.exit(f"Table Row {idx}: Entry ID '{t_eid}' not found in canonical CSV")

        csv_rec = csv_map_by_eid[t_eid]

        if t_input != csv_rec["benchmark_input"]:
            sys.exit(f"Table Row {idx} ({t_eid}): input '{t_input}' != CSV '{csv_rec['benchmark_input']}'")
        if t_display != csv_rec["display"]:
            sys.exit(f"Table Row {idx} ({t_eid}): display '{t_display}' != CSV '{csv_rec['display']}'")
        if t_norm != csv_rec["normalized"]:
            sys.exit(f"Table Row {idx} ({t_eid}): normalized '{t_norm}' != CSV '{csv_rec['normalized']}'")
        if int(t_line_str) != int(csv_rec["source_line_num"]):
            sys.exit(f"Table Row {idx} ({t_eid}): line '{t_line_str}' != CSV '{csv_rec['source_line_num']}'")

        expected_gid_str = csv_rec["conflict_group_id"] if csv_rec["conflict_group_id"] else "*None*"
        if t_gid_str != expected_gid_str and t_gid_str != csv_rec["conflict_group_id"]:
            sys.exit(f"Table Row {idx} ({t_eid}): conflict_group_id '{t_gid_str}' != CSV '{expected_gid_str}'")

    if parsed_table_eids != csv_entry_ids:
        sys.exit(f"FAIL: Table entry IDs mismatch CSV entry IDs.\nMissing: {csv_entry_ids - parsed_table_eids}")

    # 6. JSON Example Blocks & Placeholder Hygiene
    json_blocks = re.findall(r"```json(.*?)```", text, re.DOTALL)
    if not json_blocks:
        sys.exit("FAIL: Guide missing structural JSON example blocks")

    for b_idx, block in enumerate(json_blocks, start=1):
        # Check no canonical entry ID or conflict group ID appears in JSON example blocks
        for eid in csv_entry_ids:
            if eid in block:
                sys.exit(f"JSON Block {b_idx}: Canonical entry ID '{eid}' must not appear in example blocks")
        for gid in csv_conflict_gids:
            if gid in block:
                sys.exit(f"JSON Block {b_idx}: Canonical conflict group ID '{gid}' must not appear in example blocks")

        # Verify placeholders exist
        if "<" not in block or ">" not in block:
            sys.exit(f"JSON Block {b_idx}: Must use explicit <placeholder> markers")

    # 7. Check no concrete dates anywhere in document
    if CONCRETE_DATE_PATTERN.search(text):
        sys.exit("FAIL: Guide contains concrete date (YYYY-MM-DD) instead of placeholder")

    # 8. Triage & Staging checks
    if "ranking_policy_followup_needed" not in text:
        sys.exit("FAIL: Guide missing ranking_policy_followup_needed operational documentation")

    print("⚡ Milestone 4C.2B Review Guide Verification PASSED successfully!")


if __name__ == "__main__":
    g_file = Path("docs/evaluation/4c2b-human-lexical-review-guide.md")
    c_file = Path("evaluation/spelling/review-worksheets/4c2a-regression-interfering-entries.csv")
    s_file = Path("data-builder/src/review/schema.rs")
    verify_review_guide(g_file, c_file, s_file)
