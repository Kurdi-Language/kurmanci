use data_builder_lib::review::vocabulary_batch::{
    generate_vocabulary_review_batch, load_audit_flags, load_corpus_frequencies,
    load_existing_decision_target_ids,
};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn find_repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    if dir.ends_with("data-builder") {
        dir.pop();
    }
    dir
}

fn compute_file_sha256<P: AsRef<Path>>(path: P) -> String {
    let bytes = fs::read(path).expect("Failed to read file for sha256");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

#[test]
fn test_existing_decisions_excluded() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    let dec_dir = temp.path().join("data/review-decisions/kurdish-hunspell-kmr");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::create_dir_all(&dec_dir).unwrap();

    let pool_file = queue_dir.join("hunspell-only.jsonl");
    let dec_file = dec_dir.join("decisions.jsonl");

    let rec_a = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_a",
        "display": "peyv_a",
        "normalized": "peyv_a",
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

    let rec_b = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_b",
        "display": "peyv_b",
        "normalized": "peyv_b",
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
    writeln!(pf, "{}", serde_json::to_string(&rec_a).unwrap()).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&rec_b).unwrap()).unwrap();

    let decision_a = serde_json::json!({
        "schema_version": "review-decision-v1",
        "target_type": "entry",
        "target_id": "target_id_a",
        "source_id": "kurdish-hunspell-kmr",
        "review_status": "approved",
        "reviewer_id": "test_reviewer",
        "review_date": "2026-08-21"
    });
    let mut df = File::create(&dec_file).unwrap();
    writeln!(df, "{}", serde_json::to_string(&decision_a).unwrap()).unwrap();

    let excluded = load_existing_decision_target_ids(&dec_file).unwrap();
    assert!(excluded.contains("target_id_a"));

    let summary = generate_vocabulary_review_batch(temp.path()).unwrap();
    assert_eq!(summary.total_pool_candidates, 2);
    assert_eq!(summary.excluded_existing_decisions, 1);
    assert_eq!(summary.eligible_pending_candidates, 1);
    assert_eq!(summary.batch_size, 1);
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
        "source_lines": [2],
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
    assert_eq!(rec1["token_count"], 100);
    assert_eq!(rec1["document_count"], 50);
    assert_eq!(rec1["zipf"], 8.5);

    let rec2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(rec2["rank"], 2);
    assert_eq!(rec2["form"], "neasan");
    assert_eq!(rec2["token_count"], 0);
    assert_eq!(rec2["document_count"], 0);
    assert_eq!(rec2["zipf"], 0.0);
}

#[test]
fn test_audit_flag_joins() {
    let temp = TempDir::new().unwrap();
    let queue_dir = temp.path().join("data/review-queues/kurdish-hunspell-kmr");
    fs::create_dir_all(&queue_dir).unwrap();

    let pool_file = queue_dir.join("hunspell-only.jsonl");
    let susp_file = queue_dir.join("suspicious-entries.jsonl");

    let rec_susp = serde_json::json!({
        "schema_version": "review-queue-v1",
        "rule_id": "HUNSPELL_ONLY_V1",
        "rule_version": "1.0.0",
        "target_type": "entry",
        "target_id": "target_id_susp",
        "display": "test_susp",
        "normalized": "test_susp",
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
    let mut pf = File::create(&pool_file).unwrap();
    writeln!(pf, "{}", serde_json::to_string(&rec_susp).unwrap()).unwrap();

    let susp_rec = serde_json::json!({
        "target_id": "target_id_susp"
    });
    let mut sf = File::create(&susp_file).unwrap();
    writeln!(sf, "{}", serde_json::to_string(&susp_rec).unwrap()).unwrap();

    let audit_flags = load_audit_flags(&queue_dir).unwrap();
    assert!(audit_flags.contains_key("target_id_susp"));
    assert!(audit_flags.get("target_id_susp").unwrap().contains("suspicious_entry"));

    let summary = generate_vocabulary_review_batch(temp.path()).unwrap();
    assert_eq!(summary.batch_size, 1);
    assert_eq!(summary.clean_candidates_count, 0);

    let jsonl_content = fs::read_to_string(temp.path().join("data/reports/vocabulary-review/top-1000.jsonl")).unwrap();
    let rec: serde_json::Value = serde_json::from_str(jsonl_content.lines().next().unwrap()).unwrap();
    assert_eq!(rec["audit_flags"], serde_json::json!(["suspicious_entry"]));
}

#[test]
fn test_deterministic_ordering_byte_identical() {
    let repo_root = find_repo_root();
    let summary1 = generate_vocabulary_review_batch(&repo_root).unwrap();
    let tsv1 = fs::read(repo_root.join("data/reports/vocabulary-review/top-1000.tsv")).unwrap();
    let jsonl1 = fs::read(repo_root.join("data/reports/vocabulary-review/top-1000.jsonl")).unwrap();

    let summary2 = generate_vocabulary_review_batch(&repo_root).unwrap();
    let tsv2 = fs::read(repo_root.join("data/reports/vocabulary-review/top-1000.tsv")).unwrap();
    let jsonl2 = fs::read(repo_root.join("data/reports/vocabulary-review/top-1000.jsonl")).unwrap();

    assert_eq!(summary1.batch_size, summary2.batch_size);
    assert_eq!(summary1.batch_size, 1000);
    assert_eq!(tsv1, tsv2, "TSV output must be byte-identical on repeated runs");
    assert_eq!(jsonl1, jsonl2, "JSONL output must be byte-identical on repeated runs");
}

#[test]
fn test_no_mutation_invariants() {
    let repo_root = find_repo_root();

    let queue_path = repo_root.join("data/review-queues/kurdish-hunspell-kmr/hunspell-only.jsonl");
    let decisions_path = repo_root.join("data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl");
    let lexicon_path = repo_root.join("data/reviewed/lexicon.jsonl");

    let sha_queue_before = compute_file_sha256(&queue_path);
    let sha_decisions_before = compute_file_sha256(&decisions_path);
    let sha_lexicon_before = compute_file_sha256(&lexicon_path);

    generate_vocabulary_review_batch(&repo_root).unwrap();

    let sha_queue_after = compute_file_sha256(&queue_path);
    let sha_decisions_after = compute_file_sha256(&decisions_path);
    let sha_lexicon_after = compute_file_sha256(&lexicon_path);

    assert_eq!(sha_queue_before, sha_queue_after, "hunspell-only.jsonl must not be mutated!");
    assert_eq!(sha_decisions_before, sha_decisions_after, "decisions.jsonl must not be mutated!");
    assert_eq!(sha_lexicon_before, sha_lexicon_after, "data/reviewed/lexicon.jsonl must not be mutated!");
}
