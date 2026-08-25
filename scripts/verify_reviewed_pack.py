#!/usr/bin/env python3
"""
Kurmancî Reviewed-Pack Rebuild & Determinism Verifier

This script is a read-only verifier for reviewed pack rebuilds. It:
1. Dynamically parses review decisions, imported records, and conflict groups by target_id.
2. Decodes actual binary pack entries from seed and reviewed lexicon.bin files.
3. Computes external_reviewed_additions = reviewed_entries - seed_entries and verifies exact content invariants.
4. Asserts that sê, all non-approved entries, and all members of sê's conflict group are absent from reviewed.
5. Cross-checks computed additions against manifest counts.
6. Optionally compares two build output trees for byte determinism (--compare-root-a / --compare-root-b).
7. Runs an extended self-test suite against isolated mutated temporary fixtures (--self-test).
"""

import sys
import json
import argparse
import tempfile
import shutil
from pathlib import Path
from typing import Dict, Set, Any, List, Tuple

SE_TARGET_ID = "09b9615e18bca7b64270de5df9ca439415f4fec996f01f07bf7676ee8655be8e"
SE_GROUP_ID = "ce6da264f7c9c318282532121fbdd7cf300ee1273f7549e555518525d35e838f"


def parse_args():
    parser = argparse.ArgumentParser(description="Verify Reviewed Pack Rebuild & Invariants")
    parser.add_argument("--candidate-root", type=str, default=".", help="Path to workspace root to verify")
    parser.add_argument("--compare-root-a", type=str, help="Path to Pass 1 output root for tree comparison")
    parser.add_argument("--compare-root-b", type=str, help="Path to Pass 2 output root for tree comparison")
    parser.add_argument("--self-test", action="store_true", help="Run verifier self-test suite against mutated fixtures")
    return parser.parse_args()


def load_decisions(decisions_path: Path) -> List[Dict[str, Any]]:
    if not decisions_path.exists():
        raise FileNotFoundError(f"Decisions file not found: {decisions_path}")
    lines = decisions_path.read_text(encoding="utf-8").splitlines()
    records = []
    for line_num, line in enumerate(lines, 1):
        if not line.strip():
            continue
        try:
            records.append(json.loads(line))
        except Exception as e:
            raise ValueError(f"Line {line_num} in {decisions_path} is invalid JSON: {e}")
    return records


def load_pack_manifest(manifest_path: Path) -> Dict[str, Any]:
    if not manifest_path.exists():
        raise FileNotFoundError(f"Pack manifest not found: {manifest_path}")
    return json.loads(manifest_path.read_text(encoding="utf-8"))


def decode_bin_entries(bin_path: Path) -> Set[str]:
    if not bin_path.exists():
        raise FileNotFoundError(f"Binary pack not found: {bin_path}")
    data = bin_path.read_bytes()
    cursor = 8

    def read_str(c):
        l = int.from_bytes(data[c:c+2], "little")
        s = data[c+2:c+2+l].decode("utf-8")
        return s, c + 2 + l

    lang_tag, cursor = read_str(cursor)
    count = int.from_bytes(data[cursor:cursor+4], "little")
    cursor += 4
    payload_len = int.from_bytes(data[cursor:cursor+8], "little")
    cursor += 8 + 32  # payload_len + checksum

    entries = set()
    for _ in range(count):
        word, cursor = read_str(cursor)
        lemma, cursor = read_str(cursor)
        norm, cursor = read_str(cursor)
        pos, cursor = read_str(cursor)
        freq = int.from_bytes(data[cursor:cursor+8], "little")
        cursor += 8
        status, cursor = read_str(cursor)

        r_count = int.from_bytes(data[cursor:cursor+2], "little")
        cursor += 2
        for _ in range(r_count):
            _, cursor = read_str(cursor)

        s_count = int.from_bytes(data[cursor:cursor+2], "little")
        cursor += 2
        for _ in range(s_count):
            _, cursor = read_str(cursor)

        cursor += 20  # token_count (8) + document_count (8) + zipf_milli (4)
        entries.add(norm)
    return entries


