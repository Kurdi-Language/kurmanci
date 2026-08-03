//! Comprehensive unit and integration tests for the three-pack benchmark comparison engine.

use data_builder_lib::evaluation::comparison::{
    classify_pairwise_comparison, evaluate_packs, validate_and_load_pack, PackQueryResult,
    PairwiseComparisonClass,
};
use data_builder_lib::evaluation::reports::calculate_file_sha256;
use data_builder_lib::evaluation::schema::{
    compute_canonical_case_id, BenchmarkCaseRecord, BenchmarkCategory, BenchmarkExpectation,
    BenchmarkReviewStatus, BenchmarkSourceInfo, BenchmarkSourceKind, BenchmarkTask,
    BENCHMARK_CASE_SCHEMA_VERSION,
};
use data_builder_lib::pack::manifest::{
    DataLicenseEntry, PackManifest, LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION,
};
use std::fs;
use tempfile::tempdir;

fn setup_mock_workspace() -> (tempfile::TempDir, String) {
    let temp = tempdir().unwrap();

    // Create policy file
    let policy_path = temp.path().join("data/pack-policy.toml");
    fs::create_dir_all(policy_path.parent().unwrap()).unwrap();
    let policy_content = r#"schema_version = "pack-policy-v1"
default_pack = "seed"

[packs.seed]
description = "Seed lexicon"
opt_in = false
model_profile = "none"

[packs.reviewed]
description = "Reviewed lexicon"
opt_in = false
model_profile = "none"

[packs.experimental-full]
description = "Experimental lexicon"
opt_in = true
model_profile = "none"
"#;
    fs::write(&policy_path, policy_content).unwrap();
    let policy_sha256 = calculate_file_sha256(&policy_path).unwrap();

    // Create packs
    for pack_id in ["seed", "reviewed", "experimental-full"] {
        let p_dir = temp.path().join(format!("data/build/packs/{}", pack_id));
        fs::create_dir_all(&p_dir).unwrap();

        let bin_path = p_dir.join("lexicon.bin");
        let entries = vec![
            data_builder_lib::validate::SourceLexiconEntry {
                word: "spas".to_string(),
                lemma: "spas".to_string(),
                normalized: "spas".to_string(),
                part_of_speech: "noun".to_string(),
                frequency: 10,
                status: "approved".to_string(),
                variants: vec![],
                sources: vec![],
                regions: vec![],
                frequency_metadata: None,
            },
            data_builder_lib::validate::SourceLexiconEntry {
                word: "rojba".to_string(),
                lemma: "rojba".to_string(),
                normalized: "rojba".to_string(),
                part_of_speech: "noun".to_string(),
                frequency: 30,
                status: "approved".to_string(),
                variants: vec![],
                sources: vec![],
                regions: vec![],
                frequency_metadata: None,
            },
            data_builder_lib::validate::SourceLexiconEntry {
                word: "rojbas".to_string(),
                lemma: "rojbas".to_string(),
                normalized: "rojbas".to_string(),
                part_of_speech: "noun".to_string(),
                frequency: 20,
                status: "approved".to_string(),
                variants: vec![],
                sources: vec![],
                regions: vec![],
                frequency_metadata: None,
            },
            data_builder_lib::validate::SourceLexiconEntry {
                word: "rojbaş".to_string(),
                lemma: "rojbaş".to_string(),
                normalized: "rojbas".to_string(),
                part_of_speech: "noun".to_string(),
                frequency: 5,
                status: "approved".to_string(),
                variants: vec![],
                sources: vec![],
                regions: vec![],
                frequency_metadata: None,
            },
        ];
        let bin_bytes = data_builder_lib::compile::compile_binary_pack(&entries).unwrap();
        fs::write(&bin_path, bin_bytes).unwrap();

        let bin_hash = calculate_file_sha256(&bin_path).unwrap();

        let manifest = PackManifest {
            schema_version: LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION.to_string(),
            pack_id: pack_id.to_string(),
            pack_format_version: 4,
            language: "ku-Latn".to_string(),
            is_default: pack_id == "seed",
            is_experimental: pack_id == "experimental-full",
            model_profile: "none".to_string(),
            frequency_entry_count: 0,
            bigram_count: 0,
            trigram_count: 0,
            manual_seed_selected_count: 4,
            external_approved_selected_count: 0,
            external_metadata_replacement_selected_count: 0,
            external_experimental_selected_count: 0,
            external_unreviewed_selected_count: 0,
            external_excluded_by_status_count: 0,
            external_discarded_by_collision_count: 0,
            final_unique_entry_count: 4,
            pack_policy_sha256: policy_sha256.clone(),
            review_decisions_sha256: Some("decisions_sha256_mock".to_string()),
            review_queue_manifest_sha256: Some("queue_sha256_mock".to_string()),
            controlled_review_report_manifest_sha256: Some("report_sha256_mock".to_string()),
            binary_sha256: bin_hash.clone(),
            binary_size_bytes: fs::metadata(&bin_path).unwrap().len(),
            data_licenses: vec![DataLicenseEntry {
                source_id: "manual-seed".to_string(),
                spdx: "Apache-2.0".to_string(),
            }],
            attribution_files: vec!["attribution.txt".to_string()],
        };

        fs::write(
            p_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        fs::write(p_dir.join("collision-report.jsonl"), "# Collision report\n").unwrap();
        fs::write(
            p_dir.join("attribution.txt"),
            "=== Source: manual-seed ===\n",
        )
        .unwrap();

        let m_hash = calculate_file_sha256(p_dir.join("manifest.json")).unwrap();
        let c_hash = calculate_file_sha256(p_dir.join("collision-report.jsonl")).unwrap();
        let r_hash = calculate_file_sha256(p_dir.join("attribution.txt")).unwrap();

        let art_content = format!(
            "{} data/build/packs/{}/manifest.json\n{} data/build/packs/{}/lexicon.bin\n{} data/build/packs/{}/collision-report.jsonl\n{} data/build/packs/{}/attribution.txt\n",
            m_hash, pack_id, bin_hash, pack_id, c_hash, pack_id, r_hash, pack_id
        );
        fs::write(p_dir.join("artifacts.sha256"), art_content).unwrap();
    }

    (temp, policy_sha256)
}

#[derive(Clone, Copy)]
enum ProvenanceField {
    ReviewDecisions,
    ReviewQueueManifest,
    ControlledReviewReportManifest,
}

impl ProvenanceField {
    fn name(self) -> &'static str {
        match self {
            Self::ReviewDecisions => "review_decisions_sha256",
            Self::ReviewQueueManifest => "review_queue_manifest_sha256",
            Self::ControlledReviewReportManifest => "controlled_review_report_manifest_sha256",
        }
    }

    fn set(self, manifest: &mut PackManifest, value: Option<&str>) {
        let value = value.map(str::to_string);
        match self {
            Self::ReviewDecisions => manifest.review_decisions_sha256 = value,
            Self::ReviewQueueManifest => manifest.review_queue_manifest_sha256 = value,
            Self::ControlledReviewReportManifest => {
                manifest.controlled_review_report_manifest_sha256 = value
            }
        }
    }
}

