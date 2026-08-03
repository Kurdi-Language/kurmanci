//! Snapshot transition tests use synthetic temporary fixtures only.

use data_builder_lib::evaluation::schema::{
    compute_canonical_case_id, compute_source_provenance_sha256, BenchmarkCaseRecord,
    BenchmarkCategory, BenchmarkExpectation, BenchmarkReviewStatus, BenchmarkSourceInfo,
    BenchmarkSourceKind, BenchmarkTask, BENCHMARK_CASE_SCHEMA_VERSION,
};
use data_builder_lib::evaluation::transition::validate_evaluation_transition;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

struct SnapshotPaths {
    base_draft: PathBuf,
    base_reviewed: PathBuf,
    candidate_draft: PathBuf,
    candidate_reviewed: PathBuf,
}

fn fixture_draft(input: &str, expected: &str, source_record: &str) -> BenchmarkCaseRecord {
    let expectation = BenchmarkExpectation {
        expected_candidates: vec![expected.to_string()],
        forbidden_candidates: Vec::new(),
        required_top_k: Some(5),
        ..Default::default()
    };
    let case_id = compute_canonical_case_id(
        BenchmarkTask::CorrectWord,
        BenchmarkCategory::Substitution,
        input,
        None,
        &expectation,
    )
    .unwrap();

    BenchmarkCaseRecord {
        schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        case_id,
        task: BenchmarkTask::CorrectWord,
        category: BenchmarkCategory::Substitution,
        input: input.to_string(),
        context: None,
        expectation,
        review_status: BenchmarkReviewStatus::Draft,
        reviewer_id: None,
        review_date: None,
        review_notes: None,
        source: BenchmarkSourceInfo {
            kind: BenchmarkSourceKind::MechanicalDraft,
            source_id: Some("transition-test-fixture".to_string()),
            source_document_id: None,
            source_record: Some(source_record.to_string()),
        },
    }
}

fn promote(draft: &BenchmarkCaseRecord) -> BenchmarkCaseRecord {
    let mut reviewed = draft.clone();
    reviewed.review_status = BenchmarkReviewStatus::HumanReviewed;
    reviewed.reviewer_id = Some("reviewer-test-fixture".to_string());
    reviewed.review_date = Some("2000-01-01".to_string());
    reviewed.review_notes = Some("Synthetic transition-validator fixture.".to_string());
    reviewed
}

fn write_records(path: &Path, records: &[BenchmarkCaseRecord]) {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).unwrap();
        bytes.push(b'\n');
    }
    fs::write(path, bytes).unwrap();
}

fn write_snapshots(
    base_draft: &[BenchmarkCaseRecord],
    base_reviewed: &[BenchmarkCaseRecord],
    candidate_draft: &[BenchmarkCaseRecord],
    candidate_reviewed: &[BenchmarkCaseRecord],
) -> (tempfile::TempDir, SnapshotPaths) {
    let temp = tempdir().unwrap();
    let paths = SnapshotPaths {
        base_draft: temp.path().join("base-draft.jsonl"),
        base_reviewed: temp.path().join("base-reviewed.jsonl"),
        candidate_draft: temp.path().join("candidate-draft.jsonl"),
        candidate_reviewed: temp.path().join("candidate-reviewed.jsonl"),
    };
    write_records(&paths.base_draft, base_draft);
    write_records(&paths.base_reviewed, base_reviewed);
    write_records(&paths.candidate_draft, candidate_draft);
    write_records(&paths.candidate_reviewed, candidate_reviewed);
    (temp, paths)
}

fn validate(paths: &SnapshotPaths) -> Result<(), String> {
    validate_evaluation_transition(
        &paths.base_draft,
        &paths.base_reviewed,
        &paths.candidate_draft,
        &paths.candidate_reviewed,
    )
    .map(|_| ())
}

#[test]
fn metadata_only_promotion_succeeds_and_preserves_provenance_hash() {
    let draft = fixture_draft("fixture-input-a", "fixture-output-a", "fixture-record-a");
    let reviewed = promote(&draft);
    assert_eq!(
        compute_source_provenance_sha256(&draft.source).unwrap(),
        compute_source_provenance_sha256(&reviewed.source).unwrap()
    );

    let (_temp, paths) = write_snapshots(
        std::slice::from_ref(&draft),
        &[],
        &[],
        std::slice::from_ref(&reviewed),
    );
    let summary = validate_evaluation_transition(
        &paths.base_draft,
        &paths.base_reviewed,
        &paths.candidate_draft,
        &paths.candidate_reviewed,
    )
    .unwrap();

    assert_eq!(summary.promoted_cases, 1);
    assert_eq!(summary.candidate_draft_cases, 0);
    assert_eq!(summary.candidate_reviewed_cases, 1);
}

