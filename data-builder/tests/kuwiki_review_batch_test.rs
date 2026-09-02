//! Comprehensive integration tests for Kuwiki Vocabulary Review Batch Generator (`kuwiki-vocabulary-review-batch-v1`).

use data_builder_lib::corpus::vocabulary_evidence::{
    OovCandidateRecord, RepresentativeContext, VocabularyEvidenceProvenance,
    VocabularyEvidenceSummaryReport,
};
use data_builder_lib::review::kuwiki_batch::{
    generate_kuwiki_review_batch, KuwikiReviewBatchCandidate, KuwikiReviewBatchManifest,
};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;

fn calculate_bytes_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Helper to set up a fully valid mock repository environment in a temp dir.
fn setup_valid_mock_environment(temp_dir: &TempDir, record_count: usize) -> (String, String) {
    let root = temp_dir.path();

    // 1. corpora.toml
    let corpora_dir = root.join("data/source-registry");
    fs::create_dir_all(&corpora_dir).unwrap();
    let corpora_toml = r#"
[[corpora]]
corpus_id = "kuwiki"
corpus_name = "Kurmancî Wikipedia"
language = "ku-Latn"
license = "CC BY-SA 4.0"
license_url = "https://creativecommons.org"
url = "https://dumps.wikimedia.org/kuwiki/20260801/dump.xml.bz2"
version = "20260801"
description = "Wikipedia"
attribution = "Wikimedia"
notes = "notes"
document_format = "jsonl"
document_id_field = "page_id"
text_field = "text"

[[corpora.files]]
path = "data/imported/kuwiki/documents.jsonl"
sha256 = "mock_import_sha"
"#;
    let corpora_toml_path = corpora_dir.join("corpora.toml");
    fs::write(&corpora_toml_path, corpora_toml).unwrap();
    let reg_sha = calculate_bytes_sha256(corpora_toml.as_bytes());

    // 2. canonical manifest
    let canon_dir = root.join("data/imported-canonical");
    fs::create_dir_all(&canon_dir).unwrap();
    let canon_manifest = r#"{"schema_version":"canonical-import-v1"}"#;
    let canon_path = canon_dir.join("manifest.json");
    fs::write(&canon_path, canon_manifest).unwrap();
    let canon_sha = calculate_bytes_sha256(canon_manifest.as_bytes());

    // 3. partition manifest & train.jsonl
    let part_dir = root.join("data/build/corpus-partitions");
    fs::create_dir_all(&part_dir).unwrap();
    let part_manifest = r#"{"schema_version":"corpus-partition-v1"}"#;
    let part_path = part_dir.join("manifest.json");
    fs::write(&part_path, part_manifest).unwrap();
    let part_sha = calculate_bytes_sha256(part_manifest.as_bytes());

    let train_content = r#"{"corpus_id":"kuwiki","canonical_corpus_id":"kuwiki","document_id":"doc1","canonical_document_id":"doc1","text":"test"}"#;
    let train_path = part_dir.join("train.jsonl");
    fs::write(&train_path, train_content).unwrap();
    let train_sha = calculate_bytes_sha256(train_content.as_bytes());

    // 4. frequencies.jsonl & frequency_manifest.json
    let build_dir = root.join("data/build");
    fs::create_dir_all(&build_dir).unwrap();
    let freq_content = r#"{"word":"test","token_count":1,"document_count":1,"normalized_frequency":0.1,"zipf":1.0}"#;
    let freq_path = build_dir.join("frequencies.jsonl");
    fs::write(&freq_path, freq_content).unwrap();
    let freq_sha = calculate_bytes_sha256(freq_content.as_bytes());

    let freq_manifest = r#"{
        "schema_version": "frequency-build-v1",
        "partition_policy_version": "corpus-partition-v1",
        "canonical_manifest_sha256": "canon_sha_placeholder",
        "partition_manifest_sha256": "part_sha_placeholder",
        "train_partition_sha256": "train_sha_placeholder",
        "corpora_toml_sha256": "reg_sha_placeholder",
        "frequencies_artifact_sha256": "freq_sha_placeholder"
    }"#
    .replace("canon_sha_placeholder", &canon_sha)
    .replace("part_sha_placeholder", &part_sha)
    .replace("train_sha_placeholder", &train_sha)
    .replace("reg_sha_placeholder", &reg_sha)
    .replace("freq_sha_placeholder", &freq_sha);

    let freq_manifest_path = build_dir.join("frequency_manifest.json");
    fs::write(&freq_manifest_path, &freq_manifest).unwrap();
    let freq_manifest_sha = calculate_bytes_sha256(freq_manifest.as_bytes());

    // 5. Authoritative experimental-full lexicon & pack policy
    let pack_policy_content = r#"schema_version = "pack-policy-v1"
default_pack = "seed"

