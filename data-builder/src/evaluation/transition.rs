//! Snapshot-based benchmark review transition validation.

use std::collections::BTreeMap;
use std::path::Path;

use crate::evaluation::reports::load_benchmark_cases;
use crate::evaluation::schema::{
    compute_source_provenance_sha256, BenchmarkCaseRecord, BenchmarkReviewStatus,
};
use crate::evaluation::validator::validate_benchmark_case_files;

/// Counts produced after a valid base-to-candidate benchmark transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationTransitionSummary {
    pub base_draft_cases: usize,
    pub base_reviewed_cases: usize,
    pub candidate_draft_cases: usize,
    pub candidate_reviewed_cases: usize,
    pub promoted_cases: usize,
}

fn records_by_id(
    records: Vec<BenchmarkCaseRecord>,
    path: &Path,
) -> Result<BTreeMap<String, BenchmarkCaseRecord>, String> {
    let mut by_id = BTreeMap::new();
    for record in records {
        let case_id = record.case_id.clone();
        if by_id.insert(case_id.clone(), record).is_some() {
            return Err(format!(
                "Duplicate case_id '{}' detected in {:?}",
                case_id, path
            ));
        }
    }
    Ok(by_id)
}

fn require_unchanged_promotion_semantics(
    base: &BenchmarkCaseRecord,
    candidate: &BenchmarkCaseRecord,
) -> Result<(), String> {
    let case_id = &base.case_id;
    if base.schema_version != candidate.schema_version {
        return Err(format!(
            "Promotion for case '{}' changed schema_version",
            case_id
        ));
    }
    if base.case_id != candidate.case_id {
        return Err(format!("Promotion for case '{}' changed case_id", case_id));
    }
    if base.task != candidate.task {
        return Err(format!("Promotion for case '{}' changed task", case_id));
    }
    if base.category != candidate.category {
        return Err(format!("Promotion for case '{}' changed category", case_id));
    }
    if base.input != candidate.input {
        return Err(format!("Promotion for case '{}' changed input", case_id));
    }
    if base.context != candidate.context {
        return Err(format!("Promotion for case '{}' changed context", case_id));
    }
    if base.expectation != candidate.expectation {
        return Err(format!(
            "Promotion for case '{}' changed expectation",
            case_id
        ));
    }
    if base.source != candidate.source {
        return Err(format!(
            "Promotion for case '{}' changed source provenance",
            case_id
        ));
    }

    let base_provenance = compute_source_provenance_sha256(&base.source)?;
    let candidate_provenance = compute_source_provenance_sha256(&candidate.source)?;
    if base_provenance != candidate_provenance {
        return Err(format!(
            "Promotion for case '{}' changed source provenance fingerprint",
            case_id
        ));
    }

    Ok(())
}

/// Validates a transition between explicit base and candidate benchmark snapshots.
///
/// Draft records may be freely created, revised, or removed. Existing authoritative
/// reviewed records are immutable in ordinary promotion mode. A new reviewed record
/// must match a base draft semantically and may differ only in review metadata.
pub fn validate_evaluation_transition(
    base_draft_path: &Path,
    base_reviewed_path: &Path,
    candidate_draft_path: &Path,
    candidate_reviewed_path: &Path,
) -> Result<EvaluationTransitionSummary, String> {
    validate_benchmark_case_files(base_draft_path, base_reviewed_path)
        .map_err(|error| format!("Invalid base benchmark snapshot: {error}"))?;
    validate_benchmark_case_files(candidate_draft_path, candidate_reviewed_path)
        .map_err(|error| format!("Invalid candidate benchmark snapshot: {error}"))?;

    let base_draft = records_by_id(load_benchmark_cases(base_draft_path)?, base_draft_path)?;
    let base_reviewed = records_by_id(
        load_benchmark_cases(base_reviewed_path)?,
        base_reviewed_path,
    )?;
    let candidate_draft = records_by_id(
        load_benchmark_cases(candidate_draft_path)?,
        candidate_draft_path,
    )?;
    let candidate_reviewed = records_by_id(
        load_benchmark_cases(candidate_reviewed_path)?,
        candidate_reviewed_path,
    )?;

    for (case_id, base_record) in &base_reviewed {
        match candidate_reviewed.get(case_id) {
            Some(candidate_record) if candidate_record == base_record => {}
            Some(_) => {
                return Err(format!(
                    "Authoritative reviewed case '{}' was modified; ordinary promotion mode requires reviewed records to remain unchanged",
                    case_id
                ));
            }
            None if candidate_draft.contains_key(case_id) => {
                return Err(format!(
                    "Authoritative reviewed case '{}' was downgraded to draft",
                    case_id
                ));
            }
            None => {
                return Err(format!(
                    "Authoritative reviewed case '{}' was removed",
                    case_id
                ));
            }
        }
    }

    let mut promoted_cases = 0usize;
    for (case_id, candidate_record) in &candidate_reviewed {
        if base_reviewed.contains_key(case_id) {
            continue;
        }

        let base_record = base_draft.get(case_id).ok_or_else(|| {
            format!(
                "New human-reviewed case '{}' has no matching base draft",
                case_id
            )
        })?;
        if candidate_draft.contains_key(case_id) {
            return Err(format!(
                "Promoted case '{}' must be removed from the candidate draft file",
                case_id
            ));
        }
        if base_record.review_status != BenchmarkReviewStatus::Draft
            || candidate_record.review_status != BenchmarkReviewStatus::HumanReviewed
        {
            return Err(format!(
                "Case '{}' is not a valid draft-to-human-reviewed promotion",
                case_id
            ));
        }
        require_unchanged_promotion_semantics(base_record, candidate_record)?;
        promoted_cases = promoted_cases
            .checked_add(1)
            .ok_or_else(|| "Promoted case count overflow".to_string())?;
    }

    Ok(EvaluationTransitionSummary {
        base_draft_cases: base_draft.len(),
        base_reviewed_cases: base_reviewed.len(),
        candidate_draft_cases: candidate_draft.len(),
        candidate_reviewed_cases: candidate_reviewed.len(),
        promoted_cases,
    })
}
