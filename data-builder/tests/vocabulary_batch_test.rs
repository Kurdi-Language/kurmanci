use data_builder_lib::review::vocabulary_batch::{
    generate_vocabulary_review_batch, load_audit_flags, load_corpus_frequencies,
    load_existing_decision_target_ids,
};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

fn compute_file_sha256<P: AsRef<Path>>(path: P) -> String {
    let bytes = fs::read(path).expect("Failed to read file for sha256");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

fn create_mock_all_audit_queues(queue_dir: &Path) {
    let audit_files = [
        "metadata-conflict-groups.jsonl",
        "suspicious-entries.jsonl",
        "multiword-entries.jsonl",
        "capitalization-anomalies.jsonl",
        "mixed-scripts.jsonl",
        "unusual-scripts.jsonl",
        "rare-code-points.jsonl",
        "unexpected-code-points.jsonl",
        "parser-rejections.jsonl",
        "symbol-only.jsonl",
        "punctuation-only.jsonl",
        "digit-only.jsonl",
        "no-letter.jsonl",
        "short-and-long-forms.jsonl",
    ];
    for f in audit_files {
        let p = queue_dir.join(f);
        if !p.exists() {
            File::create(&p).unwrap();
        }
    }
}

#[test]
fn test_missing_frequencies_prerequisite_fails() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    fs::create_dir_all(&queue_dir).unwrap();
    create_mock_all_audit_queues(&queue_dir);

    let pool_file = queue_dir.join("hunspell-only.jsonl");
    let rec = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_1",
        "display": "peyv",
        "normalized": "peyv",
        "source_id": "kurdish-hunspell-kmr",
        "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "source_lines": [10],
        "flags": "N",
        "morphology": ["po:noun"],
        "part_of_speech": "noun",
        "reason_codes": ["HUNSPELL_ONLY_V1"],
        "suggested_action": "review_entry",
        "generated_status": "unreviewed",
        "effective_review_status": "unreviewed",
        "decision_entry_id": null,
        "queue_categories": ["hunspell_only"]
    });
    let mut pf = File::create(&pool_file).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&rec).unwrap()).unwrap();

    let freq_path = temp.path().join("data/build/frequencies.jsonl");
    let load_res = load_corpus_frequencies(&freq_path);
    assert!(load_res.is_err());
    let err_msg = load_res.unwrap_err();
    assert!(err_msg.contains("Corpus frequencies file missing"));
    assert!(err_msg.contains("build-frequencies"));

    let gen_res = generate_vocabulary_review_batch(temp.path());
    assert!(gen_res.is_err());
    assert!(gen_res.unwrap_err().contains("Corpus frequencies file missing"));
}

#[test]
fn test_missing_audit_queue_file_fails() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    fs::create_dir_all(&queue_dir).unwrap();
    // Do NOT create suspicious-entries.jsonl

    let res = load_audit_flags(&queue_dir);
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(err_msg.contains("Required audit queue file missing"));
    assert!(err_msg.contains("generate-review-queues"));
}

#[test]
fn test_malformed_audit_queue_json_fails_loudly() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    fs::create_dir_all(&queue_dir).unwrap();
    create_mock_all_audit_queues(&queue_dir);

    let susp_file = queue_dir.join("suspicious-entries.jsonl");
    let mut sf = File::create(&susp_file).unwrap();
    writeln!(sf, "{{\"target_id\": \"id_1\"}}").unwrap();
    writeln!(sf, "MALFORMED_JSON_LINE_HERE").unwrap();

    let res = load_audit_flags(&queue_dir);
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(err_msg.contains("suspicious-entries.jsonl"));
    assert!(err_msg.contains("line 2"));
    assert!(err_msg.contains("Failed parsing audit queue file"));
}