[packs.seed]
description = "Seed"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.reviewed]
description = "Reviewed"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.experimental-full]
description = "Experimental"
opt_in = true
allow_as_default = false
model_profile = "none"
"#;
    fs::write(root.join("data/pack-policy.toml"), pack_policy_content).unwrap();

    let seed_dir = root.join("data/seed");
    fs::create_dir_all(&seed_dir).unwrap();
    fs::write(seed_dir.join("lexicon.jsonl"), "").unwrap();

    let rev_dir = root.join("data/reviewed");
    fs::create_dir_all(&rev_dir).unwrap();
    fs::write(rev_dir.join("lexicon.jsonl"), "").unwrap();

    let dec_dir = root.join("data/review-decisions/kurdish-hunspell-kmr");
    fs::create_dir_all(&dec_dir).unwrap();
    fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();

    let queue_dir = root.join("data/review-queues/kurdish-hunspell-kmr");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("artifacts.sha256"), "").unwrap();

    let rep_dir = root.join("data/reports/controlled-lexicon-review");
    fs::create_dir_all(&rep_dir).unwrap();
    let sum_str = r#"{"schema_version":"controlled-review-report-v1","source_id":"kurdish-hunspell-kmr","total_decisions_count":0,"approved_count":0,"approved_with_metadata_change_count":0,"rejected_from_default_count":0,"experimental_only_count":0,"unresolved_count":0,"orphan_decisions_count":0,"decision_file_sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","provenance":{"decisions_sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","queue_manifest_sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","source_revision":"1.0","imported_lexicon_sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"}}"#;
    fs::write(rep_dir.join("summary.json"), sum_str).unwrap();
    fs::write(rep_dir.join("approved.jsonl"), "").unwrap();
    fs::write(rep_dir.join("rejected-from-default.jsonl"), "").unwrap();
    fs::write(rep_dir.join("experimental-only.jsonl"), "").unwrap();
    fs::write(rep_dir.join("unresolved.jsonl"), "").unwrap();
    fs::write(rep_dir.join("orphan-decisions.jsonl"), "").unwrap();
    fs::write(rep_dir.join("metadata-changes.jsonl"), "").unwrap();

    let empty_sha = calculate_bytes_sha256(b"");
    let sum_sha = calculate_bytes_sha256(sum_str.as_bytes());

    let art_content = format!(
        "{}  data/reports/controlled-lexicon-review/summary.json\n{}  data/reports/controlled-lexicon-review/approved.jsonl\n{}  data/reports/controlled-lexicon-review/rejected-from-default.jsonl\n{}  data/reports/controlled-lexicon-review/experimental-only.jsonl\n{}  data/reports/controlled-lexicon-review/unresolved.jsonl\n{}  data/reports/controlled-lexicon-review/orphan-decisions.jsonl\n{}  data/reports/controlled-lexicon-review/metadata-changes.jsonl\n",
        sum_sha, empty_sha, empty_sha, empty_sha, empty_sha, empty_sha, empty_sha
    );
    fs::write(rep_dir.join("artifacts.sha256"), art_content).unwrap();

    let sources_toml = r#"
[[sources]]
source_id = "manual-seed"
source_name = "Seed"
author = "Test"
license = "Apache-2.0"
license_url = "http://example.com"
url = "http://example.com"
version = "0.1.0"
redistribution = "allowed"
notes = "test notes"

[[sources.files]]
path = "data/reviewed/lexicon.jsonl"
sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

[[sources]]
source_id = "kurdish-hunspell-kmr"
source_name = "Hunspell"
author = "Test"
license = "CC-BY-SA-4.0"
license_url = "http://example.com"
url = "http://example.com"
version = "1.0"
redistribution = "allowed"
notes = "test notes"