fn rewrite_pack_manifest(
    temp: &tempfile::TempDir,
    pack_id: &str,
    update: impl FnOnce(&mut PackManifest),
) {
    let pack_dir = temp.path().join(format!("data/build/packs/{pack_id}"));
    let manifest_path = pack_dir.join("manifest.json");
    let mut manifest: PackManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    update(&mut manifest);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let manifest_hash = calculate_file_sha256(&manifest_path).unwrap();
    let binary_hash = calculate_file_sha256(pack_dir.join("lexicon.bin")).unwrap();
    let collision_hash = calculate_file_sha256(pack_dir.join("collision-report.jsonl")).unwrap();
    let attribution_hash = calculate_file_sha256(pack_dir.join("attribution.txt")).unwrap();
    let artifacts = format!(
        "{manifest_hash} data/build/packs/{pack_id}/manifest.json\n{binary_hash} data/build/packs/{pack_id}/lexicon.bin\n{collision_hash} data/build/packs/{pack_id}/collision-report.jsonl\n{attribution_hash} data/build/packs/{pack_id}/attribution.txt\n"
    );
    fs::write(pack_dir.join("artifacts.sha256"), artifacts).unwrap();
}

fn write_empty_reviewed_cases(temp: &tempfile::TempDir) {
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();
    fs::write(eval_dir.join("reviewed-cases.jsonl"), "").unwrap();
}

