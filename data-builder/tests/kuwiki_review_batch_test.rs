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
license_spdx = "CC-BY-SA-4.0"
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

    // Assert obsolete review-guide.md is NOT created
    let local_guide = temp_dir
        .path()
        .join("data/reports/vocabulary-review/kuwiki-batch-001/review-guide.md");
    assert!(!local_guide.exists());

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
    assert!(err.contains("exceeds remaining eligible queue records"));
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
        fs::write(&path, "[[corpora]]\ncorpus_id = \"opensubtitles\"\ncorpus_name = \"OpenSubtitles\"\nlanguage = \"ku-Latn\"\nlicense = \"CC BY-SA 4.0\"\nlicense_spdx = \"CC-BY-SA-4.0\"\nlicense_url = \"http://example.com\"\nurl = \"http://example.com\"\nversion = \"1.0\"\ndescription = \"desc\"\nattribution = \"attr\"\nnotes = \"notes\"\ndocument_format = \"jsonl\"\nfiles = []\n").unwrap();
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

    // Re-run batch generation
    let _sum2 =
        generate_kuwiki_review_batch(temp_dir.path(), "kuwiki", "kuwiki-batch-001", 1000).unwrap();

    let cand2 = fs::read(batch_dir.join("candidates.jsonl")).unwrap();
    let manifest2 = fs::read(batch_dir.join("manifest.json")).unwrap();
    let artifacts2 = fs::read(batch_dir.join("artifacts.sha256")).unwrap();

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
    assert_eq!(reviewed_entries.len(), 2144); // 33 seed + 800 Hunspell (107 + Review Desk batch 001: 698 approved, 8 metadata change, less seed/collision overlap) + 721 Kuwiki batch 001 + 590 Kuwiki batch 002
    assert_eq!(exp_entries.len(), 42249); // Hunspell reservoir after 93 rejections and 466 needs-linguist exclusions + 721 Kuwiki b1 app + 3 b1 exp + 590 b2 app + 2 b2 exp

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
            "84a1439f28d95f978e9a7b84bc9ee946de2825489aee084bade85e255e156166",
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

    // Invariant 2: every approved Kuwiki entry is present in reviewed and experimental-full
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

        // Verify technical fallback metadata for Kuwiki entry in reviewed pack. When the same
        // normalized form is also a human-approved Hunspell entry (Review Desk batches),
        // collision resolution keeps the Hunspell record with its real metadata, so the
        // fallback applies only to entries backed by Kuwiki alone.
        let entry = reviewed_entries
            .iter()
            .find(|e| e.normalized == *app)
            .unwrap();
        let hunspell_backed = entry.sources.iter().any(|s| s == "kurdish-hunspell-kmr");
        if !hunspell_backed {
            assert_eq!(
                entry.part_of_speech, "unknown",
                "POS fallback must be 'unknown' for {}",
                app
            );
            assert_eq!(
                entry.lemma, entry.word,
                "Lemma fallback must be display token for {}",
                app
            );
        }
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
        let manifest_content = "";
        let manifest_sha = calculate_bytes_sha256(manifest_content.as_bytes());
        let orig_cand_sha = calculate_bytes_sha256(b"original content");
        let art_sha_content = format!(
            "{}  candidates.jsonl\n{}  manifest.json\n",
            orig_cand_sha, manifest_sha
        );
        fs::write(batch_dir.join("artifacts.sha256"), art_sha_content).unwrap();
        let dec_dir = temp6.path().join("data/review-decisions/kuwiki-batch-001");
        fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();
        fs::write(dec_dir.join("manifest.json"), "").unwrap();

        let res = load_and_validate_kuwiki_decisions(temp6.path());
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("Mismatched hash") || err.contains("mismatch"));
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
    use data_builder_lib::review::kuwiki_decisions::load_and_validate_kuwiki_decisions;

    let temp = TempDir::new().unwrap();
    let root = temp.path();

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();

    fs::create_dir_all(root.join("data/source-registry")).unwrap();
    fs::copy(
        ws_root.join("data/source-registry/sources.toml"),
        root.join("data/source-registry/sources.toml"),
    )
    .unwrap();

    let b1_dir = root.join("data/review-batches/kuwiki-batch-001");
    fs::create_dir_all(&b1_dir).unwrap();
    fs::copy(
        ws_root.join("data/review-batches/kuwiki-batch-001/candidates.jsonl"),
        b1_dir.join("candidates.jsonl"),
    )
    .unwrap();
    fs::copy(
        ws_root.join("data/review-batches/kuwiki-batch-001/manifest.json"),
        b1_dir.join("manifest.json"),
    )
    .unwrap();
    fs::copy(
        ws_root.join("data/review-batches/kuwiki-batch-001/artifacts.sha256"),
        b1_dir.join("artifacts.sha256"),
    )
    .unwrap();

    let d1_dir = root.join("data/review-decisions/kuwiki-batch-001");
    fs::create_dir_all(&d1_dir).unwrap();

    let snapshot = load_and_validate_kuwiki_decisions(ws_root)
        .unwrap()
        .unwrap();

    let mut tampered_decisions = snapshot.decisions.clone();
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

    let mut dec_file_content = String::new();
    for d in tampered_decisions {
        dec_file_content.push_str(&serde_json::to_string(&d).unwrap());
        dec_file_content.push('\n');
    }
    fs::write(d1_dir.join("decisions.jsonl"), &dec_file_content).unwrap();

    let dec_sha = calculate_bytes_sha256(dec_file_content.as_bytes());
    let prov = r#"{"schema_version":"kuwiki-decision-provenance-v1","source_id":"kuwiki-batch-001","batch_id":"kuwiki-batch-001","candidate_sha256":"84a1439f28d95f978e9a7b84bc9ee946de2825489aee084bade85e255e156166","worksheet_sha256":"7c1341d75a2a1e8530495d9c69c45e10e7ba991f745ccf8a69a8c75db81af4b2","decisions_sha256":"DEC_SHA","reviewer_id":"ferhatguneri","audit_confirmation_date":"2026-09-02","counts":{"approved":733,"approved_with_metadata_change":0,"rejected_from_default_pack":214,"experimental_only":3,"needs_linguist":50,"needs_source_investigation":0,"pending":0,"total":1000},"human_confirmed_date_year_policy_count":26,"unresolved_auto_decisions":0}"#.replace("DEC_SHA", &dec_sha);
    fs::write(d1_dir.join("manifest.json"), &prov).unwrap();

    let prov_sha = calculate_bytes_sha256(prov.as_bytes());
    let art_sha_content = format!(
        "{}  decisions.jsonl\n{}  manifest.json\n",
        dec_sha, prov_sha
    );
    fs::write(d1_dir.join("artifacts.sha256"), art_sha_content).unwrap();

    let res = load_and_validate_kuwiki_decisions(root);
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(
        err_msg.contains("target_id mismatch")
            || err_msg.contains("Date/year policy ranks set mismatch")
            || err_msg.contains("mismatch")
    );
}

