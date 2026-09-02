#!/usr/bin/env python3
"""
Local Kuwiki Worksheet Fidelity Verifier for Kuwiki Batch 001.

Validates that the uncommitted human worksheet (kuwiki_batch_001_worksheet.json)
was transcribed 100% identically into committed candidate & decision records.
"""

import sys
import json
import argparse
import hashlib
import tempfile
import shutil
from pathlib import Path

EXPECTED_WORKSHEET_SHA256 = "7c1341d75a2a1e8530495d9c69c45e10e7ba991f745ccf8a69a8c75db81af4b2"
EXPECTED_CANDIDATES_SHA256 = "23d3871a8f6ef285ba9b6f231fe5d65f201934eaee2965d18cdec7770aeb3c1d"

STATUS_MAP = {
    "approved": "approved",
    "rejected": "rejected_from_default_pack",
    "rejected_from_default_pack": "rejected_from_default_pack",
    "experimental_only": "experimental_only",
    "needs_linguist": "needs_linguist"
}


def hash_field(hasher, data: bytes):
    hasher.update(len(data).to_bytes(8, "big"))
    hasher.update(data)


def compute_entry_id(source_id: str, source_revision: str, display: str, normalized: str, flags: str = "", morphology: list = None):
    if morphology is None:
        morphology = []
    hasher = hashlib.sha256()
    hash_field(hasher, b"kurmanci-review-entry-v1")
    hash_field(hasher, source_id.encode("utf-8"))
    hash_field(hasher, source_revision.encode("utf-8"))
    hash_field(hasher, display.encode("utf-8"))
    hash_field(hasher, normalized.encode("utf-8"))
    hash_field(hasher, flags.encode("utf-8"))
    for m in sorted(morphology):
        hash_field(hasher, m.encode("utf-8"))
    return hasher.hexdigest()


def find_default_worksheet_path() -> Path:
    return Path("scratch/kuwiki_batch_001_worksheet.json")