def load_target_mappings(candidate_root: Path) -> Tuple[Dict[str, str], Dict[str, List[str]]]:
    queues_dir = candidate_root / "data/review-queues/kurdish-hunspell-kmr"
    target_to_norm = {}
    conflict_groups = {}
    if queues_dir.exists():
        for qfile in queues_dir.glob("*.jsonl"):
            for line in qfile.read_text(encoding="utf-8").splitlines():
                if not line.strip():
                    continue
                rec = json.loads(line)
                t_id = rec.get("target_id")
                norm = rec.get("normalized") or rec.get("display")
                if t_id and norm:
                    target_to_norm[t_id] = norm
                if "member_entry_ids" in rec:
                    conflict_groups[rec["target_id"]] = rec["member_entry_ids"]
    return target_to_norm, conflict_groups


def derive_selection(candidate_root: Path) -> Dict[str, Any]:
    dec_path = candidate_root / "data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl"
    seed_bin_path = candidate_root / "data/build/packs/seed/lexicon.bin"
    rev_bin_path = candidate_root / "data/build/packs/reviewed/lexicon.bin"
    rev_manifest_path = candidate_root / "data/build/packs/reviewed/manifest.json"

    decisions = load_decisions(dec_path)
    seed_entries = decode_bin_entries(seed_bin_path)
    rev_entries = decode_bin_entries(rev_bin_path)
    rev_manifest = load_pack_manifest(rev_manifest_path)
    target_to_norm, conflict_groups = load_target_mappings(candidate_root)

    approved_decs = [d for d in decisions if d["review_status"] == "approved"]
    non_approved_decs = [d for d in decisions if d["review_status"] != "approved"]

    external_additions = rev_entries - seed_entries

    unresolved_target_ids = set(conflict_groups.get(SE_GROUP_ID, [])) | {SE_TARGET_ID}
    expected_external_additions = {
        target_to_norm[d["target_id"]]
        for d in approved_decs
        if d["target_id"] in target_to_norm
        and d["target_id"] not in unresolved_target_ids
        and target_to_norm[d["target_id"]] not in seed_entries
    }

    return {
        "decisions": decisions,
        "approved_decisions": approved_decs,
        "non_approved_decisions": non_approved_decs,
        "seed_entries": seed_entries,
        "rev_entries": rev_entries,
        "rev_manifest": rev_manifest,
        "external_additions": external_additions,
        "expected_external_additions": expected_external_additions,
        "target_to_norm": target_to_norm,
        "conflict_groups": conflict_groups,
    }


