//! Typed schemas, canonical identity generation, and validation for evaluation benchmark cases.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const BENCHMARK_CASE_SCHEMA_VERSION: &str = "benchmark-case-v1";
pub const BENCHMARK_CASE_DOMAIN_TAG: &str = "kurmanci-spelling-case-v1";

/// The specific engine task being evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkTask {
    AcceptWord,
    CorrectWord,
    CompletePrefix,
}

impl BenchmarkTask {
    pub fn as_str(&self) -> &'static str {
        match self {
            BenchmarkTask::AcceptWord => "accept-word",
            BenchmarkTask::CorrectWord => "correct-word",
            BenchmarkTask::CompletePrefix => "complete-prefix",
        }
    }
}

/// Category describing the linguistic or structural nature of the case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkCategory {
    CorrectSpelling,
    MissingDiacritics,
    Substitution,
    Insertion,
    Deletion,
    Transposition,
    MultiEdit,
    PrefixCompletion,
    ExactPreservation,
    CommonVsRare,
    ProperNoun,
    Morphology,
    RegionalVariant,
    UnknownWord,
    FalseAcceptance,
    NoCandidate,
}

impl BenchmarkCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            BenchmarkCategory::CorrectSpelling => "correct-spelling",
            BenchmarkCategory::MissingDiacritics => "missing-diacritics",
            BenchmarkCategory::Substitution => "substitution",
            BenchmarkCategory::Insertion => "insertion",
            BenchmarkCategory::Deletion => "deletion",
            BenchmarkCategory::Transposition => "transposition",
            BenchmarkCategory::MultiEdit => "multi-edit",
            BenchmarkCategory::PrefixCompletion => "prefix-completion",
            BenchmarkCategory::ExactPreservation => "exact-preservation",
            BenchmarkCategory::CommonVsRare => "common-vs-rare",
            BenchmarkCategory::ProperNoun => "proper-noun",
            BenchmarkCategory::Morphology => "morphology",
            BenchmarkCategory::RegionalVariant => "regional-variant",
            BenchmarkCategory::UnknownWord => "unknown-word",
            BenchmarkCategory::FalseAcceptance => "false-acceptance",
            BenchmarkCategory::NoCandidate => "no-candidate",
        }
    }
}

/// Checks task and category compatibility per the project compatibility matrix.
pub fn is_compatible_task_category(task: BenchmarkTask, category: BenchmarkCategory) -> bool {
    match task {
        BenchmarkTask::AcceptWord => matches!(
            category,
            BenchmarkCategory::CorrectSpelling
                | BenchmarkCategory::ExactPreservation
                | BenchmarkCategory::ProperNoun
                | BenchmarkCategory::Morphology
                | BenchmarkCategory::RegionalVariant
                | BenchmarkCategory::UnknownWord
                | BenchmarkCategory::FalseAcceptance
        ),
        BenchmarkTask::CorrectWord => matches!(
            category,
            BenchmarkCategory::MissingDiacritics
                | BenchmarkCategory::Substitution
                | BenchmarkCategory::Insertion
                | BenchmarkCategory::Deletion
                | BenchmarkCategory::Transposition
                | BenchmarkCategory::MultiEdit
                | BenchmarkCategory::CommonVsRare
                | BenchmarkCategory::NoCandidate
        ),
        BenchmarkTask::CompletePrefix => matches!(category, BenchmarkCategory::PrefixCompletion),
    }
}

/// Review status of the benchmark record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkReviewStatus {
    Draft,
    HumanReviewed,
}

/// Origin kind of the benchmark case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkSourceKind {
    Manual,
    HeldOutCorpus,
    MechanicalDraft,
    AiAssistedDraft,
}

/// Provenance metadata detailing origin of the case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSourceInfo {
    pub kind: BenchmarkSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_record: Option<String>,
}

/// Task-specific expected behavior for evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkExpectation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserve_exact: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_candidates: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_candidates: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_no_candidate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_top_k: Option<usize>,
}

/// Individual benchmark case record in draft-cases.jsonl or reviewed-cases.jsonl.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkCaseRecord {
    pub schema_version: String,
    pub case_id: String,
    pub task: BenchmarkTask,
    pub category: BenchmarkCategory,
    pub input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<String>>,
    pub expectation: BenchmarkExpectation,
    pub review_status: BenchmarkReviewStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_notes: Option<String>,
    pub source: BenchmarkSourceInfo,
}