def verify_worksheet_fidelity(worksheet_path: Path, repo_root: Path, quiet: bool = False, expected_worksheet_sha: str = EXPECTED_WORKSHEET_SHA256, expected_candidates_sha: str = EXPECTED_CANDIDATES_SHA256) -> int:
    if not worksheet_path.exists():
        raise FileNotFoundError(f"Local worksheet file not found at {worksheet_path}")

    ws_bytes = worksheet_path.read_bytes()
    ws_sha = hashlib.sha256(ws_bytes).hexdigest()
    if ws_sha != expected_worksheet_sha:
        raise ValueError(f"Worksheet SHA-256 mismatch! Got '{ws_sha}', expected '{expected_worksheet_sha}'")

    cand_path = repo_root / "data/review-batches/kuwiki-batch-001/candidates.jsonl"
    dec_path = repo_root / "data/review-decisions/kuwiki-batch-001/decisions.jsonl"

    if not cand_path.exists() or not dec_path.exists():
        raise FileNotFoundError(f"Committed candidates ({cand_path}) or decisions ({dec_path}) missing")

    cand_bytes = cand_path.read_bytes()
    cand_sha = hashlib.sha256(cand_bytes).hexdigest()
    if cand_sha != expected_candidates_sha:
        raise ValueError(f"Candidate file SHA-256 mismatch! Got '{cand_sha}', expected '{expected_candidates_sha}'")

    cands = [json.loads(line) for line in cand_bytes.decode("utf-8").splitlines() if line.strip()]
    decs = [json.loads(line) for line in dec_path.read_text(encoding="utf-8").splitlines() if line.strip()]

    # Candidate count == 1000
    if len(cands) != 1000:
        raise ValueError(f"Candidate count mismatch! Got {len(cands)}, expected 1000")

    # Candidate batch ranks unique and == 1..1000
    cand_ranks = set()
    cand_target_ids = set()
    cand_by_rank = {}
    for cand in cands:
        r = cand["batch_rank"]
        if r in cand_ranks:
            raise ValueError(f"Duplicated candidate batch_rank {r}")
        cand_ranks.add(r)
        cand_by_rank[r] = cand

        tid = compute_entry_id(
            "kuwiki-batch-001",
            EXPECTED_CANDIDATES_SHA256,
            cand["token"],
            cand["normalized_token"]
        )
        if tid in cand_target_ids:
            raise ValueError(f"Duplicated candidate target_id '{tid}'")
        cand_target_ids.add(tid)

    if cand_ranks != set(range(1, 1001)):
        raise ValueError("Candidate batch ranks do not equal 1..1000")
    if len(cand_target_ids) != 1000:
        raise ValueError("Computed candidate target_ids are not unique")

    # Decision count == 1000 and target_ids unique
    if len(decs) != 1000:
        raise ValueError(f"Decision count mismatch! Got {len(decs)}, expected 1000")

    dec_target_ids = set()
    dec_by_target_id = {}
    for dec in decs:
        tid = dec["target_id"]
        if tid in dec_target_ids:
            raise ValueError(f"Duplicated decision target_id '{tid}'")
        dec_target_ids.add(tid)
        dec_by_target_id[tid] = dec

    if len(dec_target_ids) != 1000:
        raise ValueError("Decision target_ids are not unique")

    if cand_target_ids != dec_target_ids:
        raise ValueError("Candidate target_id set does not match decision target_id set")

    # Worksheet decision count == 1000
    ws_data = json.loads(ws_bytes)
    ws_decs = ws_data.get("decisions", {})
    if len(ws_decs) != 1000:
        raise ValueError(f"Worksheet decision count mismatch! Got {len(ws_decs)}, expected 1000")

    ws_ranks = set()
    for rank_key, item in ws_decs.items():
        if isinstance(rank_key, str) and rank_key.isdigit():
            if int(rank_key) != item["batch_rank"]:
                raise ValueError(f"Worksheet record key '{rank_key}' does not match embedded batch_rank {item['batch_rank']}")

        r = item["batch_rank"]
        if r in ws_ranks:
            raise ValueError(f"Duplicated worksheet batch_rank {r}")
        ws_ranks.add(r)

    if ws_ranks != set(range(1, 1001)):
        raise ValueError("Worksheet batch ranks do not equal 1..1000")

    matches = 0
    for r in range(1, 1001):
        item = ws_decs.get(str(r)) or ws_decs.get(r)
        if not item:
            raise ValueError(f"Missing worksheet record for rank {r}")

        cand = cand_by_rank.get(r)
        if not cand:
            raise ValueError(f"Missing candidate for rank {r}")

        tid = compute_entry_id(
            "kuwiki-batch-001",
            EXPECTED_CANDIDATES_SHA256,
            cand["token"],
            cand["normalized_token"]
        )
        dec = dec_by_target_id.get(tid)
        if not dec:
            raise ValueError(f"Missing decision for candidate target_id '{tid}' (rank {r})")

        if item["token"] != cand["token"]:
            raise ValueError(f"Token mismatch at rank {r}: worksheet '{item['token']}' vs cand '{cand['token']}'")

        if item["normalized_token"] != cand["normalized_token"]:
            raise ValueError(f"Normalized token mismatch at rank {r}: worksheet '{item['normalized_token']}' vs cand '{cand['normalized_token']}'")

        expected_status = STATUS_MAP.get(item["decision"], item["decision"])
        if dec["review_status"] != expected_status:
            raise ValueError(f"Status mismatch at rank {r}: worksheet '{expected_status}' vs committed '{dec['review_status']}'")

        matches += 1

    if not quiet:
        print(f"✔ Worksheet SHA-256 verified: {ws_sha}")
        print(f"✔ Candidates SHA-256 verified: {cand_sha}")
        print(f"⚡ Local Kuwiki Worksheet Fidelity Verification PASSED: {matches}/1000 items match 100% identically!")

    return matches