def validate_policy_invariants(derived: Dict[str, Any], candidate_root: Path):
    decisions = derived["decisions"]
    approved_decs = derived["approved_decisions"]
    non_approved_decs = derived["non_approved_decisions"]
    rev_manifest = derived["rev_manifest"]
    ext_additions = derived["external_additions"]
    target_to_norm = derived["target_to_norm"]
    conflict_groups = derived["conflict_groups"]

    if len(decisions) == 0:
        raise AssertionError("Review decisions store is empty")
    if len(decisions) != len(approved_decs) + len(non_approved_decs):
        raise AssertionError(f"Total decisions ({len(decisions)}) != approved ({len(approved_decs)}) + non-approved ({len(non_approved_decs)})")

    # Check manifest selection count matches computed additions count
    manifest_ext_approved = rev_manifest.get("external_approved_selected_count", 0) - rev_manifest.get("external_discarded_by_collision_count", 0)
    if manifest_ext_approved != len(ext_additions):
        raise AssertionError(f"Computed external additions count ({len(ext_additions)}) != manifest count ({manifest_ext_approved})")
    se_dec = next((d for d in decisions if d["target_id"] == SE_TARGET_ID), None)
    if not se_dec:
        raise AssertionError(f"Decision for sê target_id {SE_TARGET_ID} not found")
    if se_dec["target_type"] != "entry":
        raise AssertionError("sê decision target_type must be 'entry'")
    if se_dec["review_status"] != "approved":
        raise AssertionError("sê decision review_status must be 'approved'")

    # Verify sê is absent from external_additions (and also not added over seed)
    se_norm = target_to_norm.get(SE_TARGET_ID, "sê")
    if se_norm in ext_additions:
        raise AssertionError(f"Approved sê entry ('{se_norm}') must not be included in external reviewed additions while conflict group is unresolved")

    # Verify all members of sê's unresolved conflict group are absent from external_additions
    se_group_members = conflict_groups.get(SE_GROUP_ID, [])
    if not se_group_members:
        raise AssertionError(f"Conflict group {SE_GROUP_ID} not found in metadata conflict groups")
    for m_id in se_group_members:
        m_norm = target_to_norm.get(m_id)
        if m_norm and m_norm in ext_additions:
            raise AssertionError(f"Conflict group member '{m_norm}' (id {m_id}) must be absent from external reviewed additions")

    # Verify all non-approved decision targets are absent from external_additions
    for d in non_approved_decs:
        t_id = d["target_id"]
        t_norm = target_to_norm.get(t_id)
        if t_norm and t_norm in ext_additions:
            raise AssertionError(f"Non-approved entry '{t_norm}' (id {t_id}) must be absent from external reviewed additions")

    print("⚡ Content membership & policy invariants PASSED!")


def assert_snapshot(derived: Dict[str, Any]):
    actual_ext_additions = derived["external_additions"]
    expected_ext_additions = derived["expected_external_additions"]
    if actual_ext_additions != expected_ext_additions:
        missing = expected_ext_additions - actual_ext_additions
        unexpected = actual_ext_additions - expected_ext_additions
        err = ["Snapshot assertion failed: actual reviewed-pack additions do not match expected selected set derived from decisions and policy!"]
        if missing:
            err.append(f"Missing expected entries ({len(missing)}): {sorted(list(missing))}")
        if unexpected:
            err.append(f"Unexpected extra entries ({len(unexpected)}): {sorted(list(unexpected))}")
        raise AssertionError("\n".join(err))
    print("⚡ Reviewed pack baseline snapshot PASSED!")


def verify_tree_determinism(root_a: Path, root_b: Path):
    files_a = {p.relative_to(root_a): p for p in root_a.rglob("*") if p.is_file()}
    files_b = {p.relative_to(root_b): p for p in root_b.rglob("*") if p.is_file()}

    if set(files_a.keys()) != set(files_b.keys()):
        raise AssertionError(f"File set mismatch between Pass 1 and Pass 2 trees: {set(files_a.keys()) ^ set(files_b.keys())}")

    for rel_path, path_a in files_a.items():
        path_b = files_b[rel_path]
        bytes_a = path_a.read_bytes()
        bytes_b = path_b.read_bytes()
        if bytes_a != bytes_b:
            raise AssertionError(f"Byte mismatch in {rel_path} between Pass 1 and Pass 2!")

    print("⚡ Two-pass tree determinism verification PASSED!")


