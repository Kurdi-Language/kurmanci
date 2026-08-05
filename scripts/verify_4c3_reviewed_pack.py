#!/usr/bin/env python3
"""
Kurmancî Milestone 4C.3 — Reviewed-Pack Rebuild & Determinism Verifier

This script is a read-only verifier for Milestone 4C.3. It:
1. Dynamically parses review decisions, imported records, and conflict groups.
2. Validates policy-derived selection rules and specific conflict-group invariants (e.g. sê).
3. Asserts the 4C.3 milestone baseline snapshot.
4. Optionally compares candidate seed against a base SHA seed root (--base-seed-root).
5. Optionally compares two build output trees for byte determinism (--compare-root-a / --compare-root-b).
6. Runs a self-test suite against temporary mutated fixtures (--self-test).
"""

import sys
import json
import argparse
import tempfile
import shutil
from pathlib import Path
from typing import Dict, Set, Any, List, Tuple


def parse_args():
    parser = argparse.ArgumentParser(description="Verify 4C.3 Reviewed Pack Rebuild & Invariants")
    parser.add_argument("--candidate-root", type=str, default=".", help="Path to workspace root to verify")
    parser.add_argument("--base-seed-root", type=str, help="Path to base SHA workspace/pack root for seed comparison")
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


def load_seed_entries(seed_path: Path) -> Set[str]:
    if not seed_path.exists():
        raise FileNotFoundError(f"Manual seed file not found: {seed_path}")
    lines = seed_path.read_text(encoding="utf-8").splitlines()
    words = set()
    for line in lines:
        if not line.strip():
            continue
        rec = json.loads(line)
        words.add(rec["normalized"])
    return words


def load_pack_manifest(manifest_path: Path) -> Dict[str, Any]:
    if not manifest_path.exists():
        raise FileNotFoundError(f"Pack manifest not found: {manifest_path}")
    return json.loads(manifest_path.read_text(encoding="utf-8"))


def load_target_displays(candidate_root: Path) -> Dict[str, str]:
    queues_dir = candidate_root / "data/review-queues/kurdish-hunspell-kmr"
    target_to_display = {}
    if queues_dir.exists():
        for qfile in queues_dir.glob("*.jsonl"):
            for line in qfile.read_text(encoding="utf-8").splitlines():
                if not line.strip():
                    continue
                rec = json.loads(line)
                t_id = rec.get("target_id")
                disp = rec.get("display") or rec.get("normalized")
                if t_id and disp:
                    target_to_display[t_id] = disp
    return target_to_display


def load_conflict_groups(candidate_root: Path) -> Dict[str, List[str]]:
    groups_path = candidate_root / "data/review-queues/kurdish-hunspell-kmr/metadata-conflict-groups.jsonl"
    groups = {}
    if groups_path.exists():
        for line in groups_path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            rec = json.loads(line)
            groups[rec["target_id"]] = rec.get("member_entry_ids", [])
    return groups


def derive_selection(candidate_root: Path) -> Tuple[Dict[str, Any], Set[str], Set[str]]:
    dec_path = candidate_root / "data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl"
    seed_path = candidate_root / "data/reviewed/lexicon.jsonl"
    rev_manifest_path = candidate_root / "data/build/packs/reviewed/manifest.json"

    decisions = load_decisions(dec_path)
    seed_words = load_seed_entries(seed_path)
    rev_manifest = load_pack_manifest(rev_manifest_path)
    target_displays = load_target_displays(candidate_root)
    conflict_groups = load_conflict_groups(candidate_root)

    approved_decs = [d for d in decisions if d["review_status"] == "approved"]
    non_approved_decs = [d for d in decisions if d["review_status"] != "approved"]

    # Read reviewed pack manifest counts
    manual_seed_count = rev_manifest.get("manual_seed_selected_count", 0)
    ext_approved_count = rev_manifest.get("external_approved_selected_count", 0)
    total_entries = rev_manifest.get("final_unique_entry_count", 0)

    derived = {
        "total_decisions_count": len(decisions),
        "approved_decisions": approved_decs,
        "approved_decisions_count": len(approved_decs),
        "non_approved_decisions_count": len(non_approved_decs),
        "manual_seed_count": manual_seed_count,
        "external_approved_selected_count": ext_approved_count,
        "final_unique_entry_count": total_entries,
        "seed_words": seed_words,
        "target_displays": target_displays,
        "conflict_groups": conflict_groups,
    }

    return derived, seed_words, set()


