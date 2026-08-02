use data_builder_lib::{
    compute_conflict_group_id, compute_entry_id, generate_review_queues, validate_review_decisions,
    ReviewDecisionRecord, ReviewDecisionStatus, ReviewTargetType, REVIEW_DECISION_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::BufRead;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir)
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn prepare_review_fixture() -> (TempDir, PathBuf) {
    let root = get_workspace_root();
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_root = temp_dir.path().to_path_buf();

    let dirs_to_copy = [
        "data/source-registry",
        "data/raw",
        "data/original",
        "data/imported",
        "data/reports",
        "data/review-decisions",
        "data/review-queues",
    ];

    for d in &dirs_to_copy {
        let src = root.join(d);
        if src.exists() {
            let dst = temp_root.join(d);
            copy_dir_all(&src, &dst)
                .unwrap_or_else(|e| panic!("Failed to copy fixture dir {:?}: {}", d, e));
        }
    }

    (temp_dir, temp_root)
}

#[test]
fn test_length_prefixed_canonical_id_encoding_and_source_revision() {
    let id1 = compute_entry_id(
        "kurdish-hunspell-kmr",
        "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "roj",
        "roj",
        "AN",
        &["po:noun".to_string()],
    )
    .unwrap();

    let id2 = compute_entry_id(
        "kurdish-hunspell-kmr",
        "88131d6878ef7fa3ee114aa554adc385ff85b44d",
        "roj",
        "roj",
        "AN",
        &["po:noun".to_string()],
    )
    .unwrap();

    assert_ne!(
        id1, id2,
        "Entry ID must change when source_revision changes"
    );

    let id_morph1 = compute_entry_id(
        "kurdish-hunspell-kmr",
        "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "roj",
        "roj",
        "AN",
        &["po:noun".to_string(), "is:def".to_string()],
    )
    .unwrap();

    let id_morph2 = compute_entry_id(
        "kurdish-hunspell-kmr",
        "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "roj",
        "roj",
        "AN",
        &["is:def".to_string(), "po:noun".to_string()],
    )
    .unwrap();

    assert_eq!(
        id_morph1, id_morph2,
        "Entry ID must be identical regardless of input morphology ordering"
    );
}

#[test]
fn test_commit_sha_validation() {
    use data_builder_lib::review::queues::validate_commit_sha;

    assert!(validate_commit_sha("88131d6878ef7fa3ee114aa554adc385ff85b44c").is_ok());
    assert!(validate_commit_sha("88131d6878ef7fa3ee114aa554adc385ff85b44").is_err());
    assert!(validate_commit_sha("88131d6878ef7fa3ee114aa554adc385ff85b44zz").is_err());
}

#[test]
fn test_group_id_determinism_and_member_ordering_independence() {
    let members1 = vec!["entry-id-a".to_string(), "entry-id-b".to_string()];
    let members2 = vec!["entry-id-b".to_string(), "entry-id-a".to_string()];

    let gid1 = compute_conflict_group_id("roj", &members1).unwrap();
    let gid2 = compute_conflict_group_id("roj", &members2).unwrap();

    assert_eq!(
        gid1, gid2,
        "Group ID must be independent of input member ordering"
    );
}

#[test]
fn test_review_decision_schema_validation() {
    use data_builder_lib::review::schema::validate_decision_record;

    let valid_approved = ReviewDecisionRecord {
        schema_version: REVIEW_DECISION_SCHEMA_VERSION.to_string(),
        target_type: ReviewTargetType::Entry,
        target_id: "entry-123".to_string(),
        source_id: "kurdish-hunspell-kmr".to_string(),
        review_status: ReviewDecisionStatus::Approved,
        reviewer_id: Some("maintainer-1".to_string()),
        review_date: Some("2026-08-02".to_string()),
        review_notes: None,
        evidence: vec![],
        replacement_metadata: None,
    };
    assert!(validate_decision_record(&valid_approved).is_ok());

    let invalid_unreviewed = ReviewDecisionRecord {
        schema_version: REVIEW_DECISION_SCHEMA_VERSION.to_string(),
        target_type: ReviewTargetType::Entry,
        target_id: "entry-123".to_string(),
        source_id: "kurdish-hunspell-kmr".to_string(),
        review_status: ReviewDecisionStatus::Unreviewed,
        reviewer_id: Some("maintainer-1".to_string()),
        review_date: None,
        review_notes: None,
        evidence: vec![],
        replacement_metadata: None,
    };
    assert!(validate_decision_record(&invalid_unreviewed).is_err());
}