#[test]
fn test_no_repeat_assignment_invariants_and_error_cases() {
    use data_builder_lib::review::kuwiki_batch::{
        parse_kuwiki_batch_sequence, KuwikiReviewBatchCandidate, KuwikiReviewBatchManifest,
    };
    use std::collections::BTreeSet;

    // Test sequence parsing & malformed batch ID handling (Requirement 2 & K)
    assert_eq!(parse_kuwiki_batch_sequence("kuwiki-batch-001").unwrap(), 1);
    assert_eq!(parse_kuwiki_batch_sequence("kuwiki-batch-002").unwrap(), 2);
    assert_eq!(
        parse_kuwiki_batch_sequence("kuwiki-batch-100").unwrap(),
        100
    );

    assert!(parse_kuwiki_batch_sequence("kuwiki-batch-").is_err());
    assert!(parse_kuwiki_batch_sequence("kuwiki-batch-abc").is_err());
    assert!(parse_kuwiki_batch_sequence("other-batch-001").is_err());
    assert!(parse_kuwiki_batch_sequence("kuwiki-001").is_err());

    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    setup_valid_mock_environment(&temp_dir, 3500);

    // Step 1: Generate batch 001 with 1000 candidates
    let sum1 = generate_kuwiki_review_batch(root, "kuwiki", "kuwiki-batch-001", 1000).unwrap();
    assert_eq!(sum1.batch_size, 1000);

    let batch1_dir = root.join("data/review-batches/kuwiki-batch-001");
    let cand1_lines: Vec<String> = fs::read_to_string(batch1_dir.join("candidates.jsonl"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

    let mut set1: BTreeSet<String> = BTreeSet::new();
    for line in &cand1_lines {
        let cand: KuwikiReviewBatchCandidate = serde_json::from_str(line).unwrap();
        assert!(
            set1.insert(cand.normalized_token),
            "A/B: normalized tokens must be unique in batch 001"
        );
    }
    assert_eq!(set1.len(), 1000);

    // Step 2: Generate batch 002 with 1000 candidates
    let sum2 = generate_kuwiki_review_batch(root, "kuwiki", "kuwiki-batch-002", 1000).unwrap();
    assert_eq!(sum2.batch_size, 1000);

    let batch2_dir = root.join("data/review-batches/kuwiki-batch-002");
    let cand2_lines: Vec<String> = fs::read_to_string(batch2_dir.join("candidates.jsonl"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

    let mut set2: BTreeSet<String> = BTreeSet::new();
    for line in &cand2_lines {
        let cand: KuwikiReviewBatchCandidate = serde_json::from_str(line).unwrap();
        assert!(
            set2.insert(cand.normalized_token),
            "B: normalized tokens must be unique in batch 002"
        );
    }
    assert_eq!(set2.len(), 1000);

    // Test C: batch001 ∩ batch002 = empty
    let intersect_1_2: Vec<&String> = set1.intersection(&set2).collect();
    assert!(
        intersect_1_2.is_empty(),
        "C: batch001 ∩ batch002 must be empty"
    );

    // Verify manifest for batch 002 (Requirement 5)
    let man2: KuwikiReviewBatchManifest =
        serde_json::from_slice(&fs::read(batch2_dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(man2.excluded_prior_batches.len(), 1);
    assert_eq!(man2.excluded_prior_batches[0].batch_id, "kuwiki-batch-001");
    assert_eq!(man2.excluded_prior_batches[0].candidate_count, 1000);
    assert_eq!(man2.previously_assigned_normalized_token_count, Some(1000));
    assert!(man2.excluded_due_to_prior_assignment_count.is_some());

    // Step 3: Generate batch 003 fixture (Test D: batch003 ∩ (batch001 ∪ batch002) = empty)
    let sum3 = generate_kuwiki_review_batch(root, "kuwiki", "kuwiki-batch-003", 1000).unwrap();
    assert_eq!(sum3.batch_size, 1000);

    let batch3_dir = root.join("data/review-batches/kuwiki-batch-003");
    let cand3_lines: Vec<String> = fs::read_to_string(batch3_dir.join("candidates.jsonl"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

    let mut set3: BTreeSet<String> = BTreeSet::new();
    for line in &cand3_lines {
        let cand: KuwikiReviewBatchCandidate = serde_json::from_str(line).unwrap();
        assert!(
            set3.insert(cand.normalized_token),
            "normalized tokens must be unique in batch 003"
        );
    }
    assert_eq!(set3.len(), 1000);

    let union_1_2: BTreeSet<String> = set1.union(&set2).cloned().collect();
    let intersect_3_12: Vec<&String> = set3.intersection(&union_1_2).collect();
    assert!(
        intersect_3_12.is_empty(),
        "D: batch003 ∩ (batch001 ∪ batch002) must be empty"
    );

    let man3: KuwikiReviewBatchManifest =
        serde_json::from_slice(&fs::read(batch3_dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(man3.excluded_prior_batches.len(), 2);
    assert_eq!(man3.excluded_prior_batches[0].batch_id, "kuwiki-batch-001");
    assert_eq!(man3.excluded_prior_batches[1].batch_id, "kuwiki-batch-002");
    assert_eq!(man3.previously_assigned_normalized_token_count, Some(2000));

    // Test J: Historical Duplicate -> Fail closed
    let mut modified_cand2_lines = cand2_lines.clone();
    let mut cand_from_b1: KuwikiReviewBatchCandidate =
        serde_json::from_str(&cand1_lines[0]).unwrap();
    cand_from_b1.batch_id = "kuwiki-batch-002".to_string();
    cand_from_b1.batch_rank = 10;
    modified_cand2_lines[9] = serde_json::to_string(&cand_from_b1).unwrap();

    let new_cand2_bytes = (modified_cand2_lines.join("\n") + "\n").into_bytes();
    let new_cand2_sha = calculate_bytes_sha256(&new_cand2_bytes);
    fs::write(batch2_dir.join("candidates.jsonl"), &new_cand2_bytes).unwrap();

    let mut man2_mut = man2.clone();
    man2_mut.candidates_sha256 = new_cand2_sha.clone();
    let man2_bytes = serde_json::to_string_pretty(&man2_mut)
        .unwrap()
        .into_bytes();
    let man2_sha = calculate_bytes_sha256(&man2_bytes);
    fs::write(batch2_dir.join("manifest.json"), &man2_bytes).unwrap();

    let art2_content = format!(
        "{}  candidates.jsonl\n{}  manifest.json\n",
        new_cand2_sha, man2_sha
    );
    fs::write(batch2_dir.join("artifacts.sha256"), art2_content).unwrap();

    let res_b4 = generate_kuwiki_review_batch(root, "kuwiki", "kuwiki-batch-004", 100);
    assert!(res_b4.is_err());
    let err_msg = res_b4.err().unwrap();
    assert!(
        err_msg.contains("Historical duplicate detected across committed Kuwiki batches"),
        "Error: {}",
        err_msg
    );

    // Test K: Malformed prior batch ID in review-batches directory -> Fail closed
    fs::remove_dir_all(&batch2_dir).unwrap();
    fs::remove_dir_all(&batch3_dir).unwrap();
    let malformed_dir = root.join("data/review-batches/kuwiki-batch-xyz");
    fs::create_dir_all(&malformed_dir).unwrap();

    let res_malformed = generate_kuwiki_review_batch(root, "kuwiki", "kuwiki-batch-002", 100);
    assert!(res_malformed.is_err());
    let malformed_err = res_malformed.err().unwrap();
    assert!(
        malformed_err.contains("Malformed or ambiguous prior batch directory name")
            || malformed_err.contains("Invalid batch_id format"),
        "Error: {}",
        malformed_err
    );
}

#[test]
fn test_prior_status_types_and_canonical_normalization_exclusion() {
    use data_builder_lib::review::kuwiki_batch::{
        generate_kuwiki_review_batch, KuwikiReviewBatchCandidate,
    };
    use std::collections::BTreeSet;

    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    setup_valid_mock_environment(&temp_dir, 1500);

    // Generate batch 001
    let _sum1 = generate_kuwiki_review_batch(root, "kuwiki", "kuwiki-batch-001", 1000).unwrap();

    let batch1_dir = root.join("data/review-batches/kuwiki-batch-001");
    let cand1_lines: Vec<String> = fs::read_to_string(batch1_dir.join("candidates.jsonl"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

    let sum2 = generate_kuwiki_review_batch(root, "kuwiki", "kuwiki-batch-002", 500).unwrap();
    assert_eq!(sum2.batch_size, 500);

    let batch2_dir = root.join("data/review-batches/kuwiki-batch-002");
    let cand2_lines: Vec<String> = fs::read_to_string(batch2_dir.join("candidates.jsonl"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

    let set1: BTreeSet<String> = cand1_lines
        .iter()
        .map(|l| {
            let c: KuwikiReviewBatchCandidate = serde_json::from_str(l).unwrap();
            c.normalized_token
        })
        .collect();

    let set2: BTreeSet<String> = cand2_lines
        .iter()
        .map(|l| {
            let c: KuwikiReviewBatchCandidate = serde_json::from_str(l).unwrap();
            c.normalized_token
        })
        .collect();

    // E, F, G, H: No token in set1 can appear in set2
    for tok in &set1 {
        assert!(
            !set2.contains(tok),
            "Prior token '{}' must be excluded from batch 002",
            tok
        );
    }

    // I: Case / Unicode NFC equivalent token normalized comparison
    for c2 in &cand2_lines {
        let cand: KuwikiReviewBatchCandidate = serde_json::from_str(c2).unwrap();
        let norm_canonical = data_builder_lib::normalize_text(&cand.token);
        assert_eq!(norm_canonical, cand.normalized_token);
        assert!(!set1.contains(&norm_canonical));
    }
}

#[test]
fn test_load_and_validate_kuwiki_batch_002_decisions_workspace_root() {
    use data_builder_lib::review::kuwiki_decisions::{
        load_and_validate_kuwiki_batch_002_decisions, EXPECTED_KUWIKI_BATCH_002_APPROVED_COUNT,
    };
    use std::path::PathBuf;

    let ws_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");

    let snapshot = load_and_validate_kuwiki_batch_002_decisions(ws_root)
        .unwrap()
        .expect("kuwiki-batch-002 must be registered and valid in workspace");

    assert_eq!(snapshot.batch_id, "kuwiki-batch-002");
    assert_eq!(snapshot.candidates.len(), 1000);
    assert_eq!(snapshot.decisions.len(), 1000);
    assert_eq!(
        snapshot.counts_by_status.get("approved").cloned(),
        Some(EXPECTED_KUWIKI_BATCH_002_APPROVED_COUNT)
    );
    assert_eq!(
        snapshot.counts_by_status.get("experimental_only").cloned(),
        Some(2)
    );
}

#[test]
fn test_load_and_validate_all_kuwiki_decisions_workspace_root() {
    use data_builder_lib::review::kuwiki_decisions::load_and_validate_all_kuwiki_decisions;
    use std::path::PathBuf;

    let ws_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");

    let snapshots = load_and_validate_all_kuwiki_decisions(ws_root).unwrap();

    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].batch_id, "kuwiki-batch-001");
    assert_eq!(snapshots[1].batch_id, "kuwiki-batch-002");

    // Verify 0 overlap between batch-001 and batch-002 normalized tokens
    let set1: std::collections::BTreeSet<String> = snapshots[0]
        .candidates
        .iter()
        .map(|c| c.normalized_token.clone())
        .collect();
    let set2: std::collections::BTreeSet<String> = snapshots[1]
        .candidates
        .iter()
        .map(|c| c.normalized_token.clone())
        .collect();

    let intersection: Vec<&String> = set1.intersection(&set2).collect();
    assert!(
        intersection.is_empty(),
        "Batch 001 and Batch 002 must have 0 normalized token overlap, found: {:?}",
        intersection
    );
}

#[test]
fn test_parse_kuwiki_batch_sequence_strict_contract() {
    use data_builder_lib::review::kuwiki_batch::parse_kuwiki_batch_sequence;

    // Reject list:
    assert!(parse_kuwiki_batch_sequence("kuwiki-batch-2").is_err());
    assert!(parse_kuwiki_batch_sequence("kuwiki-batch-02").is_err());
    assert!(parse_kuwiki_batch_sequence("kuwiki-batch-0002").is_err());
    assert!(parse_kuwiki_batch_sequence("kuwiki-batch-000").is_err());
    assert!(parse_kuwiki_batch_sequence("kuwiki-batch-").is_err());
    assert!(parse_kuwiki_batch_sequence("kuwiki-batch-abc").is_err());
    assert!(parse_kuwiki_batch_sequence("kuwiki-001").is_err());
    assert!(parse_kuwiki_batch_sequence("other-batch-001").is_err());

    // Accept list:
    assert_eq!(parse_kuwiki_batch_sequence("kuwiki-batch-001").unwrap(), 1);
    assert_eq!(parse_kuwiki_batch_sequence("kuwiki-batch-002").unwrap(), 2);
    assert_eq!(
        parse_kuwiki_batch_sequence("kuwiki-batch-100").unwrap(),
        100
    );
    assert_eq!(
        parse_kuwiki_batch_sequence("kuwiki-batch-999").unwrap(),
        999
    );
}

#[test]
fn test_canonical_equivalence_no_repeat() {
    use data_builder_lib::normalize::normalize_text;
    use std::collections::BTreeSet;

    // 1. Uppercase / lowercase equivalence
    let upper = "PIRTÛK";
    let lower = "pirtûk";
    assert_eq!(normalize_text(upper), "pirtûk");
    assert_eq!(normalize_text(lower), "pirtûk");
    assert_eq!(normalize_text(upper), normalize_text(lower));

    // 2. NFC vs decomposed Unicode (NFD) equivalence
    // "êdî" in NFC: \u{00EA}d\u{00EE}
    let nfc = "êdî";
    // "êdî" in NFD: e + \u{0302} + d + i + \u{0302}
    let nfd = "e\u{0302}di\u{0302}";
    assert_eq!(normalize_text(nfc), "êdî");
    assert_eq!(normalize_text(nfd), "êdî");
    assert_eq!(normalize_text(nfc), normalize_text(nfd));

    // 3. Zero-width character equivalence (ZWSP \u{200B} and BOM \u{FEFF})
    let dirty_zwsp = "roj\u{200B}baş";
    let dirty_bom = "\u{FEFF}rojbaş";
    let clean = "rojbaş";
    assert_eq!(normalize_text(dirty_zwsp), "rojbaş");
    assert_eq!(normalize_text(dirty_bom), "rojbaş");
    assert_eq!(normalize_text(dirty_zwsp), normalize_text(clean));
    assert_eq!(normalize_text(dirty_bom), normalize_text(clean));

    // Prove that equivalent representations map to identical canonical keys in history exclusion
    let mut history = BTreeSet::new();
    history.insert(normalize_text(upper));
    history.insert(normalize_text(nfc));
    history.insert(normalize_text(clean));

    // Attempting to register any equivalent representation must be rejected as already seen
    assert!(history.contains(&normalize_text(lower)));
    assert!(history.contains(&normalize_text(nfd)));
    assert!(history.contains(&normalize_text(dirty_zwsp)));
    assert!(history.contains(&normalize_text(dirty_bom)));
}

#[test]
fn test_verify_artifacts_sha256_manifest_negative_cases() {
    use data_builder_lib::review::kuwiki_decisions::verify_artifacts_sha256_manifest;

    let temp = TempDir::new().unwrap();
    let dir = temp.path();

    let file_a = dir.join("decisions.jsonl");
    let file_b = dir.join("manifest.json");
    fs::write(&file_a, "content_a").unwrap();
    fs::write(&file_b, "content_b").unwrap();

    let hash_a = calculate_bytes_sha256(b"content_a");
    let hash_b = calculate_bytes_sha256(b"content_b");

    let art_file = dir.join("artifacts.sha256");

    // Case 1: Missing artifacts.sha256 -> fail
    assert!(verify_artifacts_sha256_manifest(
        &art_file,
        dir,
        &["decisions.jsonl", "manifest.json"]
    )
    .is_err());

    // Case 2: Missing required entry -> fail
    fs::write(&art_file, format!("{} decisions.jsonl\n", hash_a)).unwrap();
    let err2 =
        verify_artifacts_sha256_manifest(&art_file, dir, &["decisions.jsonl", "manifest.json"])
            .unwrap_err();
    assert!(err2.contains("Missing required entry"));

    // Case 3: Duplicate entry -> fail
    fs::write(
        &art_file,
        format!(
            "{} decisions.jsonl\n{} decisions.jsonl\n{} manifest.json\n",
            hash_a, hash_a, hash_b
        ),
    )
    .unwrap();
    let err3 =
        verify_artifacts_sha256_manifest(&art_file, dir, &["decisions.jsonl", "manifest.json"])
            .unwrap_err();
    assert!(err3.contains("Duplicate entry"));

    // Case 4: Malformed line -> fail
    fs::write(
        &art_file,
        format!("{} decisions.jsonl extra_token\n", hash_a),
    )
    .unwrap();
    let err4 =
        verify_artifacts_sha256_manifest(&art_file, dir, &["decisions.jsonl", "manifest.json"])
            .unwrap_err();
    assert!(err4.contains("Malformed line"));

    // Case 5: Wrong/disallowed filename -> fail
    fs::write(
        &art_file,
        format!(
            "{} decisions.jsonl\n{} manifest.json\n{} forbidden.txt\n",
            hash_a, hash_b, hash_a
        ),
    )
    .unwrap();
    let err5 =
        verify_artifacts_sha256_manifest(&art_file, dir, &["decisions.jsonl", "manifest.json"])
            .unwrap_err();
    assert!(err5.contains("Unexpected or disallowed filename"));

    // Case 6: Mismatched hash -> fail
    let bad_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    fs::write(
        &art_file,
        format!("{} decisions.jsonl\n{} manifest.json\n", bad_hash, hash_b),
    )
    .unwrap();
    let err6 =
        verify_artifacts_sha256_manifest(&art_file, dir, &["decisions.jsonl", "manifest.json"])
            .unwrap_err();
    assert!(err6.contains("Mismatched hash"));

    // Case 7: Path variants -> fail
    for bad_path in &[
        "./decisions.jsonl",
        "subdir/decisions.jsonl",
        "../decisions.jsonl",
        "/absolute/decisions.jsonl",
        "decisions.jsonl\\",
        "subdir\\decisions.jsonl",
    ] {
        fs::write(
            &art_file,
            format!("{} {}\n{} manifest.json\n", hash_a, bad_path, hash_b),
        )
        .unwrap();
        let err7 =
            verify_artifacts_sha256_manifest(&art_file, dir, &["decisions.jsonl", "manifest.json"])
                .unwrap_err();
        assert!(
            err7.contains("Unexpected or disallowed filename"),
            "Expected failure for bad path variant '{}', got: {}",
            bad_path,
            err7
        );
    }
}

#[test]
fn test_kuwiki_decisions_selection_policy_mismatch_rejection() {
    use data_builder_lib::review::kuwiki_decisions::load_and_validate_kuwiki_batch_002_decisions;

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("data/source-registry")).unwrap();
    fs::copy(
        ws_root.join("data/source-registry/sources.toml"),
        root.join("data/source-registry/sources.toml"),
    )
    .unwrap();

    let b2_dir = root.join("data/review-batches/kuwiki-batch-002");
    fs::create_dir_all(&b2_dir).unwrap();
    fs::copy(
        ws_root.join("data/review-batches/kuwiki-batch-002/candidates.jsonl"),
        b2_dir.join("candidates.jsonl"),
    )
    .unwrap();

    // Mutate selection_policy in batch manifest
    let orig_manifest_str =
        fs::read_to_string(ws_root.join("data/review-batches/kuwiki-batch-002/manifest.json"))
            .unwrap();
    let mut man_val: serde_json::Value = serde_json::from_str(&orig_manifest_str).unwrap();
    man_val["selection_policy"] = serde_json::Value::String("altered-policy".to_string());
    let new_man_str = serde_json::to_string_pretty(&man_val).unwrap();
    fs::write(b2_dir.join("manifest.json"), &new_man_str).unwrap();

    let cand_sha = calculate_bytes_sha256(
        &fs::read(ws_root.join("data/review-batches/kuwiki-batch-002/candidates.jsonl")).unwrap(),
    );
    let man_sha = calculate_bytes_sha256(new_man_str.as_bytes());

    let new_art_content = format!(
        "{}  candidates.jsonl\n{}  manifest.json\n",
        cand_sha, man_sha
    );
    fs::write(b2_dir.join("artifacts.sha256"), new_art_content).unwrap();

    let d2_dir = root.join("data/review-decisions/kuwiki-batch-002");
    fs::create_dir_all(&d2_dir).unwrap();
    fs::copy(
        ws_root.join("data/review-decisions/kuwiki-batch-002/decisions.jsonl"),
        d2_dir.join("decisions.jsonl"),
    )
    .unwrap();
    fs::copy(
        ws_root.join("data/review-decisions/kuwiki-batch-002/manifest.json"),
        d2_dir.join("manifest.json"),
    )
    .unwrap();
    fs::copy(
        ws_root.join("data/review-decisions/kuwiki-batch-002/artifacts.sha256"),
        d2_dir.join("artifacts.sha256"),
    )
    .unwrap();

    let res = load_and_validate_kuwiki_batch_002_decisions(root);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(
        err.contains("Batch manifest selection_policy mismatch"),
        "Got error: {}",
        err
    );
}

#[test]
fn test_kuwiki_decisions_human_confirmed_date_year_policy_count_mismatch_rejection() {
    use data_builder_lib::review::kuwiki_decisions::load_and_validate_kuwiki_batch_002_decisions;

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("data/source-registry")).unwrap();
    fs::copy(
        ws_root.join("data/source-registry/sources.toml"),
        root.join("data/source-registry/sources.toml"),
    )
    .unwrap();

    let b2_dir = root.join("data/review-batches/kuwiki-batch-002");
    fs::create_dir_all(&b2_dir).unwrap();
    fs::copy(
        ws_root.join("data/review-batches/kuwiki-batch-002/candidates.jsonl"),
        b2_dir.join("candidates.jsonl"),
    )
    .unwrap();
    fs::copy(
        ws_root.join("data/review-batches/kuwiki-batch-002/manifest.json"),
        b2_dir.join("manifest.json"),
    )
    .unwrap();
    fs::copy(
        ws_root.join("data/review-batches/kuwiki-batch-002/artifacts.sha256"),
        b2_dir.join("artifacts.sha256"),
    )
    .unwrap();

    let d2_dir = root.join("data/review-decisions/kuwiki-batch-002");
    fs::create_dir_all(&d2_dir).unwrap();
    fs::copy(
        ws_root.join("data/review-decisions/kuwiki-batch-002/decisions.jsonl"),
        d2_dir.join("decisions.jsonl"),
    )
    .unwrap();

    // Mutate human_confirmed_date_year_policy_count from 0 to 1 in decision manifest
    let orig_prov_str =
        fs::read_to_string(ws_root.join("data/review-decisions/kuwiki-batch-002/manifest.json"))
            .unwrap();
    let mut prov_val: serde_json::Value = serde_json::from_str(&orig_prov_str).unwrap();
    prov_val["human_confirmed_date_year_policy_count"] = serde_json::Value::Number(1.into());
    let new_prov_str = serde_json::to_string_pretty(&prov_val).unwrap();
    fs::write(d2_dir.join("manifest.json"), &new_prov_str).unwrap();

    let dec_sha = calculate_bytes_sha256(
        &fs::read(ws_root.join("data/review-decisions/kuwiki-batch-002/decisions.jsonl")).unwrap(),
    );
    let prov_sha = calculate_bytes_sha256(new_prov_str.as_bytes());

    let new_art_content = format!(
        "{}  decisions.jsonl\n{}  manifest.json\n",
        dec_sha, prov_sha
    );
    fs::write(d2_dir.join("artifacts.sha256"), new_art_content).unwrap();

    let res = load_and_validate_kuwiki_batch_002_decisions(root);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(
        err.contains("Decision provenance human_confirmed_date_year_policy_count mismatch"),
        "Got error: {}",
        err
    );
}