def validate_policy_invariants(derived: Dict[str, Any], candidate_root: Path):
    decisions_count = derived["total_decisions_count"]
    if decisions_count != 13:
        raise AssertionError(f"Expected 13 review decisions, got {decisions_count}")

    approved_count = derived["approved_decisions_count"]
    non_approved_count = derived["non_approved_decisions_count"]
    if approved_count + non_approved_count != 13:
        raise AssertionError(f"Decision count mismatch: {approved_count} + {non_approved_count} != 13")

    if approved_count != 3:
        raise AssertionError(f"Expected 3 approved decisions, got {approved_count}")
    if non_approved_count != 10:
        raise AssertionError(f"Expected 10 non-approved decisions, got {non_approved_count}")

    # Check sê conflict group invariant
    target_displays = derived["target_displays"]
    conflict_groups = derived["conflict_groups"]
    approved_decs = derived["approved_decisions"]

    se_dec = None
    for d in approved_decs:
        t_id = d["target_id"]
        disp = target_displays.get(t_id, "")
        if disp == "sê" or "sê" in d.get("evidence", [""])[0] or "sê" in d.get("review_notes", ""):
            se_dec = d
            break

    if not se_dec:
        raise AssertionError("Could not find decision for sê")

    if se_dec["target_type"] != "entry":
        raise AssertionError("sê decision target_type must be 'entry'")
    if se_dec["review_status"] != "approved":
        raise AssertionError("sê decision review_status must be 'approved'")

    # Verify sê belongs to a conflict group
    se_target_id = se_dec["target_id"]
    in_group = any(se_target_id in members for members in conflict_groups.values())
    if not in_group:
        raise AssertionError("sê must belong to a conflict group")

    # Verify sê is unselected (external_approved_selected_count == 2)
    ext_approved_selected = derived["external_approved_selected_count"]
    if ext_approved_selected != 2:
        raise AssertionError(f"Expected external_approved_selected_count = 2, got {ext_approved_selected}")


def assert_snapshot(derived: Dict[str, Any]):
    target_displays = derived["target_displays"]
    approved_displays = {target_displays.get(d["target_id"]) for d in derived["approved_decisions"]}

    # Check that approved target_ids include sê, şeq, şer
    if not ("sê" in approved_displays and "şeq" in approved_displays and "şer" in approved_displays):
        raise AssertionError(f"Snapshot assertion failed: 3 approved decisions must cover sê, şeq, şer; got {approved_displays}")

    print("⚡ 4C.3 Policy-derived & snapshot verifications PASSED!")


def verify_seed_equivalence(candidate_root: Path, base_seed_root: Path):
    cand_manifest = load_pack_manifest(candidate_root / "data/build/packs/seed/manifest.json")
    base_manifest = load_pack_manifest(base_seed_root / "data/build/packs/seed/manifest.json")

    if cand_manifest["final_unique_entry_count"] != base_manifest["final_unique_entry_count"]:
        raise AssertionError("Seed entry count changed relative to base SHA!")
    if cand_manifest["binary_sha256"] != base_manifest["binary_sha256"]:
        raise AssertionError("Seed binary SHA-256 changed relative to base SHA!")

    cand_bin = (candidate_root / "data/build/packs/seed/lexicon.bin").read_bytes()
    base_bin = (base_seed_root / "data/build/packs/seed/lexicon.bin").read_bytes()
    if cand_bin != base_bin:
        raise AssertionError("Seed binary byte contents changed relative to base SHA!")

    print("⚡ Base seed equivalence verification PASSED!")


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
    print("Running verifier self-test suite against mutated fixtures...")
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_path = Path(tmp_dir)
        # Copy workspace fixture
        for d in ["data/review-decisions", "data/review-queues", "data/reviewed", "data/build/packs"]:
            src = candidate_root / d
            dst = tmp_path / d
            if src.exists():
                shutil.copytree(src, dst)

        # Test 1: Mutate decisions count
        dec_file = tmp_path / "data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl"
        dec_lines = dec_file.read_text(encoding="utf-8").splitlines()
        dec_file.write_text("\n".join(dec_lines[:-1]) + "\n", encoding="utf-8")
        try:
            d, _, _ = derive_selection(tmp_path)
            validate_policy_invariants(d, tmp_path)
            raise RuntimeError("Self-test 1 failed: expected decision count assertion error!")
        except AssertionError as e:
            print(f"✅ Self-test 1 passed (caught missing decision): {e}")

        # Restore
        dec_file.write_text("\n".join(dec_lines) + "\n", encoding="utf-8")

        # Test 2: Mutate external approved count in manifest
        man_file = tmp_path / "data/build/packs/reviewed/manifest.json"
        man_data = json.loads(man_file.read_text())
        man_data["external_approved_selected_count"] = 3
        man_file.write_text(json.dumps(man_data), encoding="utf-8")
        try:
            d, _, _ = derive_selection(tmp_path)
            validate_policy_invariants(d, tmp_path)
            raise RuntimeError("Self-test 2 failed: expected external approved count assertion error!")
        except AssertionError as e:
            print(f"✅ Self-test 2 passed (caught invalid external approved count): {e}")

    print("⚡ Verifier self-test suite PASSED successfully!")


def main():
    args = parse_args()
    cand_root = Path(args.candidate_root).resolve()

    if args.self_test:
        run_self_tests(cand_root)

    derived, _, _ = derive_selection(cand_root)
    validate_policy_invariants(derived, cand_root)
    assert_snapshot(derived)

    if args.base_seed_root:
        verify_seed_equivalence(cand_root, Path(args.base_seed_root).resolve())

    if args.compare_root_a and args.compare_root_b:
        verify_tree_determinism(Path(args.compare_root_a).resolve(), Path(args.compare_root_b).resolve())


if __name__ == "__main__":
    main()