def run_self_tests(candidate_root: Path):
    print("Running verifier self-test suite against isolated mutated fixtures...")

    def create_fixture_root(tmp_parent: Path, index: int) -> Path:
        fixture_path = tmp_parent / f"fixture_{index}"
        fixture_path.mkdir(parents=True, exist_ok=True)
        for d in ["data/review-decisions", "data/review-queues", "data/reviewed", "data/build/packs"]:
            src = candidate_root / d
            dst = fixture_path / d
            if src.exists():
                shutil.copytree(src, dst)
        return fixture_path

    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_parent = Path(tmp_dir)

        # Self-Test 1: Mutate decisions.jsonl by removing one decision line
        f1 = create_fixture_root(tmp_parent, 1)
        dec_file = f1 / "data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl"
        dec_lines = dec_file.read_text(encoding="utf-8").splitlines()
        dec_file.write_text("\n".join(dec_lines[:-1]) + "\n", encoding="utf-8")
        try:
            d = derive_selection(f1)
            validate_policy_invariants(d, f1)
            assert_snapshot(d)
            raise RuntimeError("Self-test 1 failed: expected invariant error!")
        except AssertionError as e:
            print(f"✅ Self-test 1 passed (caught missing decision): {e}")

        # Self-Test 2: Mutate manifest external approved count
        f2 = create_fixture_root(tmp_parent, 2)
        man_file = f2 / "data/build/packs/reviewed/manifest.json"
        man_data = json.loads(man_file.read_text(encoding="utf-8"))
        man_data["external_approved_selected_count"] = 999
        man_file.write_text(json.dumps(man_data), encoding="utf-8")
        try:
            d = derive_selection(f2)
            validate_policy_invariants(d, f2)
            raise RuntimeError("Self-test 2 failed: expected manifest count assertion error!")
        except AssertionError as e:
            assert "Computed external additions count" in str(e), f"Unexpected error in Self-test 2: {e}"
            print(f"✅ Self-test 2 passed (caught manifest count mismatch): {e}")

        # Self-Test 3: Inject unresolved entry ('wela') into external additions
        f3 = create_fixture_root(tmp_parent, 3)
        d3 = derive_selection(f3)
        d3["rev_manifest"]["external_approved_selected_count"] = len(d3["external_additions"]) + 1
        d3["external_additions"].add("wela")
        try:
            validate_policy_invariants(d3, f3)
            raise RuntimeError("Self-test 3 failed: expected non-approved entry injection error!")
        except AssertionError as e:
            assert "Non-approved entry" in str(e) or "count" in str(e), f"Unexpected error in Self-test 3: {e}"
            print(f"✅ Self-test 3 passed (caught injected unresolved entry 'wela'): {e}")

        # Self-Test 4: Replace an entry while count remains unchanged
        f4 = create_fixture_root(tmp_parent, 4)
        d4 = derive_selection(f4)
        d4["rev_manifest"]["external_approved_selected_count"] = len(d4["external_additions"]) + d4["rev_manifest"].get("external_discarded_by_collision_count", 0)
        d4["external_additions"].remove("şeq")
        d4["external_additions"].add("bêabrûkirî")
        try:
            validate_policy_invariants(d4, f4)
            raise RuntimeError("Self-test 4 failed: expected wrong addition error!")
        except AssertionError as e:
            assert "Non-approved entry" in str(e), f"Unexpected error in Self-test 4: {e}"
            print(f"✅ Self-test 4 passed (caught replaced entry 'bêabrûkirî'): {e}")

        # Self-Test 5: Include sê in external additions while unresolved
        f5 = create_fixture_root(tmp_parent, 5)
        d5 = derive_selection(f5)
        d5["external_additions"].add("sê")
        d5["rev_manifest"]["external_approved_selected_count"] = len(d5["external_additions"]) + d5["rev_manifest"].get("external_discarded_by_collision_count", 0)
        try:
            validate_policy_invariants(d5, f5)
            raise RuntimeError("Self-test 5 failed: expected unselected sê error!")
        except AssertionError as e:
            assert "Unselected 'sê'" in str(e) or "Non-approved entry" in str(e) or "Approved sê entry" in str(e), f"Unexpected error in Self-test 5: {e}"
            print(f"✅ Self-test 5 passed (caught sê in external additions): {e}")

    print("⚡ Extended verifier self-test suite PASSED successfully!")


def main():
    args = parse_args()
    cand_root = Path(args.candidate_root).resolve()

    if args.self_test:
        run_self_tests(cand_root)

    derived = derive_selection(cand_root)
    validate_policy_invariants(derived, cand_root)
    assert_snapshot(derived)

    if args.compare_root_a and args.compare_root_b:
        verify_tree_determinism(Path(args.compare_root_a).resolve(), Path(args.compare_root_b).resolve())


if __name__ == "__main__":
    main()