#[test]
fn test_review_queues_and_decisions_pipeline_determinism() {
    let (_temp_dir, temp_root) = prepare_review_fixture();

    let q_summary1 = generate_review_queues("kurdish-hunspell-kmr", &temp_root)
        .expect("Pass 1 queue gen failed");
    assert!(q_summary1.total_imported_records > 0);
    assert_eq!(
        q_summary1.source_revision,
        "88131d6878ef7fa3ee114aa554adc385ff85b44c"
    );

    let queues_dir = temp_root.join("data/review-queues/kurdish-hunspell-kmr");
    let manifest1_path = queues_dir.join("artifacts.sha256");
    let manifest1_content = fs::read_to_string(&manifest1_path).unwrap();

    let expected_queue_files = [
        "parser-rejections.jsonl",
        "metadata-conflict-groups.jsonl",
        "suspicious-entries.jsonl",
        "unusual-scripts.jsonl",
        "mixed-scripts.jsonl",
        "punctuation-only.jsonl",
        "symbol-only.jsonl",
        "digit-only.jsonl",
        "no-letter.jsonl",
        "rare-code-points.jsonl",
        "unexpected-code-points.jsonl",
        "short-and-long-forms.jsonl",
        "capitalization-anomalies.jsonl",
        "multiword-entries.jsonl",
        "hunspell-only.jsonl",
    ];
    for qf in &expected_queue_files {
        let path = queues_dir.join(qf);
        assert!(path.exists(), "Queue file {} must exist", qf);

        let content = fs::read(&path).unwrap();
        let hash = format!("{:x}", Sha256::digest(&content));
        let expected_line = format!("{} data/review-queues/kurdish-hunspell-kmr/{}", hash, qf);
        assert!(
            manifest1_content.contains(&expected_line),
            "Manifest must contain correct hash for {}",
            qf
        );
    }

    // Verify conflict groups preserve complete morphology evidence and real entry IDs
    let cg_path = queues_dir.join("metadata-conflict-groups.jsonl");
    let cg_file = File::open(&cg_path).unwrap();
    let reader = std::io::BufReader::new(cg_file);
    let mut cg_count = 0;
    for line in reader.lines() {
        let l = line.unwrap();
        if l.trim().is_empty() {
            continue;
        }
        let val: serde_json::Value = serde_json::from_str(&l).unwrap();
        let members = val.get("members").unwrap().as_array().unwrap();
        let normalized = val.get("normalized").unwrap().as_str().unwrap();
        assert!(
            members.len() >= 2,
            "Conflict group must contain at least 2 members"
        );
        for m in members {
            let eid = m.get("entry_id").unwrap().as_str().unwrap();
            let display = m.get("display").unwrap().as_str().unwrap();
            let flags = m.get("flags").unwrap().as_str().unwrap();
            let morph: Vec<String> = m
                .get("morphology")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_string())
                .collect();

            // Re-calculate expected entry ID using actual normalized form from conflict item
            let expected_eid = compute_entry_id(
                "kurdish-hunspell-kmr",
                "88131d6878ef7fa3ee114aa554adc385ff85b44c",
                display,
                normalized,
                flags,
                &morph,
            )
            .unwrap();
            assert_eq!(
                eid, expected_eid,
                "Conflict member entry_id must equal authentic ID calculated from source record"
            );
        }
        cg_count += 1;
    }
    assert!(
        cg_count > 0,
        "Metadata conflict groups queue must contain entries"
    );

    // Pass 2 - Determinism check
    let q_summary2 = generate_review_queues("kurdish-hunspell-kmr", &temp_root)
        .expect("Pass 2 queue gen failed");
    let manifest2_content = fs::read_to_string(&manifest1_path).unwrap();
    assert_eq!(manifest1_content, manifest2_content);
    assert_eq!(
        q_summary1.metadata_conflict_groups_count,
        q_summary2.metadata_conflict_groups_count
    );

    // Validate review decisions
    let m_summary1 = validate_review_decisions("kurdish-hunspell-kmr", &temp_root)
        .expect("Pass 1 decision validation failed");
    let m_summary2 = validate_review_decisions("kurdish-hunspell-kmr", &temp_root)
        .expect("Pass 2 decision validation failed");
    assert_eq!(
        m_summary1.decision_file_sha256,
        m_summary2.decision_file_sha256
    );
}

#[test]
fn test_extra_unmanifested_queue_file_rejection() {
    let (_temp_dir, temp_root) = prepare_review_fixture();
    let queues_dir = temp_root.join("data/review-queues/kurdish-hunspell-kmr");

    let extra_file = queues_dir.join("unexpected-extra-file.jsonl");
    fs::write(&extra_file, "{\"invalid\":true}\n").unwrap();

    let res = validate_review_decisions("kurdish-hunspell-kmr", &temp_root);
    assert!(
        res.is_err(),
        "Extra unmanifested queue file must cause validation failure"
    );
    assert!(res.unwrap_err().contains("Unexpected file"));
}