#[test]
fn test_missing_reviewed_file_rejected() {
    let (temp, _) = setup_mock_workspace();
    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains("Authoritative benchmark file missing"));
}

#[test]
fn test_empty_reviewed_benchmark_reports_unavailable_metrics() {
    let (temp, _) = setup_mock_workspace();
    write_empty_reviewed_cases(&temp);

    let summary = evaluate_packs(temp.path()).unwrap();
    assert!(!summary.benchmark_ready);
    assert_eq!(summary.total_reviewed_cases, 0);
    for metrics in summary.packs.values() {
        for metric in [
            &metrics.known_word_coverage,
            &metrics.false_acceptance_rate,
            &metrics.top_1_accuracy,
            &metrics.top_3_accuracy,
            &metrics.top_5_accuracy,
            &metrics.mrr,
            &metrics.completion_recall,
            &metrics.exact_preservation_rate,
            &metrics.no_candidate_rate,
        ] {
            assert_eq!(metric.eligible_count, 0);
            assert_eq!(metric.matched_count, 0);
            assert_eq!(metric.excluded_count, 0);
            assert_eq!(metric.value, None);
        }
    }
}

#[test]
fn test_draft_record_in_reviewed_file_rejected() {
    let (temp, _) = setup_mock_workspace();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let exp = BenchmarkExpectation {
        accepted: Some(true),
        preserve_exact: Some(true),
        expected_candidates: vec![],
        forbidden_candidates: vec![],
        allow_no_candidate: None,
        required_top_k: None,
    };
    let case_id = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ExactPreservation,
        "spas",
        None,
        &exp,
    )
    .unwrap();

    let draft_record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::AcceptWord,
        category: BenchmarkCategory::ExactPreservation,
        input: "spas".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::Draft,
        reviewer_id: None,
        review_date: None,
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    fs::write(
        eval_dir.join("reviewed-cases.jsonl"),
        serde_json::to_string(&draft_record).unwrap() + "\n",
    )
    .unwrap();

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains("expected 'human-reviewed'"));
}