#[test]
fn test_audit_flag_join_and_ranking_priority() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    let build_dir = temp.path().join("data/build");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::create_dir_all(&build_dir).unwrap();
    create_mock_all_audit_queues(&queue_dir);

    let pool_file = queue_dir.join("hunspell-only.jsonl");
    let susp_file = queue_dir.join("suspicious-entries.jsonl");
    let freq_file = build_dir.join("frequencies.jsonl");

    let rec_clean = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_clean",
        "display": "clean_word",
        "normalized": "clean_word",
        "source_id": "kurdish-hunspell-kmr",
        "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "source_lines": [1],
        "flags": "N",
        "morphology": ["po:noun"],
        "part_of_speech": "noun",
        "reason_codes": ["HUNSPELL_ONLY_V1"],
        "suggested_action": "review_entry",
        "generated_status": "unreviewed",
        "effective_review_status": "unreviewed",
        "decision_entry_id": null,
        "queue_categories": ["hunspell_only"]
    });

    let rec_susp = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_susp",
        "display": "susp_word",
        "normalized": "susp_word",
        "source_id": "kurdish-hunspell-kmr",
        "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "source_lines": [2],
        "flags": "N",
        "morphology": ["po:noun"],
        "part_of_speech": "noun",
        "reason_codes": ["HUNSPELL_ONLY_V1"],
        "suggested_action": "review_entry",
        "generated_status": "unreviewed",
        "effective_review_status": "unreviewed",
        "decision_entry_id": null,
        "queue_categories": ["hunspell_only"]
    });

    let mut pf = File::create(&pool_file).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&rec_clean).unwrap()).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&rec_susp).unwrap()).unwrap();

    let susp_rec = serde_json::json!({ "target_id": "target_id_susp" });
    let mut sf = File::create(&susp_file).unwrap();
    writeln!(sf, "{}", serde_json::to_string(&susp_rec).unwrap()).unwrap();

    File::create(&freq_file).unwrap();

    let summary = generate_vocabulary_review_batch(temp.path()).unwrap();
    assert_eq!(summary.batch_size, 2);
    assert_eq!(summary.clean_candidates_count, 1);

    let jsonl_content = fs::read_to_string(temp.path().join("data/reports/vocabulary-review/top-1000.jsonl")).unwrap();
    let lines: Vec<&str> = jsonl_content.lines().collect();
    assert_eq!(lines.len(), 2);

    let rec1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(rec1["rank"], 1);
    assert_eq!(rec1["target_id"], "target_id_clean");
    assert_eq!(rec1["audit_flags"], serde_json::json!([]));

    let rec2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(rec2["rank"], 2);
    assert_eq!(rec2["target_id"], "target_id_susp");
    assert_eq!(rec2["audit_flags"], serde_json::json!(["suspicious_entry"]));
}

#[test]
fn test_existing_decisions_status_aware() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    let dec_dir = temp.path().join("data/review-decisions/kurdish-hunspell-kmr");
    let build_dir = temp.path().join("data/build");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::create_dir_all(&dec_dir).unwrap();
    fs::create_dir_all(&build_dir).unwrap();
    create_mock_all_audit_queues(&queue_dir);

    let pool_file = queue_dir.join("hunspell-only.jsonl");
    let dec_file = dec_dir.join("decisions.jsonl");
    let freq_file = build_dir.join("frequencies.jsonl");

    File::create(&freq_file).unwrap();

    let make_cand = |id: &str, word: &str, line_num: usize| {
        serde_json::json!({
            "schema_version": "review-queue-v1",
            "rule_id": "HUNSPELL_ONLY_V1",
            "rule_version": "1.0.0",
            "target_type": "entry",
            "target_id": id,
            "display": word,
            "normalized": word,
            "source_id": "kurdish-hunspell-kmr",
            "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
            "source_lines": [line_num],
            "flags": "N",
            "morphology": ["po:noun"],
            "part_of_speech": "noun",
            "reason_codes": ["HUNSPELL_ONLY_V1"],
            "suggested_action": "review_entry",
            "generated_status": "unreviewed",
            "effective_review_status": "unreviewed",
            "decision_entry_id": null,
            "queue_categories": ["hunspell_only"]
        })
    };

    let mut pf = File::create(&pool_file).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&make_cand("id_unreviewed", "peyv_unreviewed", 1)).unwrap()).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&make_cand("id_approved", "peyv_approved", 2)).unwrap()).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&make_cand("id_rejected", "peyv_rejected", 3)).unwrap()).unwrap();

    let dec_unreviewed = serde_json::json!({
        "schema_version": "review-decision-v1",
        "target_type": "entry",
        "target_id": "id_unreviewed",
        "source_id": "kurdish-hunspell-kmr",
        "review_status": "unreviewed"
    });

    let dec_approved = serde_json::json!({
        "schema_version": "review-decision-v1",
        "target_type": "entry",
        "target_id": "id_approved",
        "source_id": "kurdish-hunspell-kmr",
        "review_status": "approved",
        "reviewer_id": "test_user",
        "review_date": "2026-08-21"
    });

    let dec_rejected = serde_json::json!({
        "schema_version": "review-decision-v1",
        "target_type": "entry",
        "target_id": "id_rejected",
        "source_id": "kurdish-hunspell-kmr",
        "review_status": "rejected_from_default_pack",
        "reviewer_id": "test_user",
        "review_date": "2026-08-21"
    });

    let mut df = File::create(&dec_file).unwrap();
    writeln!(df, "{}", serde_json::to_string(&dec_unreviewed).unwrap()).unwrap();
    writeln!(df, "{}", serde_json::to_string(&dec_approved).unwrap()).unwrap();
    writeln!(df, "{}", serde_json::to_string(&dec_rejected).unwrap()).unwrap();

    let excluded = load_existing_decision_target_ids(&dec_file).unwrap();
    assert!(!excluded.contains("id_unreviewed"), "Unreviewed decision MUST NOT exclude candidate");
    assert!(excluded.contains("id_approved"), "Approved decision MUST exclude candidate");
    assert!(excluded.contains("id_rejected"), "Rejected decision MUST exclude candidate");

    let summary = generate_vocabulary_review_batch(temp.path()).unwrap();
    assert_eq!(summary.total_pool_candidates, 3);
    assert_eq!(summary.excluded_existing_decisions, 2);
    assert_eq!(summary.eligible_pending_candidates, 1);

    let jsonl_content = fs::read_to_string(temp.path().join("data/reports/vocabulary-review/top-1000.jsonl")).unwrap();
    let rec: serde_json::Value = serde_json::from_str(jsonl_content.lines().next().unwrap()).unwrap();
    assert_eq!(rec["target_id"], "id_unreviewed");
    assert_eq!(rec["source_revision"], "88131d6878ef7fa3ee114aa554adc385ff85b44c");
    assert_eq!(rec["source_lines"], serde_json::json!([1]));
}