[[sources.files]]
path = "data/reviewed/lexicon.jsonl"
sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
"#;
    fs::write(corpora_dir.join("sources.toml"), sources_toml).unwrap();

    let _ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();

    let exp_fingerprint = data_builder_lib::compute_experimental_lexicon_fingerprint(root).unwrap();

    // 6. Evidence reports directory & queue
    let evidence_dir = root.join("data/reports/vocabulary-evidence/kuwiki");
    fs::create_dir_all(&evidence_dir).unwrap();

    let queue_path = evidence_dir.join("oov-review-queue.jsonl");
    let mut queue_file = File::create(&queue_path).unwrap();

    let mut queue_bytes = Vec::new();

    for i in 1..=record_count {
        let token = format!("candidate_{}", i);
        let norm = format!("candidate_{}", i);

        let rec = OovCandidateRecord {
            schema_version: "oov-candidate-v1".to_string(),
            rank: i,
            token: token.clone(),
            normalized_token: norm.clone(),
            token_count: (10000 - i) as u64,
            document_count: (5000 - i / 2) as u64,
            normalized_frequency: 0.001,
            zipf_milli: 6500,
            in_seed: false,
            in_reviewed: false,
            in_experimental_full: false,
            corpus_id: "kuwiki".to_string(),
            evidence_class: "oov_candidate".to_string(),
            technical_filter_status: "eligible_for_review".to_string(),
            technical_filter_reason: "none".to_string(),
            representative_contexts: vec![RepresentativeContext {
                corpus_id: "kuwiki".to_string(),
                document_id: format!("data/imported/kuwiki/documents.jsonl:{}", i),
                snippet: format!("context snippet for {}", token),
            }],
        };

        let json = serde_json::to_string(&rec).unwrap();
        writeln!(queue_file, "{}", json).unwrap();
        writeln!(queue_bytes, "{}", json).unwrap();
    }
    drop(queue_file);

    let queue_sha = calculate_bytes_sha256(&queue_bytes);

    // 7. summary.json & artifacts.sha256
    let summary = VocabularyEvidenceSummaryReport {
        schema_version: "vocabulary-evidence-v1".to_string(),
        corpus_id: "kuwiki".to_string(),
        provenance: VocabularyEvidenceProvenance {
            corpus_registry_sha256: reg_sha,
            canonical_manifest_sha256: canon_sha,
            partition_manifest_sha256: part_sha,
            train_partition_sha256: train_sha,
            frequency_artifact_sha256: freq_sha,
            frequency_build_manifest_sha256: freq_manifest_sha,
            experimental_lexicon_fingerprint: exp_fingerprint.clone(),
        },
        total_unique_train_tokens: record_count + 10,
        total_oov_unique_tokens: record_count,
        eligible_oov_candidates: record_count,
        technical_noise_exclusions: 0,
        already_known_tokens: 10,
        raw_oov_distribution: Default::default(),
        eligible_oov_distribution: Default::default(),
    };

    let summary_bytes = serde_json::to_string_pretty(&summary).unwrap();
    let summary_sha = calculate_bytes_sha256(summary_bytes.as_bytes());

    let summary_path = evidence_dir.join("summary.json");
    fs::write(&summary_path, &summary_bytes).unwrap();

    let artifacts_path = evidence_dir.join("artifacts.sha256");
    let artifacts_content = format!(
        "{}  oov-review-queue.jsonl\n{}  summary.json\n",
        queue_sha, summary_sha
    );
    fs::write(&artifacts_path, &artifacts_content).unwrap();

    (queue_sha, exp_fingerprint)
}

#[test]
fn test_kuwiki_batch_size_contract_and_context_split() {
    let temp_dir = TempDir::new().unwrap();
    let (expected_queue_sha, expected_fingerprint) = setup_valid_mock_environment(&temp_dir, 1050);

    let summary = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000)
        .expect("Batch generation failed");

    assert_eq!(summary.batch_size, 1000);
    assert_eq!(summary.input_queue_sha256, expected_queue_sha);
    assert_eq!(summary.experimental_fingerprint, expected_fingerprint);

    // Verify committed directory structure
    let batch_dir = temp_dir.path().join("data/review-batches/kuwiki-batch-001");
    assert!(batch_dir.exists());

    // Assert review-guide.md is NOT committed in data/review-batches/kuwiki-batch-001/
    assert!(!batch_dir.join("review-guide.md").exists());

    // Assert candidates.jsonl contains context_references, NOT copyright snippet text
    let candidates_path = batch_dir.join("candidates.jsonl");
    let lines: Vec<String> = fs::read_to_string(&candidates_path)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

    assert_eq!(lines.len(), 1000);

    for (idx, line) in lines.iter().enumerate() {
        let cand: KuwikiReviewBatchCandidate = serde_json::from_str(line).unwrap();

        assert_eq!(cand.batch_rank, idx + 1);
        assert_eq!(cand.original_queue_rank, idx + 1);
        assert_eq!(cand.technical_filter_status, "eligible_for_review");
        assert_eq!(cand.technical_filter_reason, "none");
        assert_eq!(cand.decision_status, "pending");
        assert!(!cand.context_references.is_empty());

        // Verify JSON string line does NOT contain "snippet"
        assert!(!line.contains("\"snippet\""));
    }

    // Verify local review guide IS created in data/reports/vocabulary-review/kuwiki-batch-001/
    let local_guide = temp_dir
        .path()
        .join("data/reports/vocabulary-review/kuwiki-batch-001/review-guide.md");
    assert!(local_guide.exists());

    // Verify manifest.json
    let manifest_path = batch_dir.join("manifest.json");
    let manifest: KuwikiReviewBatchManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();

    assert_eq!(manifest.batch_id, "kuwiki-batch-001");
    assert_eq!(manifest.batch_size, 1000);
    assert_eq!(manifest.source_version, "20260801");
    assert_eq!(manifest.input_oov_review_queue_sha256, expected_queue_sha);
    assert_eq!(
        manifest.experimental_lexicon_fingerprint,
        expected_fingerprint
    );
}

#[test]
fn test_kuwiki_batch_insufficient_queue_size_contract_rejection() {
    let temp_dir = TempDir::new().unwrap();
    setup_valid_mock_environment(&temp_dir, 500); // Only 500 records

    let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);

    assert!(res.is_err());
    let err = res.err().unwrap();
    assert!(err.contains("exceeds total eligible queue records"));
}