#[test]
fn test_required_top_k_separation_from_mrr_and_top_k_metrics() {
    let (temp, _) = setup_mock_workspace();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let exp = BenchmarkExpectation {
        accepted: None,
        preserve_exact: None,
        expected_candidates: vec!["rojbas".to_string()],
        forbidden_candidates: vec![],
        allow_no_candidate: None,
        required_top_k: Some(1), // Target is Top-1, but candidate appears at rank 2
    };
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "rojba",
        None,
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "rojba".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("test-fixture-reviewer".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    fs::write(
        eval_dir.join("reviewed-cases.jsonl"),
        serde_json::to_string(&record).unwrap() + "\n",
    )
    .unwrap();

    let summary = evaluate_packs(temp.path()).unwrap();
    let seed_m = &summary.packs["seed"];

    // Rank is 2:
    // Top-1 Accuracy: false (rank 2 != 1)
    assert_eq!(seed_m.top_1_accuracy.matched_count, 0);
    // Top-3 Accuracy: true (rank 2 <= 3)
    assert_eq!(seed_m.top_3_accuracy.matched_count, 1);
    // Top-5 Accuracy: true (rank 2 <= 5)
    assert_eq!(seed_m.top_5_accuracy.matched_count, 1);
    // MRR: 1.0 / 2.0 = 0.5
    assert_eq!(seed_m.mrr.value, Some(0.5));
}

#[test]
fn test_forbidden_candidate_introduction_and_removal() {
    let base_clean = PackQueryResult {
        accepted: false,
        suggestions: vec!["spas".to_string()],
        best_expected_rank: Some(1),
        satisfies_required_top_k: true,
        forbidden_hits: vec![],
        best_forbidden_rank: None,
    };

    let cand_with_forb = PackQueryResult {
        accepted: false,
        suggestions: vec!["spas".to_string(), "spaz".to_string()],
        best_expected_rank: Some(1),
        satisfies_required_top_k: true,
        forbidden_hits: vec!["spaz".to_string()],
        best_forbidden_rank: Some(2),
    };

    let exp = BenchmarkExpectation {
        accepted: None,
        preserve_exact: None,
        expected_candidates: vec!["spas".to_string()],
        forbidden_candidates: vec!["spaz".to_string()],
        allow_no_candidate: None,
        required_top_k: Some(5),
    };

    // Introducing forbidden candidate -> Regression
    assert_eq!(
        classify_pairwise_comparison(
            &base_clean,
            &cand_with_forb,
            &exp,
            BenchmarkTask::CorrectWord
        ),
        PairwiseComparisonClass::Regression
    );

    // Removing forbidden candidate -> Improvement
    assert_eq!(
        classify_pairwise_comparison(
            &cand_with_forb,
            &base_clean,
            &exp,
            BenchmarkTask::CorrectWord
        ),
        PairwiseComparisonClass::Improvement
    );
}

#[test]
fn test_missing_decisions_hash_rejected() {
    let (temp, _) = setup_mock_workspace();
    for p_id in ["reviewed", "experimental-full"] {
        let p_dir = temp.path().join(format!("data/build/packs/{}", p_id));
        let manifest_path = p_dir.join("manifest.json");
        let mut manifest: PackManifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest.review_decisions_sha256 = None;
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let m_hash = calculate_file_sha256(&manifest_path).unwrap();
        let bin_hash = calculate_file_sha256(p_dir.join("lexicon.bin")).unwrap();
        let c_hash = calculate_file_sha256(p_dir.join("collision-report.jsonl")).unwrap();
        let r_hash = calculate_file_sha256(p_dir.join("attribution.txt")).unwrap();

        let art_content = format!(
            "{} data/build/packs/{}/manifest.json\n{} data/build/packs/{}/lexicon.bin\n{} data/build/packs/{}/collision-report.jsonl\n{} data/build/packs/{}/attribution.txt\n",
            m_hash, p_id, bin_hash, p_id, c_hash, p_id, r_hash, p_id
        );
        fs::write(p_dir.join("artifacts.sha256"), art_content).unwrap();
    }

    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();
    fs::write(eval_dir.join("reviewed-cases.jsonl"), "").unwrap();

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains("Reviewed pack is missing required review_decisions_sha256 provenance"));
}

#[test]
fn test_experimental_missing_decisions_hash_rejected() {
    let (temp, _) = setup_mock_workspace();
    let p_dir = temp.path().join("data/build/packs/experimental-full");
    let manifest_path = p_dir.join("manifest.json");
    let mut manifest: PackManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.review_decisions_sha256 = None;
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let m_hash = calculate_file_sha256(&manifest_path).unwrap();
    let bin_hash = calculate_file_sha256(p_dir.join("lexicon.bin")).unwrap();
    let c_hash = calculate_file_sha256(p_dir.join("collision-report.jsonl")).unwrap();
    let r_hash = calculate_file_sha256(p_dir.join("attribution.txt")).unwrap();

    let art_content = format!(
        "{} data/build/packs/experimental-full/manifest.json\n{} data/build/packs/experimental-full/lexicon.bin\n{} data/build/packs/experimental-full/collision-report.jsonl\n{} data/build/packs/experimental-full/attribution.txt\n",
        m_hash, bin_hash, c_hash, r_hash
    );
    fs::write(p_dir.join("artifacts.sha256"), art_content).unwrap();

    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();
    fs::write(eval_dir.join("reviewed-cases.jsonl"), "").unwrap();

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err
        .contains("Experimental-full pack is missing required review_decisions_sha256 provenance"));
}