#[test]
fn test_no_mutation_invariants_tempdir() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    let dec_dir = temp.path().join("data/review-decisions/kurdish-hunspell-kmr");
    let rev_dir = temp.path().join("data/reviewed");
    let build_dir = temp.path().join("data/build");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::create_dir_all(&dec_dir).unwrap();
    fs::create_dir_all(&rev_dir).unwrap();
    fs::create_dir_all(&build_dir).unwrap();

    create_mock_all_audit_queues(&queue_dir);

    let pool_path = queue_dir.join("hunspell-only.jsonl");
    let dec_path = dec_dir.join("decisions.jsonl");
    let lex_path = rev_dir.join("lexicon.jsonl");
    let freq_path = build_dir.join("frequencies.jsonl");

    let rec = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_mut",
        "display": "peyv",
        "normalized": "peyv",
        "source_id": "kurdish-hunspell-kmr",
        "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "source_lines": [1],
        "flags": "N",
        "morphology": ["po:noun"],
        "part_of_speech": "noun",
        "reason_codes": ["HUNSPELL_ONLY_V1"],
        "suggested_action": "review_entry",
        "generated_status": "unreviewed",
        "effective_review_status": "unreviewed",
        "decision_entry_id": null,
        "queue_categories": ["hunspell_only"]
    });
    let mut pf = File::create(&pool_path).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&rec).unwrap()).unwrap();

    let decision = serde_json::json!({
        "schema_version": "review-decision-v1",
        "target_type": "entry",
        "target_id": "dec_id",
        "source_id": "kurdish-hunspell-kmr",
        "review_status": "approved"
    });
    let mut df = File::create(&dec_path).unwrap();
    writeln!(df, "{}", serde_json::to_string(&decision).unwrap()).unwrap();

    let lex_item = serde_json::json!({
        "word": "ez",
        "lemma": "ez",
        "normalized": "ez",
        "part_of_speech": "pronoun",
        "frequency": 50000,
        "status": "verified",
        "variants": [],
        "sources": ["manual-seed"],
        "regions": ["general"]
    });
    let mut lf = File::create(&lex_path).unwrap();
    writeln!(lf, "{}", serde_json::to_string(&lex_item).unwrap()).unwrap();

    File::create(&freq_path).unwrap();

    let sha_queue_before = compute_file_sha256(&pool_path);
    let sha_dec_before = compute_file_sha256(&dec_path);
    let sha_lex_before = compute_file_sha256(&lex_path);

    generate_vocabulary_review_batch(temp.path()).unwrap();

    let sha_queue_after = compute_file_sha256(&pool_path);
    let sha_dec_after = compute_file_sha256(&dec_path);
    let sha_lex_after = compute_file_sha256(&lex_path);

    assert_eq!(sha_queue_before, sha_queue_after, "hunspell-only.jsonl must not be mutated!");
    assert_eq!(sha_dec_before, sha_dec_after, "decisions.jsonl must not be mutated!");
    assert_eq!(sha_lex_before, sha_lex_after, "data/reviewed/lexicon.jsonl must not be mutated!");
}
