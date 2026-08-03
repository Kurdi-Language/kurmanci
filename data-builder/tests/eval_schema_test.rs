//! Comprehensive unit and integration tests for evaluation benchmark schemas, canonical ID generation, validation, and provenance reporting.

use data_builder_lib::evaluation::provenance::generate_provenance_report;
use data_builder_lib::evaluation::schema::{
    compute_canonical_case_id, compute_source_provenance_sha256, is_compatible_task_category,
    validate_case_record, validate_review_date, validate_reviewer_id, BenchmarkCaseRecord,
    BenchmarkCategory, BenchmarkExpectation, BenchmarkReviewStatus, BenchmarkSourceInfo,
    BenchmarkSourceKind, BenchmarkTask, BENCHMARK_CASE_SCHEMA_VERSION,
};
use data_builder_lib::evaluation::validator::validate_benchmark_case_set;
use std::fs;
use tempfile::tempdir;

fn create_base_expectation() -> BenchmarkExpectation {
    BenchmarkExpectation {
        accepted: None,
        preserve_exact: None,
        expected_candidates: vec!["spas".to_string()],
        forbidden_candidates: vec!["spaz".to_string()],
        allow_no_candidate: None,
        required_top_k: Some(5),
    }
}

#[test]
fn test_documented_example_case_id_matches_canonical_encoding() {
    let expectation = BenchmarkExpectation {
        accepted: Some(true),
        preserve_exact: Some(true),
        ..Default::default()
    };
    let case_id = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ExactPreservation,
        "spas",
        None,
        &expectation,
    )
    .unwrap();

    assert_eq!(
        case_id,
        "465043f4b858ae3a5d5c74aed2e80a35c482b822490f2a69b929fcd4f05e166e"
    );
}

#[test]
fn test_case_id_differs_on_accepted() {
    let mut exp1 = create_base_expectation();
    exp1.accepted = Some(true);
    let id1 = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ExactPreservation,
        "spas",
        None,
        &exp1,
    )
    .unwrap();

    let mut exp2 = create_base_expectation();
    exp2.accepted = Some(false);
    let id2 = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ExactPreservation,
        "spas",
        None,
        &exp2,
    )
    .unwrap();

    assert_ne!(id1, id2);
}

#[test]
fn test_case_id_differs_on_forbidden_candidates() {
    let mut exp1 = create_base_expectation();
    exp1.forbidden_candidates = vec!["spaz".to_string()];
    let id1 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp1,
    )
    .unwrap();

    let mut exp2 = create_base_expectation();
    exp2.forbidden_candidates = vec!["spas".to_string()];
    let id2 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp2,
    )
    .unwrap();

    assert_ne!(id1, id2);
}

#[test]
fn test_case_id_differs_on_preserve_exact() {
    let mut exp1 = create_base_expectation();
    exp1.preserve_exact = Some(true);
    let id1 = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ExactPreservation,
        "spas",
        None,
        &exp1,
    )
    .unwrap();

    let mut exp2 = create_base_expectation();
    exp2.preserve_exact = None;
    let id2 = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ExactPreservation,
        "spas",
        None,
        &exp2,
    )
    .unwrap();

    assert_ne!(id1, id2);
}

#[test]
fn test_case_id_differs_on_allow_no_candidate() {
    let exp1 = BenchmarkExpectation {
        allow_no_candidate: Some(true),
        ..Default::default()
    };
    let id1 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::NoCandidate,
        "xyz123",
        None,
        &exp1,
    )
    .unwrap();

    let exp2 = BenchmarkExpectation {
        allow_no_candidate: None,
        ..Default::default()
    };
    let id2 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::NoCandidate,
        "xyz123",
        None,
        &exp2,
    )
    .unwrap();

    assert_ne!(id1, id2);
}

#[test]
fn test_case_id_differs_on_required_top_k() {
    let mut exp1 = create_base_expectation();
    exp1.required_top_k = Some(1);
    let id1 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp1,
    )
    .unwrap();

    let mut exp2 = create_base_expectation();
    exp2.required_top_k = Some(5);
    let id2 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp2,
    )
    .unwrap();

    assert_ne!(id1, id2);
}