#[test]
fn test_kuwiki_batch_unsupported_corpus_id_rejection() {
    let temp_dir = TempDir::new().unwrap();
    setup_valid_mock_environment(&temp_dir, 1000);

    let res =
        generate_kuwiki_review_batch(temp_dir.path(), "opensubtitles", "kuwiki-batch-001", 1000);

    assert!(res.is_err());
    let err = res.err().unwrap();
    assert!(err.contains("Unsupported corpus_id"));
}

#[test]
fn test_stale_provenance_and_registry_failures() {
    // 1. Mutate canonical manifest -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir
            .path()
            .join("data/imported-canonical/manifest.json");
        fs::write(&path, r#"{"mutated":true}"#).unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("canonical manifest"));
    }

    // 2. Mutate partition manifest -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir
            .path()
            .join("data/build/corpus-partitions/manifest.json");
        fs::write(&path, r#"{"mutated":true}"#).unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("partition manifest"));
    }

    // 3. Mutate train.jsonl -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir
            .path()
            .join("data/build/corpus-partitions/train.jsonl");
        fs::write(&path, r#"{"mutated":true}"#).unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("train partition"));
    }

    // 4. Mutate frequencies.jsonl -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir.path().join("data/build/frequencies.jsonl");
        fs::write(&path, r#"{"mutated":true}"#).unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("frequencies.jsonl"));
    }

    // 5. Mutate frequency_manifest.json -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir.path().join("data/build/frequency_manifest.json");
        fs::write(&path, r#"{"mutated":true}"#).unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("frequency_manifest.json"));
    }

    // 6. Missing frequency_manifest.json -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir.path().join("data/build/frequency_manifest.json");
        fs::remove_file(&path).unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        assert!(res
            .err()
            .unwrap()
            .contains("Required evidence input missing"));
    }

    // 7. Mutate summary.json without updating artifacts.sha256 -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir
            .path()
            .join("data/reports/vocabulary-evidence/kuwiki/summary.json");
        let content = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("// mutated\n{}", content)).unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("Stale summary artifact"));
    }

    // 8. Malformed corpora.toml -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir.path().join("data/source-registry/corpora.toml");
        fs::write(&path, "invalid toml === [[[").unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("Failed to parse corpora.toml"));
    }

    // 9. Missing kuwiki entry in corpora.toml -> fail
    {
        let temp_dir = TempDir::new().unwrap();
        setup_valid_mock_environment(&temp_dir, 1050);
        let path = temp_dir.path().join("data/source-registry/corpora.toml");
        fs::write(&path, "[[corpora]]\ncorpus_id = \"opensubtitles\"\ncorpus_name = \"OpenSubtitles\"\nlanguage = \"ku-Latn\"\nlicense = \"CC BY-SA 4.0\"\nlicense_url = \"http://example.com\"\nurl = \"http://example.com\"\nversion = \"1.0\"\ndescription = \"desc\"\nattribution = \"attr\"\nnotes = \"notes\"\ndocument_format = \"jsonl\"\nfiles = []\n").unwrap();
        let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(
            err.contains("missing from registry") || err.contains("missing in registry"),
            "Unexpected error: {}",
            err
        );
    }
}

#[test]
fn test_kuwiki_batch_mutated_queue_artifacts_manifest_rejection() {
    let temp_dir = TempDir::new().unwrap();
    setup_valid_mock_environment(&temp_dir, 1050);

    // Mutate oov-review-queue.jsonl after evidence generation
    let queue_path = temp_dir
        .path()
        .join("data/reports/vocabulary-evidence/kuwiki/oov-review-queue.jsonl");
    let content = fs::read_to_string(&queue_path).unwrap();
    fs::write(&queue_path, format!("// extra line\n{}", content)).unwrap();

    let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);

    assert!(res.is_err());
    let err = res.err().unwrap();
    assert!(err.contains("Stale queue artifact"));
}

#[test]
fn test_kuwiki_batch_rank_discontinuity_rejection() {
    let temp_dir = TempDir::new().unwrap();
    setup_valid_mock_environment(&temp_dir, 1050);

    let queue_path = temp_dir
        .path()
        .join("data/reports/vocabulary-evidence/kuwiki/oov-review-queue.jsonl");
    let lines: Vec<String> = fs::read_to_string(&queue_path)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

    // Mutate line 2 rank to 999
    let mut rec: OovCandidateRecord = serde_json::from_str(&lines[1]).unwrap();
    rec.rank = 999;

    let mut new_lines = lines.clone();
    new_lines[1] = serde_json::to_string(&rec).unwrap();

    let new_content = new_lines.join("\n") + "\n";
    let new_sha = calculate_bytes_sha256(new_content.as_bytes());

    fs::write(&queue_path, &new_content).unwrap();

    // Update artifacts.sha256 so artifacts check passes and rank check triggers
    let artifacts_path = temp_dir
        .path()
        .join("data/reports/vocabulary-evidence/kuwiki/artifacts.sha256");
    let summary_path = temp_dir
        .path()
        .join("data/reports/vocabulary-evidence/kuwiki/summary.json");
    let sum_sha = calculate_bytes_sha256(&fs::read(summary_path).unwrap());

    fs::write(
        &artifacts_path,
        format!(
            "{}  oov-review-queue.jsonl\n{}  summary.json\n",
            new_sha, sum_sha
        ),
    )
    .unwrap();

    let res = generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000);

    assert!(res.is_err());
    let err = res.err().unwrap();
    assert!(err.contains("Queue rank discontinuity"));
}

