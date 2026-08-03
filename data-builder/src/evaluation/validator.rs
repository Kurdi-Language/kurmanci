//! Benchmark case set validator and integrity checking.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::evaluation::reports::load_benchmark_cases;
use crate::evaluation::schema::{
    encode_canonical_expectation, BenchmarkCategory, BenchmarkReviewStatus, BenchmarkTask,
};

/// Result of benchmark case set validation.
#[derive(Debug, Clone)]
pub struct BenchmarkValidationResult {
    pub total_cases: usize,
    pub human_reviewed_cases: usize,
    pub draft_cases: usize,
    pub benchmark_ready: bool,
    pub task_counts: BTreeMap<String, usize>,
    pub category_counts: BTreeMap<String, usize>,
}

/// Structured key for contradiction detection without delimiter string joining.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ContradictionKey {
    pub task: BenchmarkTask,
    pub category: BenchmarkCategory,
    pub input: String,
    pub context: Option<Vec<String>>,
}

/// Structured key for exact duplicate case detection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExactDuplicateKey {
    pub task: BenchmarkTask,
    pub category: BenchmarkCategory,
    pub input: String,
    pub context: Option<Vec<String>>,
    pub exp_bytes: Vec<u8>,
}

/// Validates benchmark case records across `draft-cases.jsonl` and `reviewed-cases.jsonl`.
pub fn validate_benchmark_case_set<P: AsRef<Path>>(
    root_dir: P,
) -> Result<BenchmarkValidationResult, String> {
    let root = root_dir.as_ref();
    let draft_path = root.join("evaluation/spelling/draft-cases.jsonl");
    let reviewed_path = root.join("evaluation/spelling/reviewed-cases.jsonl");
    validate_benchmark_case_files(&draft_path, &reviewed_path)
}

/// Validates an explicitly provided draft/reviewed benchmark snapshot.
pub fn validate_benchmark_case_files(
    draft_path: &Path,
    reviewed_path: &Path,
) -> Result<BenchmarkValidationResult, String> {
    let mut all_records = Vec::new();

    if draft_path.exists() {
        let draft_records = load_benchmark_cases(draft_path)?;
        for rec in draft_records {
            if rec.review_status != BenchmarkReviewStatus::Draft {
                return Err(format!(
                    "Record '{}' in draft-cases.jsonl must have review_status = 'draft'",
                    rec.case_id
                ));
            }
            all_records.push(rec);
        }
    }

    if reviewed_path.exists() {
        let reviewed_records = load_benchmark_cases(reviewed_path)?;
        for rec in reviewed_records {
            if rec.review_status != BenchmarkReviewStatus::HumanReviewed {
                return Err(format!(
                    "Record '{}' in reviewed-cases.jsonl must have review_status = 'human-reviewed'",
                    rec.case_id
                ));
            }
            all_records.push(rec);
        }
    }

    let mut seen_ids = BTreeSet::new();
    let mut seen_semantic_keys = BTreeSet::new();
    let mut seen_input_expectations: BTreeMap<ContradictionKey, Vec<u8>> = BTreeMap::new();

    let mut result = BenchmarkValidationResult {
        total_cases: 0,
        human_reviewed_cases: 0,
        draft_cases: 0,
        benchmark_ready: false,
        task_counts: BTreeMap::new(),
        category_counts: BTreeMap::new(),
    };

    for rec in &all_records {
        // 1. Check case_id uniqueness
        if !seen_ids.insert(rec.case_id.clone()) {
            return Err(format!("Duplicate case_id '{}' detected", rec.case_id));
        }

        // 2. Canonical expectation bytes
        let exp_bytes = encode_canonical_expectation(&rec.expectation)?;

        // 3. Exact duplicate structured key
        let dup_key = ExactDuplicateKey {
            task: rec.task,
            category: rec.category,
            input: rec.input.clone(),
            context: rec.context.clone(),
            exp_bytes: exp_bytes.clone(),
        };

        if !seen_semantic_keys.insert(dup_key) {
            return Err(format!(
                "Exact duplicate semantic case detected for input '{}' with case_id '{}'",
                rec.input, rec.case_id
            ));
        }

        // 4. Contradiction structured key
        let contra_key = ContradictionKey {
            task: rec.task,
            category: rec.category,
            input: rec.input.clone(),
            context: rec.context.clone(),
        };

        if let Some(prev_exp_bytes) = seen_input_expectations.get(&contra_key) {
            if prev_exp_bytes != &exp_bytes {
                return Err(format!(
                    "Contradictory benchmark expectations for task '{:?}', category '{:?}', and input '{}' across cases",
                    rec.task, rec.category, rec.input
                ));
            }
        } else {
            seen_input_expectations.insert(contra_key, exp_bytes);
        }

        // Update counts
        result.total_cases += 1;
        match rec.review_status {
            BenchmarkReviewStatus::HumanReviewed => result.human_reviewed_cases += 1,
            BenchmarkReviewStatus::Draft => result.draft_cases += 1,
        }

        let t_str = rec.task.as_str().to_string();
        *result.task_counts.entry(t_str).or_default() += 1;

        let c_str = rec.category.as_str().to_string();
        *result.category_counts.entry(c_str).or_default() += 1;
    }

    result.benchmark_ready = result.human_reviewed_cases > 0;

    Ok(result)
}