#[test]
fn test_case_id_identical_on_reordered_expected_candidates() {
    let mut exp1 = create_base_expectation();
    exp1.expected_candidates = vec!["alpha".to_string(), "beta".to_string()];
    let id1 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp1,
    )
    .unwrap();

    let mut exp2 = create_base_expectation();
    exp2.expected_candidates = vec!["beta".to_string(), "alpha".to_string()];
    let id2 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp2,
    )
    .unwrap();

    assert_eq!(id1, id2);
}

#[test]
fn test_case_id_identical_on_reordered_forbidden_candidates() {
    let mut exp1 = create_base_expectation();
    exp1.forbidden_candidates = vec!["foo".to_string(), "bar".to_string()];
    let id1 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp1,
    )
    .unwrap();

    let mut exp2 = create_base_expectation();
    exp2.forbidden_candidates = vec!["bar".to_string(), "foo".to_string()];
    let id2 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp2,
    )
    .unwrap();

    assert_eq!(id1, id2);
}

#[test]
fn test_capitalization_and_nfc_display_preservation() {
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
        BenchmarkCategory::ProperNoun,
        "Amed",
        None,
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::AcceptWord,
        category: BenchmarkCategory::ProperNoun,
        input: "Amed".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: Some("Proper noun capitalization test case".to_string()),
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    assert!(validate_case_record(&record).is_ok());
}

#[test]
fn test_task_category_compatibility_matrix() {
    assert!(is_compatible_task_category(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ProperNoun
    ));
    assert!(is_compatible_task_category(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution
    ));
    assert!(is_compatible_task_category(
        BenchmarkTask::CompletePrefix,
        BenchmarkCategory::PrefixCompletion
    ));

    assert!(!is_compatible_task_category(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::Substitution
    ));
    assert!(!is_compatible_task_category(
        BenchmarkTask::CompletePrefix,
        BenchmarkCategory::ProperNoun
    ));
}

#[test]
fn test_complete_contradictory_expectations_rejected() {
    let temp = tempdir().unwrap();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let mut exp1 = create_base_expectation();
    exp1.expected_candidates = vec!["spas".to_string()];
    let id1 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp1,
    )
    .unwrap();

    let rec1 = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id: id1,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "spaz".to_string(),
        context: None,
        expectation: exp1,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    let mut exp2 = create_base_expectation();
    exp2.expected_candidates = vec!["rojbaş".to_string()];
    let id2 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp2,
    )
    .unwrap();

    let rec2 = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id: id2,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "spaz".to_string(),
        context: None,
        expectation: exp2,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    let content = format!(
        "{}\n{}\n",
        serde_json::to_string(&rec1).unwrap(),
        serde_json::to_string(&rec2).unwrap()
    );
    fs::write(eval_dir.join("reviewed-cases.jsonl"), content).unwrap();

    assert!(validate_benchmark_case_set(temp.path()).is_err());
}

#[test]
fn test_same_input_different_categories_allowed() {
    let temp = tempdir().unwrap();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let exp1 = BenchmarkExpectation {
        accepted: Some(true),
        preserve_exact: Some(true),
        expected_candidates: vec![],
        forbidden_candidates: vec![],
        allow_no_candidate: None,
        required_top_k: None,
    };
    let id1 = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ExactPreservation,
        "spas",
        None,
        &exp1,
    )
    .unwrap();

    let rec1 = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id: id1,
        task: BenchmarkTask::AcceptWord,
        category: BenchmarkCategory::ExactPreservation,
        input: "spas".to_string(),
        context: None,
        expectation: exp1,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    let exp2 = BenchmarkExpectation {
        accepted: Some(true),
        preserve_exact: None,
        expected_candidates: vec![],
        forbidden_candidates: vec![],
        allow_no_candidate: None,
        required_top_k: None,
    };
    let id2 = compute_canonical_case_id(
        BenchmarkTask::AcceptWord,
        BenchmarkCategory::ProperNoun,
        "spas",
        None,
        &exp2,
    )
    .unwrap();

    let rec2 = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id: id2,
        task: BenchmarkTask::AcceptWord,
        category: BenchmarkCategory::ProperNoun,
        input: "spas".to_string(),
        context: None,
        expectation: exp2,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: Some("Proper noun note".to_string()),
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    let content = format!(
        "{}\n{}\n",
        serde_json::to_string(&rec1).unwrap(),
        serde_json::to_string(&rec2).unwrap()
    );
    fs::write(eval_dir.join("reviewed-cases.jsonl"), content).unwrap();

    let res = validate_benchmark_case_set(temp.path()).unwrap();
    assert_eq!(res.total_cases, 2);
}