#[test]
fn test_kuwiki_batch_2run_byte_identical_determinism() {
    let temp_dir = TempDir::new().unwrap();
    setup_valid_mock_environment(&temp_dir, 1050);

    let _sum1 =
        generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000).unwrap();

    let batch_dir = temp_dir.path().join("data/review-batches/kuwiki-batch-001");
    let cand1 = fs::read(batch_dir.join("candidates.jsonl")).unwrap();
    let manifest1 = fs::read(batch_dir.join("manifest.json")).unwrap();
    let artifacts1 = fs::read(batch_dir.join("artifacts.sha256")).unwrap();

    let local_guide_path = temp_dir
        .path()
        .join("data/reports/vocabulary-review/kuwiki-batch-001/review-guide.md");
    let guide1 = fs::read(&local_guide_path).unwrap();

    // Re-run batch generation
    let _sum2 =
        generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000).unwrap();

    let cand2 = fs::read(batch_dir.join("candidates.jsonl")).unwrap();
    let manifest2 = fs::read(batch_dir.join("manifest.json")).unwrap();
    let artifacts2 = fs::read(batch_dir.join("artifacts.sha256")).unwrap();
    let guide2 = fs::read(&local_guide_path).unwrap();

    assert_eq!(
        cand1, cand2,
        "candidates.jsonl must be 100% byte-identical across runs"
    );
    assert_eq!(
        manifest1, manifest2,
        "manifest.json must be 100% byte-identical across runs"
    );
    assert_eq!(
        artifacts1, artifacts2,
        "artifacts.sha256 must be 100% byte-identical across runs"
    );
    assert_eq!(
        guide1, guide2,
        "local review-guide.md must be 100% byte-identical across runs"
    );
}

#[test]
fn test_kuwiki_batch_preserves_existing_decisions_and_vocabulary() {
    let temp_dir = TempDir::new().unwrap();
    setup_valid_mock_environment(&temp_dir, 1050);

    let decisions_dir = temp_dir
        .path()
        .join("data/review-decisions/kurdish-hunspell-kmr");
    fs::create_dir_all(&decisions_dir).unwrap();

    let dec_file = decisions_dir.join("decisions.jsonl");
    let mock_decision = r#"{"schema_version":"review-decision-v1","source_id":"kurdish-hunspell-kmr","target_type":"entry","target_id":"test_id","review_status":"approved"}"#;
    fs::write(&dec_file, format!("{}\n", mock_decision)).unwrap();

    let _sum =
        generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000).unwrap();

    let dec_after = fs::read_to_string(&dec_file).unwrap();
    assert_eq!(
        dec_after.trim(),
        mock_decision,
        "Human review decisions must NEVER be mutated by candidate batch generation"
    );
}

#[test]
fn test_kuwiki_decisions_snapshot_validation_and_counts() {
    use data_builder_lib::review::kuwiki_decisions::{
        load_and_validate_kuwiki_decisions, EXPECTED_APPROVED_COUNT,
        EXPECTED_DATE_POLICY_CONFIRMED_COUNT, EXPECTED_EXPERIMENTAL_ONLY_COUNT,
        EXPECTED_NEEDS_LINGUIST_COUNT, EXPECTED_REJECTED_FROM_DEFAULT_PACK_COUNT,
        EXPECTED_TOTAL_DECISIONS_COUNT,
    };

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let snapshot = load_and_validate_kuwiki_decisions(ws_root)
        .expect("Failed to load and validate kuwiki decisions")
        .expect("Kuwiki decisions snapshot missing");

    assert_eq!(snapshot.batch_id, "kuwiki-batch-001");
    assert_eq!(snapshot.candidates.len(), EXPECTED_TOTAL_DECISIONS_COUNT);
    assert_eq!(snapshot.decisions.len(), EXPECTED_TOTAL_DECISIONS_COUNT);

    let approved = *snapshot.counts_by_status.get("approved").unwrap_or(&0);
    let rejected = *snapshot
        .counts_by_status
        .get("rejected_from_default_pack")
        .unwrap_or(&0);
    let experimental = *snapshot
        .counts_by_status
        .get("experimental_only")
        .unwrap_or(&0);
    let needs_ling = *snapshot
        .counts_by_status
        .get("needs_linguist")
        .unwrap_or(&0);

    assert_eq!(approved, EXPECTED_APPROVED_COUNT);
    assert_eq!(rejected, EXPECTED_REJECTED_FROM_DEFAULT_PACK_COUNT);
    assert_eq!(experimental, EXPECTED_EXPERIMENTAL_ONLY_COUNT);
    assert_eq!(needs_ling, EXPECTED_NEEDS_LINGUIST_COUNT);

    let date_policy_count = snapshot
        .decisions
        .iter()
        .filter(|d| {
            d.review_notes
                .as_ref()
                .map(|n| {
                    n.to_lowercase()
                        .contains("human-confirmed date/year policy")
                })
                .unwrap_or(false)
        })
        .count();

    assert_eq!(date_policy_count, EXPECTED_DATE_POLICY_CONFIRMED_COUNT);
}