#[test]
fn test_both_packs_missing_queue_and_controlled_report_hashes_rejected() {
    for field in [
        ProvenanceField::ReviewQueueManifest,
        ProvenanceField::ControlledReviewReportManifest,
    ] {
        let (temp, _) = setup_mock_workspace();
        for pack_id in ["reviewed", "experimental-full"] {
            rewrite_pack_manifest(&temp, pack_id, |manifest| field.set(manifest, None));
        }
        write_empty_reviewed_cases(&temp);

        let err = evaluate_packs(temp.path()).unwrap_err();
        assert!(
            err.contains(&format!(
                "Reviewed pack is missing required {} provenance",
                field.name()
            )),
            "unexpected error for {}: {err}",
            field.name()
        );
    }
}

#[test]
fn test_matching_empty_provenance_hashes_rejected() {
    for field in [
        ProvenanceField::ReviewDecisions,
        ProvenanceField::ReviewQueueManifest,
        ProvenanceField::ControlledReviewReportManifest,
    ] {
        let (temp, _) = setup_mock_workspace();
        for pack_id in ["reviewed", "experimental-full"] {
            rewrite_pack_manifest(&temp, pack_id, |manifest| field.set(manifest, Some("")));
        }
        write_empty_reviewed_cases(&temp);

        let err = evaluate_packs(temp.path()).unwrap_err();
        assert!(
            err.contains(&format!(
                "Reviewed pack is missing required {} provenance or it is empty",
                field.name()
            )),
            "unexpected error for {}: {err}",
            field.name()
        );
    }
}

#[test]
fn test_experimental_missing_queue_manifest_hash_rejected() {
    let (temp, _) = setup_mock_workspace();
    rewrite_pack_manifest(&temp, "experimental-full", |manifest| {
        ProvenanceField::ReviewQueueManifest.set(manifest, None)
    });
    write_empty_reviewed_cases(&temp);

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains(
        "Experimental-full pack is missing required review_queue_manifest_sha256 provenance"
    ));
}

#[test]
fn test_reviewed_missing_controlled_report_manifest_hash_rejected() {
    let (temp, _) = setup_mock_workspace();
    rewrite_pack_manifest(&temp, "reviewed", |manifest| {
        ProvenanceField::ControlledReviewReportManifest.set(manifest, None)
    });
    write_empty_reviewed_cases(&temp);

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains(
        "Reviewed pack is missing required controlled_review_report_manifest_sha256 provenance"
    ));
}

#[test]
fn test_mismatched_review_decisions_sha256_rejected() {
    let (temp, _) = setup_mock_workspace();
    let p_dir = temp.path().join("data/build/packs/experimental-full");

    let manifest_path = p_dir.join("manifest.json");
    let mut manifest: PackManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.review_decisions_sha256 = Some("mismatched_hash".to_string());
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let m_hash = calculate_file_sha256(&manifest_path).unwrap();
    let bin_hash = calculate_file_sha256(p_dir.join("lexicon.bin")).unwrap();
    let c_hash = calculate_file_sha256(p_dir.join("collision-report.jsonl")).unwrap();
    let r_hash = calculate_file_sha256(p_dir.join("attribution.txt")).unwrap();

    let art_content = format!(
        "{} data/build/packs/experimental-full/manifest.json\n{} data/build/packs/experimental-full/lexicon.bin\n{} data/build/packs/experimental-full/collision-report.jsonl\n{} data/build/packs/experimental-full/attribution.txt\n",
        m_hash, bin_hash, c_hash, r_hash
    );
    fs::write(p_dir.join("artifacts.sha256"), art_content).unwrap();

    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();
    fs::write(eval_dir.join("reviewed-cases.jsonl"), "").unwrap();

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains("review_decisions_sha256 mismatch"));
}