fn is_nfc_normalized(s: &str) -> bool {
    s.chars().nfc().collect::<String>() == s
}

fn checked_u64_len(len: usize) -> Result<[u8; 8], String> {
    u64::try_from(len)
        .map(|v| v.to_be_bytes())
        .map_err(|_| format!("Length {} exceeds u64 limit", len))
}

fn encode_canonical_field(field_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let len_bytes = checked_u64_len(field_bytes.len())?;
    let mut vec = Vec::with_capacity(8 + field_bytes.len());
    vec.extend_from_slice(&len_bytes);
    vec.extend_from_slice(field_bytes);
    Ok(vec)
}

fn encode_canonical_str(s: &str) -> Result<Vec<u8>, String> {
    encode_canonical_field(s.as_bytes())
}

fn encode_canonical_str_array(items: &[String]) -> Result<Vec<u8>, String> {
    let mut sorted = items.to_vec();
    sorted.sort();
    let mut vec = Vec::new();
    let len_bytes = checked_u64_len(sorted.len())?;
    vec.extend_from_slice(&len_bytes);
    for item in &sorted {
        vec.extend(encode_canonical_str(item)?);
    }
    Ok(vec)
}

fn encode_canonical_opt_bool(opt: Option<bool>) -> Vec<u8> {
    match opt {
        None => vec![0x00],
        Some(true) => vec![0x01, 0x01],
        Some(false) => vec![0x01, 0x00],
    }
}

fn encode_canonical_opt_usize(opt: Option<usize>) -> Result<Vec<u8>, String> {
    match opt {
        None => Ok(vec![0x00]),
        Some(val) => {
            let val_bytes = checked_u64_len(val)?;
            let mut vec = Vec::with_capacity(9);
            vec.push(0x01);
            vec.extend_from_slice(&val_bytes);
            Ok(vec)
        }
    }
}

/// Shared canonical expectation encoder.
pub fn encode_canonical_expectation(exp: &BenchmarkExpectation) -> Result<Vec<u8>, String> {
    let mut vec = Vec::new();
    vec.extend(encode_canonical_opt_bool(exp.accepted));
    vec.extend(encode_canonical_opt_bool(exp.preserve_exact));
    vec.extend(encode_canonical_str_array(&exp.expected_candidates)?);
    vec.extend(encode_canonical_str_array(&exp.forbidden_candidates)?);
    vec.extend(encode_canonical_opt_bool(exp.allow_no_candidate));
    vec.extend(encode_canonical_opt_usize(exp.required_top_k)?);
    Ok(vec)
}

/// Computes shared canonical u64 big-endian length-prefixed SHA-256 identity `case_id`.
pub fn compute_canonical_case_id(
    task: BenchmarkTask,
    category: BenchmarkCategory,
    input: &str,
    context: Option<&[String]>,
    expectation: &BenchmarkExpectation,
) -> Result<String, String> {
    let mut payload = Vec::new();
    payload.extend(encode_canonical_str(BENCHMARK_CASE_DOMAIN_TAG)?);
    payload.extend(encode_canonical_str(task.as_str())?);
    payload.extend(encode_canonical_str(category.as_str())?);
    payload.extend(encode_canonical_str(input)?);

    if let Some(ctx) = context {
        let mut vec = Vec::new();
        let len_bytes = checked_u64_len(ctx.len())?;
        vec.extend_from_slice(&len_bytes);
        for item in ctx {
            vec.extend(encode_canonical_str(item)?);
        }
        payload.extend(vec);
    } else {
        payload.extend(encode_canonical_str_array(&[])?);
    }

    payload.extend(encode_canonical_expectation(expectation)?);

    Ok(format!("{:x}", Sha256::digest(&payload)))
}

fn validate_review_date(date_str: &str) -> Result<(), String> {
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return Err(format!(
            "Invalid date format '{}': expected YYYY-MM-DD",
            date_str
        ));
    }
    let year: i32 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid year in date '{}'", date_str))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| format!("Invalid month in date '{}'", date_str))?;
    let day: u32 = parts[2]
        .parse()
        .map_err(|_| format!("Invalid day in date '{}'", date_str))?;

    if !(2020..=2100).contains(&year) {
        return Err(format!("Year out of range in date '{}'", date_str));
    }
    if !(1..=12).contains(&month) {
        return Err(format!("Invalid month in date '{}'", date_str));
    }
    let max_days = match month {
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=max_days).contains(&day) {
        return Err(format!("Invalid day in date '{}'", date_str));
    }
    Ok(())
}