#[test]
fn test_kuwiki_pack_promotion_and_set_invariants() {
    use data_builder_lib::pack::builder::resolve_authoritative_pack_lexicon;
    use data_builder_lib::review::kuwiki_decisions::load_and_validate_kuwiki_decisions;
    use data_builder_lib::review::schema::compute_entry_id;
    use std::collections::BTreeSet;

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let snapshot = load_and_validate_kuwiki_decisions(ws_root)
        .unwrap()
        .unwrap();

    let seed_entries = resolve_authoritative_pack_lexicon("seed", ws_root).unwrap();
    let reviewed_entries = resolve_authoritative_pack_lexicon("reviewed", ws_root).unwrap();
    let exp_entries = resolve_authoritative_pack_lexicon("experimental-full", ws_root).unwrap();

    assert_eq!(seed_entries.len(), 33);
    assert_eq!(reviewed_entries.len(), 873); // 33 seed + 107 Hunspell + 733 Kuwiki
    assert_eq!(exp_entries.len(), 41842); // 41106 + 733 Kuwiki approved + 3 Kuwiki experimental

    let seed_set: BTreeSet<String> = seed_entries.iter().map(|e| e.normalized.clone()).collect();
    let reviewed_set: BTreeSet<String> = reviewed_entries
        .iter()
        .map(|e| e.normalized.clone())
        .collect();
    let exp_set: BTreeSet<String> = exp_entries.iter().map(|e| e.normalized.clone()).collect();

    // Invariant 1: seed ⊆ reviewed ⊆ experimental-full
    for s in &seed_set {
        assert!(
            reviewed_set.contains(s),
            "Seed entry '{}' missing from reviewed pack",
            s
        );
        assert!(
            exp_set.contains(s),
            "Seed entry '{}' missing from experimental-full pack",
            s
        );
    }
    for r in &reviewed_set {
        assert!(
            exp_set.contains(r),
            "Reviewed entry '{}' missing from experimental-full pack",
            r
        );
    }

    // Map Kuwiki target_id -> review_status & candidate
    let mut approved_ku_tokens = BTreeSet::new();
    let mut experimental_ku_tokens = BTreeSet::new();
    let mut excluded_ku_tokens = BTreeSet::new();

    let dec_map: std::collections::BTreeMap<
        String,
        &data_builder_lib::review::schema::ReviewDecisionRecord,
    > = snapshot
        .decisions
        .iter()
        .map(|d| (d.target_id.clone(), d))
        .collect();

    for cand in &snapshot.candidates {
        let tid = compute_entry_id(
            "kuwiki-batch-001",
            "23d3871a8f6ef285ba9b6f231fe5d65f201934eaee2965d18cdec7770aeb3c1d",
            &cand.token,
            &cand.normalized_token,
            "",
            &[],
        )
        .unwrap();
        let dec = dec_map
            .get(&tid)
            .expect("Decision missing for candidate target_id");
        match dec.review_status {
            data_builder_lib::ReviewDecisionStatus::Approved => {
                approved_ku_tokens.insert(cand.normalized_token.clone());
            }
            data_builder_lib::ReviewDecisionStatus::ExperimentalOnly => {
                experimental_ku_tokens.insert(cand.normalized_token.clone());
            }
            data_builder_lib::ReviewDecisionStatus::RejectedFromDefaultPack
            | data_builder_lib::ReviewDecisionStatus::NeedsLinguist => {
                excluded_ku_tokens.insert(cand.normalized_token.clone());
            }
            _ => {}
        }
    }

    // Invariant 2: All 733 approved Kuwiki entries are present in reviewed and experimental-full
    for app in &approved_ku_tokens {
        assert!(
            reviewed_set.contains(app),
            "Approved Kuwiki token '{}' missing from reviewed pack",
            app
        );
        assert!(
            exp_set.contains(app),
            "Approved Kuwiki token '{}' missing from experimental-full pack",
            app
        );

        // Verify technical fallback metadata for Kuwiki entry in reviewed pack
        let entry = reviewed_entries
            .iter()
            .find(|e| e.normalized == *app)
            .unwrap();
        assert_eq!(
            entry.part_of_speech, "unknown",
            "POS fallback must be 'unknown'"
        );
        assert_eq!(
            entry.lemma, entry.word,
            "Lemma fallback must be display token"
        );
    }

    // Invariant 3: Experimental-only entries present in experimental-full, NOT in reviewed
    for exp in &experimental_ku_tokens {
        assert!(
            !reviewed_set.contains(exp),
            "Experimental-only token '{}' should NOT be in reviewed pack",
            exp
        );
        assert!(
            exp_set.contains(exp),
            "Experimental-only token '{}' missing from experimental-full pack",
            exp
        );
    }

    // Invariant 4: Rejected & needs_linguist entries NOT in reviewed or experimental-full
    for excl in &excluded_ku_tokens {
        assert!(
            !reviewed_set.contains(excl),
            "Excluded Kuwiki token '{}' found in reviewed pack",
            excl
        );
        assert!(
            !exp_set.contains(excl),
            "Excluded Kuwiki token '{}' found in experimental-full pack",
            excl
        );
    }
}