#[test]
fn test_missing_queue_manifest_hash_rejected() {
    let (temp, _) = setup_mock_workspace();
    let p_dir = temp.path().join("data/build/packs/reviewed");

    let manifest_path = p_dir.join("manifest.json");
    let mut manifest: PackManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.review_queue_manifest_sha256 = None;
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let m_hash = calculate_file_sha256(&manifest_path).unwrap();
    let bin_hash = calculate_file_sha256(p_dir.join("lexicon.bin")).unwrap();
    let c_hash = calculate_file_sha256(p_dir.join("collision-report.jsonl")).unwrap();
    let r_hash = calculate_file_sha256(p_dir.join("attribution.txt")).unwrap();

    let art_content = format!(
        "{} data/build/packs/reviewed/manifest.json\n{} data/build/packs/reviewed/lexicon.bin\n{} data/build/packs/reviewed/collision-report.jsonl\n{} data/build/packs/reviewed/attribution.txt\n",
        m_hash, bin_hash, c_hash, r_hash
    );
    fs::write(p_dir.join("artifacts.sha256"), art_content).unwrap();

    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();
    fs::write(eval_dir.join("reviewed-cases.jsonl"), "").unwrap();

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(
        err.contains("Reviewed pack is missing required review_queue_manifest_sha256 provenance")
    );
}

#[test]
fn test_mismatched_queue_manifest_hash_rejected() {
    let (temp, _) = setup_mock_workspace();
    let p_dir = temp.path().join("data/build/packs/experimental-full");

    let manifest_path = p_dir.join("manifest.json");
    let mut manifest: PackManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.review_queue_manifest_sha256 = Some("mismatched_queue_hash".to_string());
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let m_hash = calculate_file_sha256(&manifest_path).unwrap();
    let bin_hash = calculate_file_sha256(p_dir.join("lexicon.bin")).unwrap();
    let c_hash = calculate_file_sha256(p_dir.join("collision-report.jsonl")).unwrap();
    let r_hash = calculate_file_sha256(p_dir.join("attribution.txt")).unwrap();

    let art_content = format!(
        "{} data/build/packs/experimental-full/manifest.json\n{} data/build/packs/experimental-full/lexicon.bin\n{} data/build/packs/experimental-full/collision-report.jsonl\n{} data/build/packs/experimental-full/attribution.txt\n",
        m_hash, bin_hash, c_hash, r_hash
    );
    fs::write(p_dir.join("artifacts.sha256"), art_content).unwrap();

    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();
    fs::write(eval_dir.join("reviewed-cases.jsonl"), "").unwrap();

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains("review_queue_manifest_sha256 mismatch"));
}

#[test]
fn test_missing_controlled_report_manifest_hash_rejected() {
    let (temp, _) = setup_mock_workspace();
    let p_dir = temp.path().join("data/build/packs/experimental-full");

    let manifest_path = p_dir.join("manifest.json");
    let mut manifest: PackManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.controlled_review_report_manifest_sha256 = None;
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let m_hash = calculate_file_sha256(&manifest_path).unwrap();
    let bin_hash = calculate_file_sha256(p_dir.join("lexicon.bin")).unwrap();
    let c_hash = calculate_file_sha256(p_dir.join("collision-report.jsonl")).unwrap();
    let r_hash = calculate_file_sha256(p_dir.join("attribution.txt")).unwrap();

    let art_content = format!(
        "{} data/build/packs/experimental-full/manifest.json\n{} data/build/packs/experimental-full/lexicon.bin\n{} data/build/packs/experimental-full/collision-report.jsonl\n{} data/build/packs/experimental-full/attribution.txt\n",
        m_hash, bin_hash, c_hash, r_hash
    );
    fs::write(p_dir.join("artifacts.sha256"), art_content).unwrap();

    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();
    fs::write(eval_dir.join("reviewed-cases.jsonl"), "").unwrap();

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains("Experimental-full pack is missing required controlled_review_report_manifest_sha256 provenance"));
}