#[test]
fn test_missing_seed_lexicon_fails_provenance() {
    let temp = tempdir().unwrap();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let hunspell_dir = temp.path().join("data/imported/kurdish-hunspell-kmr");
    fs::create_dir_all(&hunspell_dir).unwrap();
    fs::write(hunspell_dir.join("lexicon.jsonl"), "").unwrap();

    let err = generate_provenance_report(temp.path()).unwrap_err();
    assert!(err.contains("Required seed lexicon file missing"));
}

#[test]
fn test_missing_hunspell_lexicon_fails_provenance() {
    let temp = tempdir().unwrap();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let seed_dir = temp.path().join("data/reviewed");
    fs::create_dir_all(&seed_dir).unwrap();
    fs::write(seed_dir.join("lexicon.jsonl"), "").unwrap();

    let err = generate_provenance_report(temp.path()).unwrap_err();
    assert!(err.contains("Required Hunspell lexicon file missing"));
}

#[test]
fn test_delimiter_ambiguity_no_false_duplicates() {
    let temp = tempdir().unwrap();
    let eval_dir = temp.path().join("evaluation/spelling");
    fs::create_dir_all(&eval_dir).unwrap();

    let exp = create_base_expectation();

    let id1 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "a|b",
        Some(&["c".to_string()]),
        &exp,
    )
    .unwrap();

    let rec1 = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id: id1,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "a|b".to_string(),
        context: Some(vec!["c".to_string()]),
        expectation: exp.clone(),
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    let id2 = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "a",
        Some(&["b|c".to_string()]),
        &exp,
    )
    .unwrap();

    let rec2 = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id: id2,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "a".to_string(),
        context: Some(vec!["b|c".to_string()]),
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    let content = format!(
        "{}\n{}\n",
        serde_json::to_string(&rec1).unwrap(),
        serde_json::to_string(&rec2).unwrap()
    );
    fs::write(eval_dir.join("reviewed-cases.jsonl"), content).unwrap();

    let res = validate_benchmark_case_set(temp.path()).unwrap();
    assert_eq!(res.total_cases, 2);
}

#[test]
fn test_nul_containing_context_rejected() {
    let exp = create_base_expectation();
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        Some(&["foo\0bar".to_string()]),
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "spaz".to_string(),
        context: Some(vec!["foo\0bar".to_string()]),
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    assert!(validate_case_record(&record).is_err());
}

#[test]
fn test_non_nfc_context_rejected() {
    let exp = create_base_expectation();
    // "e\u{302}" is NFD ê
    let nfd_ctx = "e\u{302}";
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        Some(&[nfd_ctx.to_string()]),
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "spaz".to_string(),
        context: Some(vec![nfd_ctx.to_string()]),
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    assert!(validate_case_record(&record).is_err());
}

#[test]
fn test_empty_context_item_rejected() {
    let exp = create_base_expectation();
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        Some(&["   ".to_string()]),
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "spaz".to_string(),
        context: Some(vec!["   ".to_string()]),
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    assert!(validate_case_record(&record).is_err());
}

#[test]
fn test_unknown_json_field_rejected() {
    let json = r#"{
        "schema_version": "benchmark-case-v1",
        "case_id": "dummy",
        "task": "correct-word",
        "category": "substitution",
        "input": "spaz",
        "expectation": {
            "expected_candidates": ["spas"],
            "unknown_exp_field": true
        },
        "review_status": "human-reviewed",
        "reviewer_id": "linguist-001",
        "review_date": "2026-08-03",
        "source": {
            "kind": "manual"
        },
        "unknown_top_level_field": "invalid"
    }"#;

    let res: Result<BenchmarkCaseRecord, _> = serde_json::from_str(json);
    assert!(res.is_err());
}