#[test]
fn test_kuwiki_pack_build_two_pass_determinism() {
    use data_builder_lib::pack::builder::build_pack;

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let m1 = build_pack("reviewed", ws_root).unwrap();
    let m2 = build_pack("reviewed", ws_root).unwrap();

    assert_eq!(
        m1.binary_sha256, m2.binary_sha256,
        "Binary pack SHA-256 must be 100% byte-identical across passes"
    );
    assert_eq!(m1.final_unique_entry_count, m2.final_unique_entry_count);
    assert_eq!(m1.source_provenance, m2.source_provenance);

    let e1 = build_pack("experimental-full", ws_root).unwrap();
    let e2 = build_pack("experimental-full", ws_root).unwrap();

    assert_eq!(
        e1.binary_sha256, e2.binary_sha256,
        "Experimental-full binary pack SHA-256 must be 100% byte-identical across passes"
    );
    assert_eq!(e1.final_unique_entry_count, e2.final_unique_entry_count);
    assert_eq!(e1.source_provenance, e2.source_provenance);
}

#[test]
fn test_kuwiki_decisions_negative_validation_cases() {
    use data_builder_lib::review::kuwiki_decisions::load_and_validate_kuwiki_decisions;

    fn make_mock_kuwiki_repo(root: &std::path::Path) {
        let reg_dir = root.join("data/source-registry");
        fs::create_dir_all(&reg_dir).unwrap();
        let sources_toml = r#"
[[sources]]
source_id = "kuwiki-batch-001"
source_name = "Kuwiki"
author = "Wikimedia"
license = "CC BY-SA 4.0"
license_url = "https://creativecommons.org"
url = "https://dumps.wikimedia.org"
version = "4941c9c26dd5d242f4bd4e00e45dfcf0c681ff30"
redistribution = "allowed"
notes = "test"
"#;
        fs::write(reg_dir.join("sources.toml"), sources_toml).unwrap();

        let batch_dir = root.join("data/review-batches/kuwiki-batch-001");
        let dec_dir = root.join("data/review-decisions/kuwiki-batch-001");
        fs::create_dir_all(&batch_dir).unwrap();
        fs::create_dir_all(&dec_dir).unwrap();
    }

    // Case 1: Registered kuwiki source + missing candidates -> fail
    {
        let temp1 = TempDir::new().unwrap();
        make_mock_kuwiki_repo(temp1.path());
        let dec_dir = temp1.path().join("data/review-decisions/kuwiki-batch-001");
        fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();
        fs::write(dec_dir.join("manifest.json"), "").unwrap();
        let batch_dir = temp1.path().join("data/review-batches/kuwiki-batch-001");
        fs::write(batch_dir.join("manifest.json"), "").unwrap();
        fs::write(batch_dir.join("artifacts.sha256"), "").unwrap();

        let res = load_and_validate_kuwiki_decisions(temp1.path());
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("candidate file missing"));
    }

    // Case 2: Registered kuwiki source + missing decisions -> fail
    {
        let temp2 = TempDir::new().unwrap();
        make_mock_kuwiki_repo(temp2.path());
        let batch_dir = temp2.path().join("data/review-batches/kuwiki-batch-001");
        fs::write(batch_dir.join("candidates.jsonl"), "").unwrap();
        fs::write(batch_dir.join("manifest.json"), "").unwrap();
        fs::write(batch_dir.join("artifacts.sha256"), "").unwrap();
        let dec_dir = temp2.path().join("data/review-decisions/kuwiki-batch-001");
        fs::write(dec_dir.join("manifest.json"), "").unwrap();

        let res = load_and_validate_kuwiki_decisions(temp2.path());
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("decision file missing"));
    }

    // Case 3: Missing batch manifest -> fail
    {
        let temp3 = TempDir::new().unwrap();
        make_mock_kuwiki_repo(temp3.path());
        let batch_dir = temp3.path().join("data/review-batches/kuwiki-batch-001");
        fs::write(batch_dir.join("candidates.jsonl"), "").unwrap();
        fs::write(batch_dir.join("artifacts.sha256"), "").unwrap();
        let dec_dir = temp3.path().join("data/review-decisions/kuwiki-batch-001");
        fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();
        fs::write(dec_dir.join("manifest.json"), "").unwrap();

        let res = load_and_validate_kuwiki_decisions(temp3.path());
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("batch manifest missing"));
    }

    // Case 4: Missing artifacts.sha256 -> fail
    {
        let temp4 = TempDir::new().unwrap();
        make_mock_kuwiki_repo(temp4.path());
        let batch_dir = temp4.path().join("data/review-batches/kuwiki-batch-001");
        fs::write(batch_dir.join("candidates.jsonl"), "").unwrap();
        fs::write(batch_dir.join("manifest.json"), "").unwrap();
        let dec_dir = temp4.path().join("data/review-decisions/kuwiki-batch-001");
        fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();
        fs::write(dec_dir.join("manifest.json"), "").unwrap();

        let res = load_and_validate_kuwiki_decisions(temp4.path());
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("artifacts.sha256 missing"));
    }

    // Case 5: Missing decision provenance manifest -> fail
    {
        let temp5 = TempDir::new().unwrap();
        make_mock_kuwiki_repo(temp5.path());
        let batch_dir = temp5.path().join("data/review-batches/kuwiki-batch-001");
        fs::write(batch_dir.join("candidates.jsonl"), "").unwrap();
        fs::write(batch_dir.join("manifest.json"), "").unwrap();
        fs::write(batch_dir.join("artifacts.sha256"), "").unwrap();
        let dec_dir = temp5.path().join("data/review-decisions/kuwiki-batch-001");
        fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();

        let res = load_and_validate_kuwiki_decisions(temp5.path());
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("provenance manifest missing"));
    }

    // Case 6: Tampered candidates hash -> fail
    {
        let temp6 = TempDir::new().unwrap();
        make_mock_kuwiki_repo(temp6.path());
        let batch_dir = temp6.path().join("data/review-batches/kuwiki-batch-001");
        fs::write(batch_dir.join("candidates.jsonl"), "tampered content").unwrap();
        fs::write(batch_dir.join("manifest.json"), "").unwrap();
        fs::write(batch_dir.join("artifacts.sha256"), "").unwrap();
        let dec_dir = temp6.path().join("data/review-decisions/kuwiki-batch-001");
        fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();
        fs::write(dec_dir.join("manifest.json"), "").unwrap();

        let res = load_and_validate_kuwiki_decisions(temp6.path());
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("SHA-256 mismatch"));
    }
}