#[test]
fn test_mismatched_controlled_report_manifest_hash_rejected() {
    let (temp, _) = setup_mock_workspace();
    let p_dir = temp.path().join("data/build/packs/experimental-full");

    let manifest_path = p_dir.join("manifest.json");
    let mut manifest: PackManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.controlled_review_report_manifest_sha256 = Some("mismatched_report_hash".to_string());
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let m_hash = calculate_file_sha256(&manifest_path).unwrap();
    let bin_hash = calculate_file_sha256(p_dir.join("lexicon.bin")).unwrap();
    let c_hash = calculate_file_sha256(p_dir.join("collision-report.jsonl")).unwrap();
    let r_hash = calculate_file_sha256(p_dir.join("attribution.txt")).unwrap();

    let art_content = format!(
        "{} data/build/packs/experimental-full/manifest.json\n{} data/build/packs/experimental-full/lexicon.bin\n{} data/build/packs/experimental-full/collision-report.jsonl\n{} data/build/packs/experimental-full/attribution.txt\n",
        m_hash, bin_hash, c_hash, r_hash
    );
    fs::write(p_dir.join("artifacts.sha256"), art_content).unwrap();

    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();
    fs::write(eval_dir.join("reviewed-cases.jsonl"), "").unwrap();

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains("controlled_review_report_manifest_sha256 mismatch"));
}