def run_self_test(repo_root: Path, worksheet_path: Path):
    print("Running verify_kuwiki_worksheet self-test suite...")

    if not worksheet_path.exists():
        print(f"❌ Error: Self-test requires local worksheet file at {worksheet_path}")
        sys.exit(1)

    # Base verification check
    try:
        verify_worksheet_fidelity(worksheet_path, repo_root, quiet=True)
        print("✅ Base fidelity verification passed (1000/1000)")
    except Exception as e:
        print(f"❌ Base verification failed unexpectedly: {e}")
        sys.exit(1)

    with tempfile.TemporaryDirectory() as tmpdir:
        tmp_path = Path(tmpdir)
        tmp_repo = tmp_path / "repo"
        shutil.copytree(repo_root / "data", tmp_repo / "data")
        tmp_ws = tmp_path / "worksheet.json"
        shutil.copyfile(worksheet_path, tmp_ws)

        # 1. Candidate SHA Mismatch
        try:
            verify_worksheet_fidelity(tmp_ws, tmp_repo, quiet=True, expected_candidates_sha="0000000000000000000000000000000000000000000000000000000000000000")
            print("❌ Self-test failed: candidate SHA mismatch was NOT caught")
            sys.exit(1)
        except ValueError as e:
            assert "Candidate file SHA-256 mismatch" in str(e)
            print("✅ Self-test 1 passed: caught candidate SHA mismatch")

        # 2. Duplicated Candidate Rank
        cand_file = tmp_repo / "data/review-batches/kuwiki-batch-001/candidates.jsonl"
        lines = cand_file.read_text(encoding="utf-8").splitlines()
        rec2 = json.loads(lines[1])
        rec2["batch_rank"] = 1
        lines[1] = json.dumps(rec2)
        cand_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
        tampered_cand_sha = hashlib.sha256(cand_file.read_bytes()).hexdigest()

        try:
            verify_worksheet_fidelity(tmp_ws, tmp_repo, quiet=True, expected_candidates_sha=tampered_cand_sha)
            print("❌ Self-test failed: duplicated candidate rank was NOT caught")
            sys.exit(1)
        except ValueError as e:
            assert "Duplicated candidate batch_rank" in str(e)
            print("✅ Self-test 2 passed: caught duplicated candidate rank")

        # Reset candidates
        shutil.copytree(repo_root / "data/review-batches", tmp_repo / "data/review-batches", dirs_exist_ok=True)

        # 3. Duplicated Decision Target ID
        dec_file = tmp_repo / "data/review-decisions/kuwiki-batch-001/decisions.jsonl"
        d_lines = dec_file.read_text(encoding="utf-8").splitlines()
        d0 = json.loads(d_lines[0])
        d1 = json.loads(d_lines[1])
        d1["target_id"] = d0["target_id"]
        d_lines[1] = json.dumps(d1)
        dec_file.write_text("\n".join(d_lines) + "\n", encoding="utf-8")

        try:
            verify_worksheet_fidelity(tmp_ws, tmp_repo, quiet=True)
            print("❌ Self-test failed: duplicated decision target_id was NOT caught")
            sys.exit(1)
        except ValueError as e:
            assert "Duplicated decision target_id" in str(e) or "Decision count mismatch" in str(e)
            print("✅ Self-test 3 passed: caught duplicated decision target_id")

        # Reset decisions
        shutil.copytree(repo_root / "data/review-decisions", tmp_repo / "data/review-decisions", dirs_exist_ok=True)

        # 4. Duplicated Worksheet Rank & Missing Worksheet Rank
        ws_data = json.loads(tmp_ws.read_bytes())
        ws_decs = ws_data["decisions"]
        # Remove rank 1000, key rank 2 as 1
        ws_decs["2"]["batch_rank"] = 1
        ws_data["decisions"] = ws_decs
        tmp_ws.write_text(json.dumps(ws_data), encoding="utf-8")
        tmp_ws_sha = hashlib.sha256(tmp_ws.read_bytes()).hexdigest()

        try:
            verify_worksheet_fidelity(tmp_ws, tmp_repo, quiet=True, expected_worksheet_sha=tmp_ws_sha)
            print("❌ Self-test failed: duplicated worksheet rank was NOT caught")
            sys.exit(1)
        except ValueError as e:
            assert "batch_rank" in str(e) or "ranks do not equal" in str(e)
            print("✅ Self-test 4 passed: caught duplicated/mismatched worksheet rank")

    print("⚡ verify_kuwiki_worksheet self-test suite PASSED successfully!")


def main():
    parser = argparse.ArgumentParser(description="Verify local Kuwiki human worksheet fidelity against committed decisions.")
    parser.add_argument("--worksheet", type=Path, default=find_default_worksheet_path(), help="Path to local kuwiki_batch_001_worksheet.json")
    parser.add_argument("--repo-root", type=Path, default=Path("."), help="Path to repository root")
    parser.add_argument("--self-test", action="store_true", help="Run self-test suite against mutated temporary fixtures")
    args = parser.parse_args()

    if args.self_test:
        run_self_test(args.repo_root, args.worksheet)
        sys.exit(0)

    try:
        verify_worksheet_fidelity(args.worksheet, args.repo_root)
    except Exception as e:
        print(f"❌ Error: Verification failed: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
