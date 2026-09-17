//! Pre-review filtering and the authoritative fail-closed invariant on the Hunspell path:
//! `generate-review-queues` keeps every out-of-alphabet import entry out of the ordinary
//! review pool (evidence retained in `alphabet-policy-excluded.jsonl`, import untouched),
//! hyphen/apostrophe forms stay reviewable, and an `approved` decision on such an entry
//! makes authoritative pack resolution fail with a clear message instead of silently
//! entering or leaving the reviewed pack.

use data_builder_lib::alphabet::{out_of_alphabet_chars, WORD_INTERNAL_PUNCTUATION};
use data_builder_lib::pack::builder::resolve_authoritative_pack_payload;
use data_builder_lib::review::queues::EntryQueueRecord;
use data_builder_lib::{generate_review_queues, validate_review_decisions};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn ws_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_all(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Everything the review pipeline and the authoritative resolver read, copied so the
/// repository itself is never touched.
fn fixture() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    for d in [
        "data/source-registry",
        "data/raw",
        "data/original/kurdish-hunspell-kmr",
        "data/imported/kurdish-hunspell-kmr",
        "data/reviewed",
        "data/reports/kurdish-hunspell-kmr",
        "data/review-decisions",
        "data/review-queues",
        "data/review-batches",
    ] {
        let src = ws_root().join(d);
        if src.exists() {
            copy_dir_all(&src, &root.join(d));
        }
    }
    fs::copy(
        ws_root().join("data/pack-policy.toml"),
        root.join("data/pack-policy.toml"),
    )
    .unwrap();
    (tmp, root)
}

fn read_queue(path: &Path) -> Vec<EntryQueueRecord> {
    BufReader::new(fs::File::open(path).unwrap())
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(&l).unwrap())
        .collect()
}

