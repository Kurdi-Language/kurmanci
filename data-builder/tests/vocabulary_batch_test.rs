use data_builder_lib::review::vocabulary_batch::{
    generate_vocabulary_review_batch, load_audit_flags, load_corpus_frequencies,
    load_existing_decision_target_ids,
};
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_missing_frequencies_prerequisite_fails() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    fs::create_dir_all(&queue_dir).unwrap();

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
fn test_malformed_audit_queue_json_fails_loudly() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    fs::create_dir_all(&queue_dir).unwrap();

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
fn test_existing_decisions_status_aware() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    let dec_dir = temp.path().join("data/review-decisions/kurdish-hunspell-kmr");
    let build_dir = temp.path().join("data/build");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::create_dir_all(&dec_dir).unwrap();
    fs::create_dir_all(&build_dir).unwrap();

    let pool_file = queue_dir.join("hunspell-only.jsonl");
    let dec_file = dec_dir.join("decisions.jsonl");
    let freq_file = build_dir.join("frequencies.jsonl");

    File::create(&freq_file).unwrap(); // empty frequencies file is valid

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
fn test_frequency_joins_and_zero_frequency_candidates() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    let build_dir = temp.path().join("data/build");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::create_dir_all(&build_dir).unwrap();

    let pool_file = queue_dir.join("hunspell-only.jsonl");
    let freq_file = build_dir.join("frequencies.jsonl");

    let rec_with_freq = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_freq",
        "display": "kurmancî",
        "normalized": "kurmancî",
        "source_id": "kurdish-hunspell-kmr",
        "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "source_lines": [42],
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

    let rec_zero_freq = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_zero",
        "display": "neasan",
        "normalized": "neasan",
        "source_id": "kurdish-hunspell-kmr",
        "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "source_lines": [99],
        "flags": "A",
        "morphology": ["po:adj"],
        "part_of_speech": "adjective",
        "reason_codes": ["HUNSPELL_ONLY_V1"],
        "suggested_action": "review_entry",
        "generated_status": "unreviewed",
        "effective_review_status": "unreviewed",
        "decision_entry_id": null,
        "queue_categories": ["hunspell_only"]
    });

    let mut pf = File::create(&pool_file).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&rec_with_freq).unwrap()).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&rec_zero_freq).unwrap()).unwrap();

    let freq_data = serde_json::json!({
        "word": "kurmancî",
        "token_count": 100,
        "document_count": 50,
        "zipf": 8.5
    });
    let mut ff = File::create(&freq_file).unwrap();
    writeln!(ff, "{}", serde_json::to_string(&freq_data).unwrap()).unwrap();

    let freqs = load_corpus_frequencies(&freq_file).unwrap();
    assert_eq!(freqs.get("kurmancî"), Some(&(100, 50, 8.5)));
    assert_eq!(freqs.get("neasan"), None);

    let summary = generate_vocabulary_review_batch(temp.path()).unwrap();
    assert_eq!(summary.batch_size, 2);
    assert_eq!(summary.corpus_matched_count, 1);

    let jsonl_content = fs::read_to_string(temp.path().join("data/reports/vocabulary-review/top-1000.jsonl")).unwrap();
    let lines: Vec<&str> = jsonl_content.lines().collect();
    assert_eq!(lines.len(), 2);

    let rec1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(rec1["rank"], 1);
    assert_eq!(rec1["form"], "kurmancî");
    assert_eq!(rec1["source_revision"], "88131d6878ef7fa3ee114aa554adc385ff85b44c");
    assert_eq!(rec1["source_lines"], serde_json::json!([42]));
    assert_eq!(rec1["token_count"], 100);
    assert_eq!(rec1["document_count"], 50);
    assert_eq!(rec1["zipf"], 8.5);

    let rec2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(rec2["rank"], 2);
    assert_eq!(rec2["form"], "neasan");
    assert_eq!(rec2["source_lines"], serde_json::json!([99]));
    assert_eq!(rec2["token_count"], 0);
    assert_eq!(rec2["document_count"], 0);
    assert_eq!(rec2["zipf"], 0.0);
}

#[test]
fn test_parallel_safe_reproducibility_in_tempdir() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    let build_dir = temp.path().join("data/build");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::create_dir_all(&build_dir).unwrap();

    let pool_file = queue_dir.join("hunspell-only.jsonl");
    let freq_file = build_dir.join("frequencies.jsonl");

    let rec = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_rep",
        "display": "peyv_rep",
        "normalized": "peyv_rep",
        "source_id": "kurdish-hunspell-kmr",
        "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "source_lines": [100],
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
    File::create(&freq_file).unwrap();

    let summary1 = generate_vocabulary_review_batch(temp.path()).unwrap();
    let tsv1 = fs::read(temp.path().join("data/reports/vocabulary-review/top-1000.tsv")).unwrap();
    let jsonl1 = fs::read(temp.path().join("data/reports/vocabulary-review/top-1000.jsonl")).unwrap();

    let summary2 = generate_vocabulary_review_batch(temp.path()).unwrap();
    let tsv2 = fs::read(temp.path().join("data/reports/vocabulary-review/top-1000.tsv")).unwrap();
    let jsonl2 = fs::read(temp.path().join("data/reports/vocabulary-review/top-1000.jsonl")).unwrap();

    assert_eq!(summary1.batch_size, summary2.batch_size);
    assert_eq!(tsv1, tsv2, "TSV output must be byte-identical on repeated runs");
    assert_eq!(jsonl1, jsonl2, "JSONL output must be byte-identical on repeated runs");
}