/// Validates an individual `BenchmarkCaseRecord`.
pub fn validate_case_record(record: &BenchmarkCaseRecord) -> Result<(), String> {
    if record.schema_version != BENCHMARK_CASE_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported schema_version '{}': expected '{}'",
            record.schema_version, BENCHMARK_CASE_SCHEMA_VERSION
        ));
    }

    if record.input.trim().is_empty() {
        return Err("Input string must not be empty".to_string());
    }

    if !is_nfc_normalized(&record.input) {
        return Err(format!("Input '{}' must be NFC normalized", record.input));
    }

    if record.input.chars().any(|c| c.is_control() || c == '\0') {
        return Err(format!(
            "Input '{}' contains forbidden control or NUL characters",
            record.input
        ));
    }

    if let Some(ref ctx) = record.context {
        for (idx, item) in ctx.iter().enumerate() {
            if item.trim().is_empty() {
                return Err(format!(
                    "Context item [{}] must not be empty or whitespace-only",
                    idx
                ));
            }
            if !is_nfc_normalized(item) {
                return Err(format!(
                    "Context item [{}] '{}' must be NFC normalized",
                    idx, item
                ));
            }
            if item.chars().any(|c| c.is_control() || c == '\0') {
                return Err(format!(
                    "Context item [{}] '{}' contains forbidden control or NUL characters",
                    idx, item
                ));
            }
        }
    }

    if !is_compatible_task_category(record.task, record.category) {
        return Err(format!(
            "Task '{:?}' is incompatible with category '{:?}'",
            record.task, record.category
        ));
    }

    let expected_id = compute_canonical_case_id(
        record.task,
        record.category,
        &record.input,
        record.context.as_deref(),
        &record.expectation,
    )?;
    if record.case_id != expected_id {
        return Err(format!(
            "Invalid case_id '{}': expected canonical identity '{}'",
            record.case_id, expected_id
        ));
    }

    // Validate expected_candidates and forbidden_candidates
    let exp_set: std::collections::BTreeSet<_> =
        record.expectation.expected_candidates.iter().collect();
    if exp_set.len() != record.expectation.expected_candidates.len() {
        return Err(format!(
            "Duplicate expected_candidates in case '{}'",
            record.case_id
        ));
    }

    for (idx, cand) in record.expectation.expected_candidates.iter().enumerate() {
        if cand.trim().is_empty() {
            return Err(format!(
                "Expected candidate [{}] must not be empty or whitespace-only",
                idx
            ));
        }
        if !is_nfc_normalized(cand) {
            return Err(format!(
                "Expected candidate [{}] '{}' must be NFC normalized",
                idx, cand
            ));
        }
        if cand.chars().any(|c| c.is_control() || c == '\0') {
            return Err(format!(
                "Expected candidate [{}] '{}' contains forbidden control or NUL characters",
                idx, cand
            ));
        }
    }

    let forb_set: std::collections::BTreeSet<_> =
        record.expectation.forbidden_candidates.iter().collect();
    if forb_set.len() != record.expectation.forbidden_candidates.len() {
        return Err(format!(
            "Duplicate forbidden_candidates in case '{}'",
            record.case_id
        ));
    }

    for (idx, cand) in record.expectation.forbidden_candidates.iter().enumerate() {
        if cand.trim().is_empty() {
            return Err(format!(
                "Forbidden candidate [{}] must not be empty or whitespace-only",
                idx
            ));
        }
        if !is_nfc_normalized(cand) {
            return Err(format!(
                "Forbidden candidate [{}] '{}' must be NFC normalized",
                idx, cand
            ));
        }
        if cand.chars().any(|c| c.is_control() || c == '\0') {
            return Err(format!(
                "Forbidden candidate [{}] '{}' contains forbidden control or NUL characters",
                idx, cand
            ));
        }
    }

    // Disjoint expected and forbidden candidates
    for cand in &record.expectation.expected_candidates {
        if forb_set.contains(cand) {
            return Err(format!(
                "Candidate '{}' cannot be both expected and forbidden in case '{}'",
                cand, record.case_id
            ));
        }
    }

    // Expectation constraints
    if let Some(top_k) = record.expectation.required_top_k {
        if top_k == 0 {
            return Err("required_top_k must be > 0".to_string());
        }
        if record.task == BenchmarkTask::AcceptWord {
            return Err("required_top_k is only allowed for candidate-returning tasks".to_string());
        }
    }

    if record.expectation.preserve_exact.is_some() && record.task != BenchmarkTask::AcceptWord {
        return Err("preserve_exact is only allowed for AcceptWord task".to_string());
    }

    if record.expectation.allow_no_candidate == Some(true) {
        if !record.expectation.expected_candidates.is_empty() {
            return Err(
                "Contradictory expectation: allow_no_candidate = true cannot be combined with non-empty expected_candidates"
                    .to_string(),
            );
        }
        if record.expectation.accepted == Some(true) {
            return Err(
                "Contradictory expectation: allow_no_candidate = true cannot be combined with accepted = true"
                    .to_string(),
            );
        }
    }

    // Source provenance validation
    if let Some(ref s) = record.source.source_id {
        if s.trim().is_empty() {
            return Err("source.source_id cannot be blank".to_string());
        }
    }
    if let Some(ref s) = record.source.source_document_id {
        if s.trim().is_empty() {
            return Err("source.source_document_id cannot be blank".to_string());
        }
    }
    if let Some(ref s) = record.source.source_record {
        if s.trim().is_empty() {
            return Err("source.source_record cannot be blank".to_string());
        }
    }

    match record.source.kind {
        BenchmarkSourceKind::HeldOutCorpus => {
            if record
                .source
                .source_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err(
                    "Source kind 'held-out-corpus' requires non-empty source_id".to_string()
                );
            }
            if record
                .source
                .source_document_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err(
                    "Source kind 'held-out-corpus' requires non-empty source_document_id"
                        .to_string(),
                );
            }
        }
        BenchmarkSourceKind::MechanicalDraft => {
            if record
                .source
                .source_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err(
                    "Source kind 'mechanical-draft' requires non-empty source_id".to_string(),
                );
            }
        }
        BenchmarkSourceKind::AiAssistedDraft => {
            if record
                .source
                .source_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err(
                    "Source kind 'ai-assisted-draft' requires non-empty source_id".to_string(),
                );
            }
        }
        BenchmarkSourceKind::Manual => {}
    }

    match record.review_status {
        BenchmarkReviewStatus::HumanReviewed => {
            let reviewer = record.reviewer_id.as_deref().unwrap_or("");
            if reviewer.trim().is_empty() {
                return Err("Human-reviewed status requires a non-empty reviewer_id".to_string());
            }
            let date = record.review_date.as_deref().ok_or_else(|| {
                "Human-reviewed status requires review_date (YYYY-MM-DD)".to_string()
            })?;
            validate_review_date(date)?;

            match record.task {
                BenchmarkTask::CorrectWord | BenchmarkTask::CompletePrefix => {
                    if record.expectation.expected_candidates.is_empty()
                        && record.expectation.allow_no_candidate != Some(true)
                    {
                        return Err(format!(
                            "Human-reviewed task '{:?}' requires expected_candidates or allow_no_candidate = true",
                            record.task
                        ));
                    }
                }
                BenchmarkTask::AcceptWord => {
                    if record.expectation.accepted.is_none() {
                        return Err(
                            "Human-reviewed AcceptWord task requires explicit expectation.accepted boolean"
                                .to_string(),
                        );
                    }
                }
            }

            let requires_notes = matches!(
                record.category,
                BenchmarkCategory::RegionalVariant
                    | BenchmarkCategory::Morphology
                    | BenchmarkCategory::ProperNoun
                    | BenchmarkCategory::CommonVsRare
            );
            if requires_notes
                && record
                    .review_notes
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                return Err(format!(
                    "Category '{:?}' requires non-empty review_notes for linguistic review context",
                    record.category
                ));
            }
        }
        BenchmarkReviewStatus::Draft => {
            if record.reviewer_id.is_some() {
                return Err("Draft status must not contain reviewer_id".to_string());
            }
            if record.review_date.is_some() {
                return Err("Draft status must not contain review_date".to_string());
            }
        }
    }

    Ok(())
}