fn sha256_file(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

#[test]
fn hunspell_review_pool_excludes_out_of_alphabet_entries_and_keeps_evidence() {
    let (_tmp, root) = fixture();
    let import = root.join("data/imported/kurdish-hunspell-kmr/lexicon.jsonl");
    let import_before = sha256_file(&import);

    let summary = generate_review_queues("kurdish-hunspell-kmr", &root).unwrap();
    assert!(summary.alphabet_policy_excluded_count > 0);
    assert_eq!(
        sha256_file(&import),
        import_before,
        "the import is never modified"
    );

    let qdir = root.join("data/review-queues/kurdish-hunspell-kmr");
    let pool = read_queue(&qdir.join("hunspell-only.jsonl"));
    let excluded = read_queue(&qdir.join("alphabet-policy-excluded.jsonl"));
    assert_eq!(pool.len(), summary.hunspell_only_entries_count);
    assert_eq!(excluded.len(), summary.alphabet_policy_excluded_count);

    // The ordinary review pool is clean; hyphen/apostrophe forms stay in it.
    let offenders: Vec<&str> = pool
        .iter()
        .filter(|r| !out_of_alphabet_chars(&r.normalized).is_empty())
        .map(|r| r.normalized.as_str())
        .collect();
    assert!(offenders.is_empty(), "{:?}", offenders);
    assert!(pool.iter().any(|r| r
        .normalized
        .chars()
        .any(|c| WORD_INTERNAL_PUNCTUATION.contains(&c))));
    assert!(pool.iter().all(|r| r.suggested_action == "retain"));

    // Every excluded record is outside the alphabet, says why, and is not assigned.
    for r in &excluded {
        assert!(
            !out_of_alphabet_chars(&r.normalized).is_empty(),
            "{}",
            r.normalized
        );
        assert_eq!(r.rule_id, "ALPHABET_POLICY_V1");
        assert_eq!(r.suggested_action, "excluded_by_alphabet_policy");
        assert!(r.reason_codes.iter().any(|c| c == "OUT_OF_ALPHABET"));
        assert!(
            r.reason_codes.iter().any(|c| c.contains("U+")),
            "{:?}",
            r.reason_codes
        );
        assert_eq!(r.generated_status, "unreviewed");
    }
    // The pool and the excluded queue partition the non-seed import entries.
    let pool_ids: std::collections::BTreeSet<&str> =
        pool.iter().map(|r| r.target_id.as_str()).collect();
    assert!(excluded
        .iter()
        .all(|r| !pool_ids.contains(r.target_id.as_str())));

    // Any other queue record of an excluded entry carries the policy reason too.
    let excluded_ids: std::collections::BTreeSet<&str> =
        excluded.iter().map(|r| r.target_id.as_str()).collect();
    for name in [
        "rare-code-points.jsonl",
        "digit-only.jsonl",
        "symbol-only.jsonl",
    ] {
        for r in read_queue(&qdir.join(name)) {
            if excluded_ids.contains(r.target_id.as_str()) {
                assert!(
                    r.reason_codes.iter().any(|c| c == "OUT_OF_ALPHABET"),
                    "{}",
                    name
                );
                assert_eq!(r.suggested_action, "excluded_by_alphabet_policy");
            }
        }
    }
    // The generated queues validate and merge with the committed decisions.
    validate_review_decisions("kurdish-hunspell-kmr", &root).unwrap();
    eprintln!(
        "review pool {} entries; {} excluded from ordinary review by the alphabet policy",
        pool.len(),
        excluded.len()
    );
}

#[test]
fn approved_hunspell_decision_on_out_of_alphabet_entry_fails_authoritative_resolution() {
    let (_tmp, root) = fixture();
    generate_review_queues("kurdish-hunspell-kmr", &root).unwrap();
    let qdir = root.join("data/review-queues/kurdish-hunspell-kmr");
    let excluded = read_queue(&qdir.join("alphabet-policy-excluded.jsonl"));
    let victim = excluded
        .iter()
        .find(|r| r.normalized.chars().all(|c| c.is_alphabetic()))
        .expect("an alphabetic out-of-alphabet entry (e.g. with ḧ)");
    let decisions = root.join("data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl");
    let before = fs::read_to_string(&decisions).unwrap();

    // Sanity: the untouched fixture resolves.
    validate_review_decisions("kurdish-hunspell-kmr", &root).unwrap();
    resolve_authoritative_pack_payload("reviewed", &root).unwrap();

    // A contradictory approval: the resolver refuses, naming the form, source and decision.
    let record = serde_json::json!({
        "schema_version": "review-decision-v1",
        "target_type": "entry",
        "target_id": victim.target_id,
        "source_id": "kurdish-hunspell-kmr",
        "review_status": "approved",
        "reviewer_id": "test-reviewer",
        "review_date": "2026-09-17",
        "review_notes": "test: contradictory approval",
        "evidence": []
    });
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(&decisions)
        .unwrap();
    writeln!(f, "{}", record).unwrap();
    validate_review_decisions("kurdish-hunspell-kmr", &root).unwrap();
    for pack in ["reviewed", "experimental-full"] {
        let err = resolve_authoritative_pack_payload(pack, &root)
            .err()
            .unwrap_or_else(|| panic!("{}: contradictory approval must fail", pack));
        assert!(
            err.contains("production lexical eligibility violation"),
            "{}",
            err
        );
        assert!(
            err.contains(&format!("token: {:?}", victim.display)),
            "{}",
            err
        );
        assert!(err.contains("source: kurdish-hunspell-kmr"), "{}", err);
        assert!(err.contains("decision: approved"), "{}", err);
    }

    // The same entry rejected from the default pack is a legitimate state.
    let rejected = before.trim_end().to_string()
        + "\n"
        + &serde_json::json!({
            "schema_version": "review-decision-v1",
            "target_type": "entry",
            "target_id": victim.target_id,
            "source_id": "kurdish-hunspell-kmr",
            "review_status": "rejected_from_default_pack",
            "reviewer_id": "test-reviewer",
            "review_date": "2026-09-17",
            "review_notes": "test: outside the alphabet",
            "evidence": []
        })
        .to_string()
        + "\n";
    fs::write(&decisions, rejected).unwrap();
    validate_review_decisions("kurdish-hunspell-kmr", &root).unwrap();
    let reviewed = resolve_authoritative_pack_payload("reviewed", &root).unwrap();
    assert!(reviewed
        .resolved_entries
        .iter()
        .all(|e| e.normalized != victim.normalized));
    resolve_authoritative_pack_payload("experimental-full", &root).unwrap();
}