#[test]
fn test_empty_expected_or_forbidden_candidate_rejected() {
    let mut exp = create_base_expectation();
    exp.expected_candidates = vec!["  ".to_string()];
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "spaz".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::Manual,
            source_id: None,
            source_document_id: None,
            source_record: None,
        },
    };

    assert!(validate_case_record(&record).is_err());
}

#[test]
fn test_held_out_source_without_doc_id_rejected() {
    let exp = create_base_expectation();
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "spaz".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("linguist-001".to_string()),
        review_date: Some("2026-08-03".to_string()),
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::HeldOutCorpus,
            source_id: Some("opensubtitles".to_string()),
            source_document_id: None, // missing required doc id
            source_record: None,
        },
    };

    assert!(validate_case_record(&record).is_err());
}

#[test]
fn test_ai_assisted_source_without_source_id_rejected() {
    let exp = create_base_expectation();
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "spaz",
        None,
        &exp,
    )
    .unwrap();

    let record = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "spaz".to_string(),
        context: None,
        expectation: exp,
        review_status: BenchmarkReviewStatus::Draft,
        reviewer_id: None,
        review_date: None,
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::AiAssistedDraft,
            source_id: None, // missing required source_id
            source_document_id: None,
            source_record: None,
        },
    };

    assert!(validate_case_record(&record).is_err());
}

#[test]
fn test_reviewer_id_accepts_segmented_human_and_body_conventions() {
    for reviewer_id in [
        "abc",
        "reviewer-ferhat",
        "reviewer-linguist-01",
        "123-reviewer-456",
        "institution-example",
        "committee-orthography",
    ] {
        assert!(
            validate_reviewer_id(reviewer_id).is_ok(),
            "expected reviewer ID '{reviewer_id}' to be valid"
        );
    }
    assert!(validate_reviewer_id(&"a".repeat(64)).is_ok());
}

#[test]
fn test_reviewer_id_rejects_invalid_structure_and_lengths() {
    for reviewer_id in [
        "",
        "ab",
        "---",
        "-reviewer",
        "reviewer-",
        "reviewer--ferhat",
        "Reviewer-ferhat",
        "reviewer_ferhat",
        "reviewer-ferhat\u{00ee}",
        "123",
        "123-456",
        "000-001",
    ] {
        assert!(
            validate_reviewer_id(reviewer_id).is_err(),
            "expected reviewer ID '{reviewer_id}' to be rejected"
        );
    }
    assert!(validate_reviewer_id(&"a".repeat(65)).is_err());
}

#[test]
fn test_reviewer_id_rejects_reserved_automation_segments_anywhere() {
    for segment in [
        "ai",
        "auto",
        "automatic",
        "bot",
        "system",
        "assistant",
        "chatgpt",
    ] {
        for reviewer_id in [
            format!("{segment}-reviewer"),
            format!("reviewer-{segment}"),
            format!("committee-{segment}-review"),
        ] {
            assert!(
                validate_reviewer_id(&reviewer_id).is_err(),
                "expected reviewer ID '{reviewer_id}' to be rejected"
            );
        }
    }
}

#[test]
fn test_review_date_accepts_exact_gregorian_boundaries_and_leap_day() {
    for date in ["0001-01-01", "2000-02-29", "9999-12-31"] {
        assert!(
            validate_review_date(date).is_ok(),
            "expected review date '{date}' to be valid"
        );
    }
}

#[test]
fn test_review_date_rejects_invalid_format_range_and_calendar_dates() {
    for date in [
        "0000-01-01",
        "10000-01-01",
        "2026-8-03",
        "2026-08-3",
        "2026-08-03T00:00:00Z",
        "1900-02-29",
        "2026-02-29",
        "2026-00-01",
        "2026-13-01",
        "2026-04-31",
    ] {
        assert!(
            validate_review_date(date).is_err(),
            "expected review date '{date}' to be rejected"
        );
    }
}