#[test]
fn test_kuwiki_decisions_reordering_preserves_semantics() {
    use data_builder_lib::pack::selection::SelectionCounts;
    use data_builder_lib::review::kuwiki_decisions::{
        load_and_validate_kuwiki_decisions, select_kuwiki_candidates_for_pack,
    };

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();

    let snapshot = load_and_validate_kuwiki_decisions(ws_root)
        .unwrap()
        .unwrap();

    let mut counts_normal = SelectionCounts::default();
    let selected_normal =
        select_kuwiki_candidates_for_pack("reviewed", &snapshot, &mut counts_normal).unwrap();

    // Create snapshot variant with reversed decisions array
    let mut snapshot_reversed = snapshot.clone();
    snapshot_reversed.decisions.reverse();

    let mut counts_reversed = SelectionCounts::default();
    let selected_reversed =
        select_kuwiki_candidates_for_pack("reviewed", &snapshot_reversed, &mut counts_reversed)
            .unwrap();

    assert_eq!(selected_normal.len(), selected_reversed.len());
    assert_eq!(
        counts_normal.external_approved_selected,
        counts_reversed.external_approved_selected
    );

    for (a, b) in selected_normal.iter().zip(selected_reversed.iter()) {
        assert_eq!(a.entry_id, b.entry_id);
        assert_eq!(a.normalized, b.normalized);
        assert_eq!(a.status, b.status);
    }
}

#[test]
fn test_kuwiki_decisions_date_policy_wrong_target_id_rejection() {
    use data_builder_lib::review::kuwiki_decisions::{
        load_and_validate_kuwiki_decisions, validate_kuwiki_decision_records,
    };

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();

    let snapshot = load_and_validate_kuwiki_decisions(ws_root)
        .unwrap()
        .unwrap();

    let mut tampered_decisions = snapshot.decisions.clone();

    // Find a date/year policy decision (e.g. rank 608) and swap target_id with decision 0 (rank 1)
    let mut date_policy_idx = None;
    for (idx, dec) in tampered_decisions.iter().enumerate() {
        let notes_combined = format!(
            "{} {}",
            dec.review_notes.as_deref().unwrap_or_default(),
            serde_json::to_string(&dec.evidence).unwrap_or_default()
        );
        if notes_combined
            .to_lowercase()
            .contains("human-confirmed date/year policy")
        {
            date_policy_idx = Some(idx);
            break;
        }
    }
    let date_idx = date_policy_idx.unwrap();
    let tid0 = tampered_decisions[0].target_id.clone();
    let tid_date = tampered_decisions[date_idx].target_id.clone();

    tampered_decisions[0].target_id = tid_date;
    tampered_decisions[date_idx].target_id = tid0;

    let res = validate_kuwiki_decision_records(&snapshot.candidates, &tampered_decisions);
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(err_msg.contains("Date/year policy ranks set mismatch"));
}