#[test]
fn test_no_candidate_correct_word() {
    let (temp, _) = setup_mock_workspace();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let exp = BenchmarkExpectation {
        accepted: None,
        preserve_exact: None,
        expected_candidates: vec![],
        forbidden_candidates: vec![],
        allow_no_candidate: Some(true),
        required_top_k: None,
    };
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "xyzxyz",
        None,
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "xyzxyz".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("test-fixture-reviewer".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    fs::write(
        eval_dir.join("reviewed-cases.jsonl"),
        serde_json::to_string(&record).unwrap() + "\n",
    )
    .unwrap();

    let summary = evaluate_packs(temp.path()).unwrap();
    let seed_m = &summary.packs["seed"];
    assert_eq!(seed_m.no_candidate_rate.matched_count, 1);
}

#[test]
fn test_no_candidate_complete_prefix() {
    let (temp, _) = setup_mock_workspace();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let exp = BenchmarkExpectation {
        accepted: None,
        preserve_exact: None,
        expected_candidates: vec![],
        forbidden_candidates: vec![],
        allow_no_candidate: Some(true),
        required_top_k: None,
    };
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CompletePrefix,
        BenchmarkCategory::PrefixCompletion,
        "xyzxyz",
        None,
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CompletePrefix,
        category: BenchmarkCategory::PrefixCompletion,
        input: "xyzxyz".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("test-fixture-reviewer".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    fs::write(
        eval_dir.join("reviewed-cases.jsonl"),
        serde_json::to_string(&record).unwrap() + "\n",
    )
    .unwrap();

    let summary = evaluate_packs(temp.path()).unwrap();
    let seed_m = &summary.packs["seed"];
    assert_eq!(seed_m.no_candidate_rate.matched_count, 1);
}

#[test]
fn test_nonempty_complete_prefix_recall() {
    let (temp, _) = setup_mock_workspace();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let expectation = BenchmarkExpectation {
        accepted: None,
        preserve_exact: None,
        expected_candidates: vec!["rojba".to_string()],
        forbidden_candidates: vec![],
        allow_no_candidate: None,
        required_top_k: Some(3),
    };
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CompletePrefix,
        BenchmarkCategory::PrefixCompletion,
        "roj",
        None,
        &expectation,
    )
    .unwrap();
    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CompletePrefix,
        category: BenchmarkCategory::PrefixCompletion,
        input: "roj".to_string(),
        context: None,
        expectation,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("test-fixture-reviewer".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };
    fs::write(
        eval_dir.join("reviewed-cases.jsonl"),
        serde_json::to_string(&record).unwrap() + "\n",
    )
    .unwrap();

    let summary = evaluate_packs(temp.path()).unwrap();
    for metrics in summary.packs.values() {
        assert_eq!(metrics.completion_recall.eligible_count, 1);
        assert_eq!(metrics.completion_recall.matched_count, 1);
        assert_eq!(metrics.completion_recall.value, Some(1.0));
    }
}

#[test]
fn test_stale_or_mismatched_pack_policy_rejected() {
    let (temp, _policy_sha256) = setup_mock_workspace();
    let err = validate_and_load_pack(temp.path(), "seed", "stale_policy_sha256").unwrap_err();
    assert!(err.contains("policy SHA-256"));
}

#[test]
fn test_tampered_pack_artifact_rejected() {
    let (temp, policy_sha256) = setup_mock_workspace();
    let p_dir = temp.path().join("data/build/packs/seed");

    // Tamper with binary
    fs::write(p_dir.join("lexicon.bin"), "corrupted content").unwrap();

    let err = validate_and_load_pack(temp.path(), "seed", &policy_sha256).unwrap_err();
    assert!(err.contains("Artifact SHA-256 mismatch"));
}

#[test]
fn test_extra_pack_artifact_rejected() {
    let (temp, _) = setup_mock_workspace();
    fs::write(
        temp.path().join("data/build/packs/seed/unexpected.txt"),
        "unexpected",
    )
    .unwrap();
    write_empty_reviewed_cases(&temp);

    let err = evaluate_packs(temp.path()).unwrap_err();
    assert!(err.contains("artifact set mismatch"));
}

#[test]
fn test_nonempty_deterministic_report_generation() {
    let (temp, _) = setup_mock_workspace();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let exp = BenchmarkExpectation {
        accepted: Some(true),
        preserve_exact: Some(true),
        expected_candidates: vec![],
        forbidden_candidates: vec![],
        allow_no_candidate: None,
        required_top_k: None,
    };
    let case_id = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ExactPreservation,
        "spas",
        None,
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::AcceptWord,
        category: BenchmarkCategory::ExactPreservation,
        input: "spas".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("test-fixture-reviewer".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    fs::write(
        eval_dir.join("reviewed-cases.jsonl"),
        serde_json::to_string(&record).unwrap() + "\n",
    )
    .unwrap();

    let summary1 = evaluate_packs(temp.path()).unwrap();
    let report_dir = temp.path().join("data/reports/pack-comparison");
    let art1 = fs::read_to_string(report_dir.join("artifacts.sha256")).unwrap();

    let summary2 = evaluate_packs(temp.path()).unwrap();
    let art2 = fs::read_to_string(report_dir.join("artifacts.sha256")).unwrap();

    assert_eq!(art1, art2);
    assert!(summary1.benchmark_ready);
    assert_eq!(summary1.total_reviewed_cases, 1);
    assert_eq!(summary1.review_decisions_sha256, "decisions_sha256_mock");
    assert_eq!(summary2.review_decisions_sha256, "decisions_sha256_mock");
    assert_eq!(summary1.review_queue_manifest_sha256, "queue_sha256_mock");
    assert_eq!(summary2.review_queue_manifest_sha256, "queue_sha256_mock");
    assert_eq!(
        summary1.controlled_review_report_manifest_sha256,
        "report_sha256_mock"
    );
    assert_eq!(
        summary2.controlled_review_report_manifest_sha256,
        "report_sha256_mock"
    );
}

/// Asserts that authoritative benchmark files remain empty until genuine human-reviewed cases are added in Milestone 4B.3.
/// Note: While 4B.3 is planned, both files must remain 0 bytes. When native linguists populate reviewed-cases.jsonl in 4B.3,
/// this test will transition to a before/after byte comparison.
#[test]
fn test_authoritative_benchmark_files_unchanged() {
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| std::path::PathBuf::from(d).join(".."))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let draft_path = root.join("evaluation/spelling/draft-cases.jsonl");
    let reviewed_path = root.join("evaluation/spelling/reviewed-cases.jsonl");

    let draft_meta = fs::metadata(&draft_path).expect("draft-cases.jsonl missing");
    let reviewed_meta = fs::metadata(&reviewed_path).expect("reviewed-cases.jsonl missing");

    assert_eq!(draft_meta.len(), 0, "draft-cases.jsonl must remain empty");
    assert_eq!(
        reviewed_meta.len(),
        0,
        "reviewed-cases.jsonl must remain empty until human review"
    );
}