#[test]
fn ai_assisted_origin_survives_promotion_and_cannot_change_to_manual() {
    let mut draft = fixture_draft(
        "fixture-input-ai-origin",
        "fixture-output-ai-origin",
        "fixture-record-ai-origin",
    );
    draft.source.kind = BenchmarkSourceKind::AiAssistedDraft;
    let reviewed = promote(&draft);
    assert_eq!(reviewed.source.kind, BenchmarkSourceKind::AiAssistedDraft);

    let (_valid_temp, valid_paths) = write_snapshots(
        std::slice::from_ref(&draft),
        &[],
        &[],
        std::slice::from_ref(&reviewed),
    );
    validate(&valid_paths).unwrap();

    let mut changed_origin = reviewed;
    changed_origin.source.kind = BenchmarkSourceKind::Manual;
    let (_invalid_temp, invalid_paths) =
        write_snapshots(std::slice::from_ref(&draft), &[], &[], &[changed_origin]);
    let error = validate(&invalid_paths).unwrap_err();
    assert!(error.contains("changed source provenance"));
}

#[test]
fn ordinary_draft_creation_revision_and_removal_are_permitted() {
    let removed = fixture_draft("fixture-input-b", "fixture-output-b", "fixture-record-b");
    let revised_before = fixture_draft("fixture-input-c", "fixture-output-c", "fixture-record-c");
    let revised_after = fixture_draft("fixture-input-c", "fixture-output-c2", "fixture-record-c");
    let created = fixture_draft("fixture-input-d", "fixture-output-d", "fixture-record-d");

    let (_temp, paths) = write_snapshots(
        &[removed, revised_before],
        &[],
        &[revised_after, created],
        &[],
    );
    validate(&paths).unwrap();
}

#[test]
fn promotion_with_changed_expectation_is_rejected() {
    let draft = fixture_draft("fixture-input-e", "fixture-output-e", "fixture-record-e");
    let changed_draft = fixture_draft("fixture-input-e", "fixture-output-e2", "fixture-record-e");
    let changed_reviewed = promote(&changed_draft);
    let (_temp, paths) =
        write_snapshots(std::slice::from_ref(&draft), &[], &[], &[changed_reviewed]);

    assert!(validate(&paths).is_err());
}

#[test]
fn promotion_with_changed_input_or_context_is_rejected() {
    let draft = fixture_draft("fixture-input-f", "fixture-output-f", "fixture-record-f");
    let mut changed = fixture_draft("fixture-input-f2", "fixture-output-f", "fixture-record-f");
    changed.context = Some(vec!["fixture-context".to_string()]);
    changed.case_id = compute_canonical_case_id(
        changed.task,
        changed.category,
        &changed.input,
        changed.context.as_deref(),
        &changed.expectation,
    )
    .unwrap();
    let changed_reviewed = promote(&changed);
    let (_temp, paths) = write_snapshots(&[draft], &[], &[], &[changed_reviewed]);

    assert!(validate(&paths).is_err());
}

#[test]
fn promotion_with_changed_source_provenance_is_rejected() {
    let draft = fixture_draft("fixture-input-g", "fixture-output-g", "fixture-record-g");
    let mut reviewed = promote(&draft);
    reviewed.source.source_record = Some("changed-fixture-record".to_string());
    let (_temp, paths) = write_snapshots(&[draft], &[], &[], &[reviewed]);

    let error = validate(&paths).unwrap_err();
    assert!(error.contains("changed source provenance"));
}

#[test]
fn promoted_case_must_be_removed_from_candidate_draft() {
    let draft = fixture_draft("fixture-input-h", "fixture-output-h", "fixture-record-h");
    let reviewed = promote(&draft);
    let (_temp, paths) = write_snapshots(
        std::slice::from_ref(&draft),
        &[],
        std::slice::from_ref(&draft),
        std::slice::from_ref(&reviewed),
    );

    assert!(validate(&paths).is_err());
}

#[test]
fn unexplained_new_reviewed_record_is_rejected() {
    let draft = fixture_draft("fixture-input-i", "fixture-output-i", "fixture-record-i");
    let reviewed = promote(&draft);
    let (_temp, paths) = write_snapshots(&[], &[], &[], &[reviewed]);

    let error = validate(&paths).unwrap_err();
    assert!(error.contains("no matching base draft"));
}

#[test]
fn reviewed_record_cannot_be_downgraded_to_draft() {
    let draft = fixture_draft("fixture-input-j", "fixture-output-j", "fixture-record-j");
    let reviewed = promote(&draft);
    let (_temp, paths) = write_snapshots(&[], &[reviewed], &[draft], &[]);

    let error = validate(&paths).unwrap_err();
    assert!(error.contains("downgraded to draft"));
}

#[test]
fn reviewed_record_cannot_be_removed() {
    let reviewed = promote(&fixture_draft(
        "fixture-input-k",
        "fixture-output-k",
        "fixture-record-k",
    ));
    let (_temp, paths) = write_snapshots(&[], &[reviewed], &[], &[]);

    let error = validate(&paths).unwrap_err();
    assert!(error.contains("was removed"));
}

#[test]
fn existing_reviewed_record_cannot_be_modified() {
    let reviewed = promote(&fixture_draft(
        "fixture-input-l",
        "fixture-output-l",
        "fixture-record-l",
    ));
    let mut changed = reviewed.clone();
    changed.review_notes = Some("Changed synthetic fixture note.".to_string());
    let (_temp, paths) = write_snapshots(&[], &[reviewed], &[], &[changed]);

    let error = validate(&paths).unwrap_err();
    assert!(error.contains("was modified"));
}