#[test]
fn test_target_aware_metadata_change_identical_to_target_rejection() {
    let (_temp_dir, temp_root) = prepare_review_fixture();
    let decisions_file =
        temp_root.join("data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl");

    let h_path = temp_root.join("data/review-queues/kurdish-hunspell-kmr/hunspell-only.jsonl");
    let h_file = File::open(&h_path).unwrap();
    let reader = std::io::BufReader::new(h_file);
    let target_val = reader
        .lines()
        .map_while(Result::ok)
        .map(|l| serde_json::from_str::<serde_json::Value>(&l).unwrap())
        .find(|v| {
            let display = v.get("display").and_then(|x| x.as_str()).unwrap_or("");
            !display.is_empty() && display.chars().all(|c| c.is_alphabetic())
        })
        .expect("Should find an alphabetic entry in hunspell-only.jsonl");

    let target_id = target_val.get("target_id").unwrap().as_str().unwrap();
    let display = target_val.get("display").unwrap().as_str().unwrap();
    let flags = target_val.get("flags").unwrap().as_str().unwrap();
    let morph_arr: Vec<String> = target_val
        .get("morphology")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let morph_json = serde_json::to_string(&morph_arr).unwrap();

    let dec_json = format!(
        "{{\"schema_version\":\"review-decision-v1\",\"target_type\":\"entry\",\"target_id\":\"{}\",\"source_id\":\"kurdish-hunspell-kmr\",\"review_status\":\"approved_with_metadata_change\",\"reviewer_id\":\"test-user\",\"review_date\":\"2026-08-02\",\"replacement_metadata\":{{\"display\":\"{}\",\"normalized\":\"{}\",\"flags\":\"{}\",\"morphology\":{}}}}}\n",
        target_id, display, display, flags, morph_json
    );
    fs::write(&decisions_file, dec_json).unwrap();

    let res = validate_review_decisions("kurdish-hunspell-kmr", &temp_root);
    assert!(
        res.is_err(),
        "Identical replacement metadata must be rejected"
    );
    assert!(res
        .unwrap_err()
        .contains("identical to the original target entry"));
}

#[test]
fn test_orphan_decision_isolation_and_manifest_stale_rejection() {
    let (_temp_dir, temp_root) = prepare_review_fixture();
    let decisions_file =
        temp_root.join("data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl");

    let orphan_json = "{\"schema_version\":\"review-decision-v1\",\"target_type\":\"entry\",\"target_id\":\"non-existent-target-1234567890\",\"source_id\":\"kurdish-hunspell-kmr\",\"review_status\":\"approved\",\"reviewer_id\":\"test-user\",\"review_date\":\"2026-08-02\"}\n";
    fs::write(&decisions_file, orphan_json).unwrap();

    let res = validate_review_decisions("kurdish-hunspell-kmr", &temp_root)
        .expect("Validation with orphan decision should succeed");
    assert_eq!(res.orphan_decisions_count, 1);
    assert_eq!(
        res.approved_count, 0,
        "Orphan decisions must NOT be counted as approved!"
    );

    let queue_file =
        temp_root.join("data/review-queues/kurdish-hunspell-kmr/parser-rejections.jsonl");
    if queue_file.exists() {
        let original_content = fs::read_to_string(&queue_file).unwrap();
        fs::write(&queue_file, original_content.clone() + "\n// Tampered line").unwrap();

        let err_res = validate_review_decisions("kurdish-hunspell-kmr", &temp_root);
        assert!(
            err_res.is_err(),
            "Checksum mismatch must cause validate_review_decisions to fail"
        );
        assert!(err_res.unwrap_err().contains("Checksum mismatch"));
    }
}

#[test]
fn test_failed_installation_and_failed_rollback_error_reporting() {
    let (_temp_dir, temp_root) = prepare_review_fixture();
    let backup_dir = temp_root.join("data/reports/controlled-lexicon-review.tmp_backup");

    if backup_dir.exists() {
        let _ = fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o755));
        let _ = fs::remove_dir_all(&backup_dir);
    }

    let _ = validate_review_decisions("kurdish-hunspell-kmr", &temp_root)
        .expect("Initial validation failed");

    fs::create_dir_all(&backup_dir).unwrap();
    fs::write(backup_dir.join("read_only.txt"), "data").unwrap();
    fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let res = validate_review_decisions("kurdish-hunspell-kmr", &temp_root);
    assert!(
        res.is_err(),
        "Must fail when backup directory cannot be cleaned"
    );

    let _ = fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o755));
    let _ = fs::remove_dir_all(&backup_dir);
}