#[test]
fn test_human_reviewed_record_requires_valid_reviewer_metadata() {
    let expectation = create_base_expectation();
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "fixture-review-input",
        None,
        &expectation,
    )
    .unwrap();
    let valid = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "fixture-review-input".to_string(),
        context: None,
        expectation,
        review_status: BenchmarkReviewStatus::HumanReviewed,
        reviewer_id: Some("reviewer-test-fixture".to_string()),
        review_date: Some("2000-01-01".to_string()),
        review_notes: Some("Synthetic schema-validation fixture.".to_string()),
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::MechanicalDraft,
            source_id: Some("schema-test-fixture".to_string()),
            source_document_id: None,
            source_record: Some("fixture-record".to_string()),
        },
    };
    assert!(validate_case_record(&valid).is_ok());

    let mut missing_reviewer = valid.clone();
    missing_reviewer.reviewer_id = None;
    assert!(validate_case_record(&missing_reviewer).is_err());

    let mut automated_reviewer = valid.clone();
    automated_reviewer.reviewer_id = Some("reviewer-bot".to_string());
    assert!(validate_case_record(&automated_reviewer).is_err());

    let mut missing_date = valid.clone();
    missing_date.review_date = None;
    assert!(validate_case_record(&missing_date).is_err());

    let mut invalid_date = valid;
    invalid_date.review_date = Some("2000-02-30".to_string());
    assert!(validate_case_record(&invalid_date).is_err());
}

#[test]
fn test_draft_record_cannot_contain_review_identity_or_date() {
    let expectation = create_base_expectation();
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        "fixture-draft-input",
        None,
        &expectation,
    )
    .unwrap();
    let draft = BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: "fixture-draft-input".to_string(),
        context: None,
        expectation,
        review_status: BenchmarkReviewStatus::Draft,
        reviewer_id: None,
        review_date: None,
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::MechanicalDraft,
            source_id: Some("schema-test-fixture".to_string()),
            source_document_id: None,
            source_record: Some("fixture-record".to_string()),
        },
    };
    assert!(validate_case_record(&draft).is_ok());

    let mut with_reviewer = draft.clone();
    with_reviewer.reviewer_id = Some("reviewer-test-fixture".to_string());
    assert!(validate_case_record(&with_reviewer).is_err());

    let mut with_date = draft;
    with_date.review_date = Some("2000-01-01".to_string());
    assert!(validate_case_record(&with_date).is_err());
}

#[test]
fn test_source_provenance_fingerprint_changes_with_each_source_field() {
    let source = BenchmarkSourceInfo {
        kind: BenchmarkSourceKind::MechanicalDraft,
        source_id: Some("source-fixture".to_string()),
        source_document_id: Some("document-fixture".to_string()),
        source_record: Some("record-fixture".to_string()),
    };
    let original = compute_source_provenance_sha256(&source).unwrap();

    let mut changed_kind = source.clone();
    changed_kind.kind = BenchmarkSourceKind::AiAssistedDraft;
    assert_ne!(
        original,
        compute_source_provenance_sha256(&changed_kind).unwrap()
    );

    let mut changed_source_id = source.clone();
    changed_source_id.source_id = Some("different-source".to_string());
    assert_ne!(
        original,
        compute_source_provenance_sha256(&changed_source_id).unwrap()
    );

    let mut changed_document = source.clone();
    changed_document.source_document_id = Some("different-document".to_string());
    assert_ne!(
        original,
        compute_source_provenance_sha256(&changed_document).unwrap()
    );

    let mut changed_record = source;
    changed_record.source_record = Some("different-record".to_string());
    assert_ne!(
        original,
        compute_source_provenance_sha256(&changed_record).unwrap()
    );
}

#[test]
fn test_regression_no_tests_modify_authoritative_evaluation_data() {
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| std::path::PathBuf::from(d).join(".."))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let draft_path = root.join("evaluation/spelling/draft-cases.jsonl");
    let reviewed_path = root.join("evaluation/spelling/reviewed-cases.jsonl");

    let draft_before = include_bytes!("../../evaluation/spelling/draft-cases.jsonl");
    let reviewed_before = include_bytes!("../../evaluation/spelling/reviewed-cases.jsonl");
    let draft_after = fs::read(&draft_path).expect("failed to read draft-cases.jsonl");
    let reviewed_after = fs::read(&reviewed_path).expect("failed to read reviewed-cases.jsonl");

    assert_eq!(
        draft_after.as_slice(),
        draft_before,
        "tests must not modify draft-cases.jsonl"
    );
    assert_eq!(
        reviewed_after.as_slice(),
        reviewed_before,
        "tests must not modify reviewed-cases.jsonl"
    );
}
