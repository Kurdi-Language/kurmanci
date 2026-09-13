//! First Kuwiki-Backed Human Vocabulary Review Batch Generator (`kuwiki-vocabulary-review-batch-v1`).
//!
//! Generates a deterministic, reviewable first batch of top 1,000 highest-attestation
//! `kuwiki` OOV candidates (`oov-review-queue.jsonl`) for human Kurmancî review.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::corpus::vocabulary_evidence::{
    compute_experimental_lexicon_fingerprint, OovCandidateRecord, RepresentativeContext,
    VocabularyEvidenceSummaryReport,
};
use crate::CorpusRegistry;

pub const KUWIKI_REVIEW_BATCH_SCHEMA_VERSION: &str = "kuwiki-vocabulary-review-batch-v1";
pub const KUWIKI_REVIEW_BATCH_MANIFEST_SCHEMA_VERSION: &str = "kuwiki-review-batch-manifest-v1";
pub const DEFAULT_KUWIKI_BATCH_ID: &str = "kuwiki-batch-001";
pub const DEFAULT_KUWIKI_BATCH_SIZE: usize = 1000;

/// Context reference used internally during batch generation for statistics.
/// Stripped from committed `candidates.jsonl` artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextReference {
    pub corpus_id: String,
    pub document_id: String,
}

/// Candidate record inside the committed `candidates.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KuwikiReviewBatchCandidate {
    pub schema_version: String,
    pub batch_id: String,
    pub batch_rank: usize,
    pub original_queue_rank: usize,
    pub token: String,
    pub normalized_token: String,
    pub token_count: u64,
    pub document_count: u64,
    pub normalized_frequency: f64,
    pub zipf_milli: u32,
    pub corpus_id: String,
    pub in_seed: bool,
    pub in_reviewed: bool,
    pub in_experimental_full: bool,
    pub technical_filter_status: String,
    pub technical_filter_reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_references: Vec<ContextReference>,
    pub decision_status: String,
}

/// Parses numeric sequence `NNN` from batch ID string matching `kuwiki-batch-NNN`.
/// Returns Err if `batch_id` does not strictly follow this format.
pub fn parse_kuwiki_batch_sequence(batch_id: &str) -> Result<u32, String> {
    let prefix = "kuwiki-batch-";
    if !batch_id.starts_with(prefix) {
        return Err(format!(
            "Invalid batch_id format '{}': must start with '{}'",
            batch_id, prefix
        ));
    }
    let num_str = &batch_id[prefix.len()..];
    if num_str.len() != 3 || !num_str.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Invalid batch_id format '{}': suffix '{}' must be exactly three ASCII digits",
            batch_id, num_str
        ));
    }
    let seq = num_str.parse::<u32>().map_err(|e| {
        format!(
            "Failed to parse numeric batch sequence from '{}': {}",
            batch_id, e
        )
    })?;
    if seq < 1 {
        return Err(format!(
            "Invalid batch_id sequence in '{}': sequence number must be >= 1",
            batch_id
        ));
    }
    Ok(seq)
}

/// Exclusion source entry for an earlier committed review batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExcludedPriorBatch {
    pub batch_id: String,
    pub candidates_sha256: String,
    pub candidate_count: usize,
}

/// Provenance & integrity manifest in `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KuwikiReviewBatchManifest {
    pub schema_version: String,
    pub batch_id: String,
    pub source_corpus_id: String,
    pub source_version: String,
    pub input_oov_review_queue_sha256: String,
    pub vocabulary_evidence_artifacts_manifest_sha256: String,
    pub corpus_registry_sha256: String,
    pub canonical_manifest_sha256: String,
    pub partition_manifest_sha256: String,
    pub train_partition_sha256: String,
    pub frequency_artifact_sha256: String,
    pub frequency_build_manifest_sha256: String,
    pub experimental_lexicon_fingerprint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_prior_batches: Vec<ExcludedPriorBatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previously_assigned_normalized_token_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded_due_to_prior_assignment_count: Option<usize>,
    pub selection_policy: String,
    pub batch_size: usize,
    pub candidates_file: String,
    pub candidates_sha256: String,
}

/// Special diagnostic target presence report entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecialTargetBatchPresence {
    pub target: String,
    pub normalized_target: String,
    pub present_in_batch: bool,
    pub batch_rank: Option<usize>,
    pub document_count: u64,
    pub token_count: u64,
    pub representative_contexts: Vec<RepresentativeContext>,
}

/// Summary report emitted when the batch is generated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KuwikiReviewBatchSummary {
    pub schema_version: String,
    pub batch_id: String,
    pub corpus_id: String,
    pub batch_size: usize,
    pub input_queue_sha256: String,
    pub experimental_fingerprint: String,
    pub doc_count_min: u64,
    pub doc_count_median: u64,
    pub doc_count_max: u64,
    pub token_count_min: u64,
    pub token_count_median: u64,
    pub token_count_max: u64,
    pub gte_100_docs_count: usize,
    pub gte_500_docs_count: usize,
    pub gte_1000_docs_count: usize,
    pub contexts_1_count: usize,
    pub contexts_2_count: usize,
    pub contexts_3_count: usize,
    pub contexts_lacking_count: usize,
    pub non_ascii_candidate_count: usize,
    pub special_targets: Vec<SpecialTargetBatchPresence>,
    pub output_dir: String,
}

/// Calculates SHA-256 of file contents.
fn calculate_file_sha256<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let mut file = File::open(&path)
        .map_err(|e| format!("Failed to open for hashing {:?}: {}", path.as_ref(), e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .map_err(|e| format!("Failed to hash file {:?}: {}", path.as_ref(), e))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Helper to verify a required evidence input file exists and matches its expected SHA-256 hash.
fn verify_file_sha256<P: AsRef<Path>>(
    path: P,
    expected_sha: &str,
    label: &str,
) -> Result<(), String> {
    let p = path.as_ref();
    if !p.exists() {
        return Err(format!(
            "Required evidence input missing at {:?} for {}.",
            p, label
        ));
    }
    let actual_sha = calculate_file_sha256(p)?;
    if actual_sha != expected_sha {
        return Err(format!(
            "Stale evidence provenance: {} SHA-256 '{}' does not match summary recorded '{}'",
            label, actual_sha, expected_sha
        ));
    }
    Ok(())
}

/// Helper to verify an artifact hash recorded inside `artifacts.sha256`.
fn verify_artifact_in_manifest<P: AsRef<Path>>(
    artifacts_manifest_path: P,
    target_filename: &str,
    actual_file_sha256: &str,
    label: &str,
) -> Result<(), String> {
    let p = artifacts_manifest_path.as_ref();
    let content = fs::read_to_string(p)
        .map_err(|e| format!("Failed to read artifacts manifest {:?}: {}", p, e))?;
    let mut recorded_sha = None;
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 {
            let recorded_path = parts[1];
            if recorded_path == target_filename
                || recorded_path.ends_with(&format!("/{}", target_filename))
            {
                recorded_sha = Some(parts[0].to_string());
                break;
            }
        }
    }
    let recorded_sha = recorded_sha.ok_or_else(|| {
        format!(
            "Entry for '{}' missing in evidence artifacts manifest {:?}",
            target_filename, p
        )
    })?;
    if actual_file_sha256 != recorded_sha {
        return Err(format!(
            "Stale {} artifact: {} SHA-256 '{}' does not match artifacts.sha256 recorded '{}'",
            label, target_filename, actual_file_sha256, recorded_sha
        ));
    }
    Ok(())
}

/// Helper to compute median of a u64 slice.
fn compute_median_u64(vals: &[u64]) -> u64 {
    if vals.is_empty() {
        return 0;
    }
    let mut sorted = vals.to_vec();
    sorted.sort_unstable();
    let len = sorted.len();
    if len % 2 == 1 {
        sorted[len / 2]
    } else {
        (sorted[len / 2 - 1] + sorted[len / 2]) / 2
    }
}

/// Looks up source version for `corpus_id`. Fail-closed on missing registry or missing entry.
fn get_corpus_source_version<P: AsRef<Path>>(
    root_dir: P,
    corpus_id: &str,
) -> Result<String, String> {
    if corpus_id != "kuwiki" {
        return Err(format!(
            "Unsupported corpus_id '{}': kuwiki review batch generation requires corpus_id == 'kuwiki'.",
            corpus_id
        ));
    }
    let corpora_toml_path = root_dir.as_ref().join("data/source-registry/corpora.toml");
    if !corpora_toml_path.exists() {
        return Err(format!(
            "Corpus registry file missing at {:?}.",
            corpora_toml_path
        ));
    }
    let content = fs::read_to_string(&corpora_toml_path).map_err(|e| {
        format!(
            "Failed to read corpora.toml at {:?}: {}",
            corpora_toml_path, e
        )
    })?;
    let registry: CorpusRegistry = toml::from_str(&content).map_err(|e| {
        format!(
            "Failed to parse corpora.toml at {:?}: {}",
            corpora_toml_path, e
        )
    })?;
    for entry in registry.corpora {
        if entry.corpus_id == corpus_id {
            return Ok(entry.version);
        }
    }
    Err(format!(
        "Corpus ID '{}' missing from registry at {:?}",
        corpus_id, corpora_toml_path
    ))
}

/// Verifies full PR #47 evidence provenance, stale file SHA-256 hashes, and queue manifest integrity.
pub fn verify_vocabulary_evidence_provenance<P: AsRef<Path>>(
    root_dir: P,
    corpus_id: &str,
) -> Result<(VocabularyEvidenceSummaryReport, String, String), String> {
    let root = root_dir.as_ref();
    let evidence_dir = root
        .join("data/reports/vocabulary-evidence")
        .join(corpus_id);
    let summary_path = evidence_dir.join("summary.json");
    let queue_path = evidence_dir.join("oov-review-queue.jsonl");
    let artifacts_path = evidence_dir.join("artifacts.sha256");

    if !summary_path.exists() {
        return Err(format!(
            "Vocabulary evidence summary missing at {:?}. Run build-vocabulary-evidence --corpus-id {} first.",
            summary_path, corpus_id
        ));
    }
    if !queue_path.exists() {
        return Err(format!(
            "Input OOV review queue missing at {:?}. Run build-vocabulary-evidence --corpus-id {} first.",
            queue_path, corpus_id
        ));
    }
    if !artifacts_path.exists() {
        return Err(format!(
            "Vocabulary evidence artifacts.sha256 missing at {:?}. Run build-vocabulary-evidence --corpus-id {} first.",
            artifacts_path, corpus_id
        ));
    }

    let summary_sha256 = calculate_file_sha256(&summary_path)?;
    let queue_file_sha256 = calculate_file_sha256(&queue_path)?;
    let artifacts_sha256 = calculate_file_sha256(&artifacts_path)?;

    // 1. Verify summary.json against PR #47 artifacts.sha256
    verify_artifact_in_manifest(&artifacts_path, "summary.json", &summary_sha256, "summary")?;

    // 2. Verify oov-review-queue.jsonl against PR #47 artifacts.sha256
    verify_artifact_in_manifest(
        &artifacts_path,
        "oov-review-queue.jsonl",
        &queue_file_sha256,
        "queue",
    )?;

    // Parse summary.json
    let summary_bytes = fs::read(&summary_path)
        .map_err(|e| format!("Failed to read evidence summary {:?}: {}", summary_path, e))?;
    let summary: VocabularyEvidenceSummaryReport = serde_json::from_slice(&summary_bytes)
        .map_err(|e| format!("Failed to parse evidence summary {:?}: {}", summary_path, e))?;

    if summary.schema_version != "vocabulary-evidence-v1" {
        return Err(format!(
            "Evidence summary schema_version mismatch: recorded '{}', expected 'vocabulary-evidence-v1'",
            summary.schema_version
        ));
    }
    if summary.corpus_id != corpus_id {
        return Err(format!(
            "Evidence summary corpus_id mismatch: recorded '{}', expected '{}'",
            summary.corpus_id, corpus_id
        ));
    }

    // 3. Strict verification of ALL 6 underlying provenance files
    verify_file_sha256(
        root.join("data/source-registry/corpora.toml"),
        &summary.provenance.corpus_registry_sha256,
        "corpora.toml",
    )?;

    verify_file_sha256(
        root.join("data/imported-canonical/manifest.json"),
        &summary.provenance.canonical_manifest_sha256,
        "canonical manifest",
    )?;

    verify_file_sha256(
        root.join("data/build/corpus-partitions/manifest.json"),
        &summary.provenance.partition_manifest_sha256,
        "partition manifest",
    )?;

    verify_file_sha256(
        root.join("data/build/corpus-partitions/train.jsonl"),
        &summary.provenance.train_partition_sha256,
        "train partition",
    )?;

    verify_file_sha256(
        root.join("data/build/frequencies.jsonl"),
        &summary.provenance.frequency_artifact_sha256,
        "frequencies.jsonl",
    )?;

    verify_file_sha256(
        root.join("data/build/frequency_manifest.json"),
        &summary.provenance.frequency_build_manifest_sha256,
        "frequency_manifest.json",
    )?;

    // 4. Recompute & verify current experimental lexicon fingerprint
    let current_exp_fingerprint = compute_experimental_lexicon_fingerprint(root)?;
    if current_exp_fingerprint != summary.provenance.experimental_lexicon_fingerprint {
        return Err(format!(
            "Stale experimental lexicon fingerprint: current '{}' does not match evidence summary recorded '{}'",
            current_exp_fingerprint, summary.provenance.experimental_lexicon_fingerprint
        ));
    }

    Ok((summary, queue_file_sha256, artifacts_sha256))
}

/// Generates a deterministic human vocabulary review batch from kuwiki OOV review queue.
pub fn generate_kuwiki_review_batch<P: AsRef<Path>>(
    root_dir: P,
    corpus_id: &str,
    batch_id: &str,
    batch_size: usize,
) -> Result<KuwikiReviewBatchSummary, String> {
    let root = root_dir.as_ref();

    if corpus_id != "kuwiki" {
        return Err(format!(
            "Unsupported corpus_id '{}': generate_kuwiki_review_batch requires corpus_id == 'kuwiki'.",
            corpus_id
        ));
    }

    let source_version = get_corpus_source_version(root, corpus_id)?;

    // 1. Full Fail-Closed Provenance Verification
    let (evidence_summary, input_queue_sha256, evidence_artifacts_sha256) =
        verify_vocabulary_evidence_provenance(root, corpus_id)?;

    let exp_fingerprint = evidence_summary.provenance.experimental_lexicon_fingerprint;

    let queue_path = root
        .join("data/reports/vocabulary-evidence")
        .join(corpus_id)
        .join("oov-review-queue.jsonl");

    // Read & strictly assert queue rank, filter status, deduplication, and monotonic sort invariants
    let queue_file = File::open(&queue_path)
        .map_err(|e| format!("Failed to open queue file {:?}: {}", queue_path, e))?;
    let reader = BufReader::new(queue_file);

    let mut eligible_records: Vec<OovCandidateRecord> = Vec::new();
    let mut seen_normalized = BTreeSet::new();

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line_num = line_idx + 1;
        let line = line_res
            .map_err(|e| format!("Read error at {:?}:line {}: {}", queue_path, line_num, e))?;

        if line.trim().is_empty() {
            continue;
        }

        let rec: OovCandidateRecord = serde_json::from_str(&line).map_err(|e| {
            format!(
                "JSON parse error at {:?}:line {}: {}",
                queue_path, line_num, e
            )
        })?;

        // 1. Strict rank check
        if rec.rank != line_num {
            return Err(format!(
                "Queue rank discontinuity at {:?}:line {}: record rank is {}, expected sequential line {}",
                queue_path, line_num, rec.rank, line_num
            ));
        }

        // 2. Strict corpus_id check
        if rec.corpus_id != corpus_id {
            return Err(format!(
                "Queue corpus_id mismatch at {:?}:line {}: record has '{}', expected '{}'",
                queue_path, line_num, rec.corpus_id, corpus_id
            ));
        }

        // 3. Strict filter status & reason check
        if rec.technical_filter_status != "eligible_for_review"
            || rec.technical_filter_reason != "none"
        {
            return Err(format!(
                "Queue invariant failure at {:?}:line {}: record '{}' has non-eligible status '{}' or reason '{}'",
                queue_path, line_num, rec.token, rec.technical_filter_status, rec.technical_filter_reason
            ));
        }

        let canonical_token = crate::normalize::normalize_text(&rec.token);
        if rec.normalized_token != canonical_token {
            return Err(format!(
                "Queue invariant failure at {:?}:line {}: record token '{}' has normalized_token '{}' inconsistent with normalize_text '{}'",
                queue_path, line_num, rec.token, rec.normalized_token, canonical_token
            ));
        }

        // 4. Strict deduplication check
        if !seen_normalized.insert(canonical_token) {
            return Err(format!(
                "Queue invariant failure at {:?}:line {}: duplicate canonical token for '{}'",
                queue_path, line_num, rec.token
            ));
        }

        // 5. Strict monotonic sort check relative to previous record
        if let Some(prev) = eligible_records.last() {
            let is_valid_order = rec.document_count < prev.document_count
                || (rec.document_count == prev.document_count
                    && rec.token_count < prev.token_count)
                || (rec.document_count == prev.document_count
                    && rec.token_count == prev.token_count
                    && rec.normalized_token > prev.normalized_token);

            if !is_valid_order {
                return Err(format!(
                    "Queue sorting invariant broken at {:?}:line {}: record '{}' (docs={}, tokens={}) comes after '{}' (docs={}, tokens={})",
                    queue_path, line_num, rec.normalized_token, rec.document_count, rec.token_count, prev.normalized_token, prev.document_count, prev.token_count
                ));
            }
        }

        eligible_records.push(rec);
    }

    // Target batch sequence check (Requirement 2)
    let target_seq = parse_kuwiki_batch_sequence(batch_id)?;

    // 6. Generic Prior-Batch Exclusion Engine
    let review_batches_dir = root.join("data/review-batches");
    let mut token_to_first_assignment: BTreeMap<String, (String, usize)> = BTreeMap::new();
    let mut excluded_prior_batches: Vec<ExcludedPriorBatch> = Vec::new();

    if review_batches_dir.exists() {
        let entries = fs::read_dir(&review_batches_dir).map_err(|e| {
            format!(
                "Failed to read review-batches dir {:?}: {}",
                review_batches_dir, e
            )
        })?;

        let mut prior_batch_dirs: Vec<(u32, String, std::path::PathBuf)> = Vec::new();
        for entry_res in entries {
            let entry =
                entry_res.map_err(|e| format!("Failed reading entry in review-batches: {}", e))?;
            let path = entry.path();
            if path.is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if dir_name.ends_with(".tmp") {
                    continue;
                }

                // Requirement 2: Strict batch ID format & sequence parsing
                if dir_name.starts_with("kuwiki-") || dir_name.starts_with("kuwiki-batch-") {
                    let seq = parse_kuwiki_batch_sequence(&dir_name).map_err(|e| {
                        format!(
                            "Malformed or ambiguous prior batch directory name '{:?}': {}",
                            path, e
                        )
                    })?;

                    if seq < target_seq {
                        prior_batch_dirs.push((seq, dir_name, path));
                    }
                }
            }
        }

        // Sort strictly by batch sequence NNN ascending
        prior_batch_dirs.sort_by_key(|(seq, _, _)| *seq);

        for (seq, dir_name, path) in prior_batch_dirs {
            let candidates_file = path.join("candidates.jsonl");
            let manifest_file = path.join("manifest.json");
            let artifacts_file = path.join("artifacts.sha256");

            // Requirement 4: Verify prior batch complete existence
            if !manifest_file.exists() {
                return Err(format!(
                    "Missing manifest.json in prior review batch directory {:?}",
                    path
                ));
            }
            if !candidates_file.exists() {
                return Err(format!(
                    "Missing candidates.jsonl in prior review batch directory {:?}",
                    path
                ));
            }
            if !artifacts_file.exists() {
                return Err(format!(
                    "Missing artifacts.sha256 in prior review batch directory {:?}",
                    path
                ));
            }

            let actual_cand_sha = calculate_file_sha256(&candidates_file)?;
            let actual_man_sha = calculate_file_sha256(&manifest_file)?;

            let manifest_bytes = fs::read(&manifest_file)
                .map_err(|e| format!("Failed to read manifest {:?}: {}", manifest_file, e))?;
            let prev_manifest: KuwikiReviewBatchManifest = serde_json::from_slice(&manifest_bytes)
                .map_err(|e| format!("Failed to parse manifest {:?}: {}", manifest_file, e))?;

            // Requirement 2 & 4: Check manifest fields
            if prev_manifest.batch_id != dir_name {
                return Err(format!(
                    "Prior batch manifest batch_id mismatch for '{:?}': manifest has '{}', expected '{}'",
                    path, prev_manifest.batch_id, dir_name
                ));
            }
            let manifest_seq = parse_kuwiki_batch_sequence(&prev_manifest.batch_id)?;
            if manifest_seq != seq {
                return Err(format!(
                    "Prior batch sequence mismatch in manifest for '{}': manifest sequence {}, dir sequence {}",
                    dir_name, manifest_seq, seq
                ));
            }
            if prev_manifest.source_corpus_id != "kuwiki" {
                return Err(format!(
                    "Prior batch source_corpus_id mismatch for '{}': manifest has '{}', expected 'kuwiki'",
                    dir_name, prev_manifest.source_corpus_id
                ));
            }

            if prev_manifest.candidates_sha256 != actual_cand_sha {
                return Err(format!(
                    "Stale prior batch candidate SHA-256 for '{}': manifest recorded '{}', actual '{}'",
                    dir_name, prev_manifest.candidates_sha256, actual_cand_sha
                ));
            }

            // Verify artifacts.sha256
            verify_artifact_in_manifest(
                &artifacts_file,
                "candidates.jsonl",
                &actual_cand_sha,
                "prior candidate",
            )?;
            verify_artifact_in_manifest(
                &artifacts_file,
                "manifest.json",
                &actual_man_sha,
                "prior manifest",
            )?;

            let cand_file = File::open(&candidates_file).map_err(|e| {
                format!(
                    "Failed to open candidates file {:?}: {}",
                    candidates_file, e
                )
            })?;
            let cand_reader = BufReader::new(cand_file);

            let mut batch_candidate_count = 0;
            let mut seen_in_this_batch = BTreeSet::new();

            for (line_idx, line_res) in cand_reader.lines().enumerate() {
                let line_num = line_idx + 1;
                let line = line_res.map_err(|e| {
                    format!(
                        "Error reading line {} in {:?}: {}",
                        line_num, candidates_file, e
                    )
                })?;
                if line.trim().is_empty() {
                    continue;
                }
                let cand: KuwikiReviewBatchCandidate =
                    serde_json::from_str(&line).map_err(|e| {
                        format!(
                            "JSON parse error at {:?}:line {}: {}",
                            candidates_file, line_num, e
                        )
                    })?;

                if cand.batch_rank != line_num {
                    return Err(format!(
                        "Prior batch candidate rank discontinuity in '{}': rank is {}, expected {}",
                        dir_name, cand.batch_rank, line_num
                    ));
                }
                if cand.batch_id != dir_name {
                    return Err(format!(
                        "Prior batch candidate batch_id mismatch in '{}' at rank {}: candidate has '{}'",
                        dir_name, line_num, cand.batch_id
                    ));
                }
                if cand.corpus_id != "kuwiki" {
                    return Err(format!(
                        "Prior batch candidate corpus_id mismatch in '{}' at rank {}: candidate has '{}'",
                        dir_name, line_num, cand.corpus_id
                    ));
                }
                let canonical = crate::normalize::normalize_text(&cand.token);
                if cand.normalized_token != canonical {
                    return Err(format!(
                        "Prior batch '{}' candidate rank {} normalized_token '{}' is inconsistent with normalize_text(&cand.token) '{}'",
                        dir_name, line_num, cand.normalized_token, canonical
                    ));
                }

                // Check uniqueness within this prior batch
                if !seen_in_this_batch.insert(canonical.clone()) {
                    return Err(format!(
                        "Prior batch '{}' contains duplicate canonical token '{}' at rank {}",
                        dir_name, canonical, line_num
                    ));
                }

                // Requirement 3: Check duplicate across ALL historical committed Kuwiki batches -> FAIL CLOSED
                if let Some((first_batch, first_rank)) = token_to_first_assignment.get(&canonical) {
                    return Err(format!(
                        "Historical duplicate detected across committed Kuwiki batches: canonical token '{}' appears in both prior batch '{}' (rank {}) and prior batch '{}' (rank {})",
                        canonical, first_batch, first_rank, dir_name, cand.batch_rank
                    ));
                }

                token_to_first_assignment.insert(canonical, (dir_name.clone(), cand.batch_rank));
                batch_candidate_count += 1;
            }

            excluded_prior_batches.push(ExcludedPriorBatch {
                batch_id: dir_name,
                candidates_sha256: actual_cand_sha,
                candidate_count: batch_candidate_count,
            });
        }
    }

    let previously_assigned_normalized_token_count = token_to_first_assignment.len();

    let mut excluded_due_to_prior_assignment_count = 0;
    let mut diagnostic_skipped_sample: Vec<(String, String, usize)> = Vec::new();

    const DIAGNOSTIC_SAMPLE_CAP: usize = 10;

    let filtered_queue_records: Vec<&OovCandidateRecord> = eligible_records
        .iter()
        .filter(|rec| {
            let canonical = crate::normalize::normalize_text(&rec.token);
            if let Some((prior_batch, prior_rank)) = token_to_first_assignment.get(&canonical) {
                excluded_due_to_prior_assignment_count += 1;
                if diagnostic_skipped_sample.len() < DIAGNOSTIC_SAMPLE_CAP {
                    diagnostic_skipped_sample.push((canonical, prior_batch.clone(), *prior_rank));
                }
                false
            } else {
                true
            }
        })
        .collect();

    // Print diagnostic report summary (Requirement 14)
    println!("=== Prior Batch Exclusion Diagnostic Summary ===");
    println!(
        "Total prior Kuwiki batches discovered: {}",
        excluded_prior_batches.len()
    );
    println!(
        "Total normalized tokens assigned in prior batches: {}",
        previously_assigned_normalized_token_count
    );
    println!(
        "Fresh OOV queue records skipped due to prior assignment: {}",
        excluded_due_to_prior_assignment_count
    );
    println!(
        "First {} skipped prior assignments (diagnostic sample):",
        DIAGNOSTIC_SAMPLE_CAP
    );
    for (idx, (norm_tok, p_batch, p_rank)) in diagnostic_skipped_sample.iter().enumerate() {
        println!(
            "  {:2}. '{}' (previously assigned in {} rank {})",
            idx + 1,
            norm_tok,
            p_batch,
            p_rank
        );
    }

    // 7. Strict Batch Size Contract
    if filtered_queue_records.len() < batch_size {
        return Err(format!(
            "Requested batch size {} exceeds remaining eligible queue records {}",
            batch_size,
            filtered_queue_records.len()
        ));
    }

    let mut candidates: Vec<KuwikiReviewBatchCandidate> = Vec::with_capacity(batch_size);
    let mut current_batch_normalized: BTreeSet<String> = BTreeSet::new();

    for (b_idx, queue_rec) in filtered_queue_records.iter().take(batch_size).enumerate() {
        let batch_rank = b_idx + 1;

        // Requirement 6: Enforce current-batch normalized token uniqueness
        if !current_batch_normalized.insert(queue_rec.normalized_token.clone()) {
            return Err(format!(
                "Batch selection invariant broken: candidate '{}' selected more than once in batch selection",
                queue_rec.normalized_token
            ));
        }

        let context_refs = queue_rec
            .representative_contexts
            .iter()
            .map(|ctx| ContextReference {
                corpus_id: ctx.corpus_id.clone(),
                document_id: ctx.document_id.clone(),
            })
            .collect();

        let candidate = KuwikiReviewBatchCandidate {
            schema_version: KUWIKI_REVIEW_BATCH_SCHEMA_VERSION.to_string(),
            batch_id: batch_id.to_string(),
            batch_rank,
            original_queue_rank: queue_rec.rank,
            token: queue_rec.token.clone(),
            normalized_token: queue_rec.normalized_token.clone(),
            token_count: queue_rec.token_count,
            document_count: queue_rec.document_count,
            normalized_frequency: queue_rec.normalized_frequency,
            zipf_milli: queue_rec.zipf_milli,
            corpus_id: queue_rec.corpus_id.clone(),
            in_seed: queue_rec.in_seed,
            in_reviewed: queue_rec.in_reviewed,
            in_experimental_full: queue_rec.in_experimental_full,
            technical_filter_status: queue_rec.technical_filter_status.clone(),
            technical_filter_reason: queue_rec.technical_filter_reason.clone(),
            context_references: context_refs,
            decision_status: "pending".to_string(),
        };

        if let Some(prev) = candidates.last() {
            if candidate.original_queue_rank <= prev.original_queue_rank {
                return Err(format!(
                    "original_queue_rank must be strictly increasing: candidate '{}' has rank {}, previous has {}",
                    candidate.normalized_token, candidate.original_queue_rank, prev.original_queue_rank
                ));
            }
        }

        candidates.push(candidate);
    }

    if candidates.len() != batch_size || current_batch_normalized.len() != batch_size {
        return Err(format!(
            "Batch candidate count invariant failure: expected {} candidates with {} unique normalized tokens, got {} and {}",
            batch_size, batch_size, candidates.len(), current_batch_normalized.len()
        ));
    }

    // Descriptive statistics
    let doc_counts: Vec<u64> = candidates.iter().map(|c| c.document_count).collect();
    let token_counts: Vec<u64> = candidates.iter().map(|c| c.token_count).collect();

    let doc_count_min = *doc_counts.iter().min().unwrap_or(&0);
    let doc_count_max = *doc_counts.iter().max().unwrap_or(&0);
    let doc_count_median = compute_median_u64(&doc_counts);

    let token_count_min = *token_counts.iter().min().unwrap_or(&0);
    let token_count_max = *token_counts.iter().max().unwrap_or(&0);
    let token_count_median = compute_median_u64(&token_counts);

    let gte_100_docs_count = candidates
        .iter()
        .filter(|c| c.document_count >= 100)
        .count();
    let gte_500_docs_count = candidates
        .iter()
        .filter(|c| c.document_count >= 500)
        .count();
    let gte_1000_docs_count = candidates
        .iter()
        .filter(|c| c.document_count >= 1000)
        .count();

    let contexts_1_count = candidates
        .iter()
        .filter(|c| c.context_references.len() == 1)
        .count();
    let contexts_2_count = candidates
        .iter()
        .filter(|c| c.context_references.len() == 2)
        .count();
    let contexts_3_count = candidates
        .iter()
        .filter(|c| c.context_references.len() >= 3)
        .count();
    let contexts_lacking_count = candidates
        .iter()
        .filter(|c| c.context_references.is_empty())
        .count();

    let non_ascii_candidate_count = candidates
        .iter()
        .filter(|c| !c.normalized_token.is_ascii())
        .count();

    // Special Diagnostic Targets check
    let special_targets_list = [
        "destxweş",
        "taştê",
        "porteqal",
        "kategorî",
        "girêdanên",
        "binêre",
        "landkreis",
        "franche",
        "bourgogne",
    ];

    let mut special_targets_presence = Vec::new();
    for target in &special_targets_list {
        let norm_target = crate::normalize::normalize_text(target);
        let found = candidates
            .iter()
            .find(|c| c.normalized_token == norm_target);

        if let Some(c) = found {
            // Retrieve representative contexts from eligible_records for local guide report
            let orig_rec = eligible_records
                .iter()
                .find(|r| r.normalized_token == norm_target);
            let ctxs = orig_rec
                .map(|r| r.representative_contexts.clone())
                .unwrap_or_default();

            special_targets_presence.push(SpecialTargetBatchPresence {
                target: target.to_string(),
                normalized_target: norm_target,
                present_in_batch: true,
                batch_rank: Some(c.batch_rank),
                document_count: c.document_count,
                token_count: c.token_count,
                representative_contexts: ctxs,
            });
        } else {
            let in_full_queue = eligible_records
                .iter()
                .find(|r| r.normalized_token == norm_target);
            let (d_cnt, t_cnt, ctxs) = if let Some(r) = in_full_queue {
                (
                    r.document_count,
                    r.token_count,
                    r.representative_contexts.clone(),
                )
            } else {
                (0, 0, Vec::new())
            };

            special_targets_presence.push(SpecialTargetBatchPresence {
                target: target.to_string(),
                normalized_target: norm_target,
                present_in_batch: false,
                batch_rank: None,
                document_count: d_cnt,
                token_count: t_cnt,
                representative_contexts: ctxs,
            });
        }
    }

    // Atomic Creation of COMMITTED Output Directory (`data/review-batches/kuwiki-batch-001/`)
    let batch_dir = root.join("data/review-batches").join(batch_id);
    let stage_dir = root
        .join("data/review-batches")
        .join(format!("{}.tmp", batch_id));

    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .map_err(|e| format!("Failed to clean existing stage dir {:?}: {}", stage_dir, e))?;
    }
    fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("Failed to create stage dir {:?}: {}", stage_dir, e))?;

    // 1. candidates.jsonl (COMMITTED — context_references stripped from committed output)
    let candidates_path = stage_dir.join("candidates.jsonl");
    let mut cand_file = File::create(&candidates_path).map_err(|e| {
        format!(
            "Failed to create candidates file {:?}: {}",
            candidates_path, e
        )
    })?;

    for cand in &candidates {
        let json = serde_json::to_string(cand)
            .map_err(|e| format!("Serialization error for candidate: {}", e))?;
        writeln!(cand_file, "{}", json)
            .map_err(|e| format!("Failed to write candidate to {:?}: {}", candidates_path, e))?;
    }
    drop(cand_file);

    let candidates_sha256 = calculate_file_sha256(&candidates_path)?;

    // 2. manifest.json (COMMITTED — contains complete PR #47 provenance)
    let manifest = KuwikiReviewBatchManifest {
        schema_version: KUWIKI_REVIEW_BATCH_MANIFEST_SCHEMA_VERSION.to_string(),
        batch_id: batch_id.to_string(),
        source_corpus_id: corpus_id.to_string(),
        source_version: source_version.clone(),
        input_oov_review_queue_sha256: input_queue_sha256.clone(),
        vocabulary_evidence_artifacts_manifest_sha256: evidence_artifacts_sha256,
        corpus_registry_sha256: evidence_summary.provenance.corpus_registry_sha256,
        canonical_manifest_sha256: evidence_summary.provenance.canonical_manifest_sha256,
        partition_manifest_sha256: evidence_summary.provenance.partition_manifest_sha256,
        train_partition_sha256: evidence_summary.provenance.train_partition_sha256,
        frequency_artifact_sha256: evidence_summary.provenance.frequency_artifact_sha256,
        frequency_build_manifest_sha256: evidence_summary
            .provenance
            .frequency_build_manifest_sha256,
        experimental_lexicon_fingerprint: exp_fingerprint.clone(),
        excluded_prior_batches,
        previously_assigned_normalized_token_count: Some(
            previously_assigned_normalized_token_count,
        ),
        excluded_due_to_prior_assignment_count: Some(excluded_due_to_prior_assignment_count),
        selection_policy: format!("top-{}-eligible", batch_size),
        batch_size,
        candidates_file: "candidates.jsonl".to_string(),
        candidates_sha256: candidates_sha256.clone(),
    };

    let manifest_path = stage_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Serialization error for manifest: {}", e))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| format!("Failed to write manifest {:?}: {}", manifest_path, e))?;

    let manifest_sha256 = calculate_file_sha256(&manifest_path)?;

    // 3. artifacts.sha256 (COMMITTED)
    let artifacts_path = stage_dir.join("artifacts.sha256");
    let mut artifacts_file = File::create(&artifacts_path).map_err(|e| {
        format!(
            "Failed to create artifacts file {:?}: {}",
            artifacts_path, e
        )
    })?;
    writeln!(artifacts_file, "{}  candidates.jsonl", candidates_sha256).unwrap();
    writeln!(artifacts_file, "{}  manifest.json", manifest_sha256).unwrap();
    drop(artifacts_file);

    // Atomic Move of Stage Directory to COMMITTED path
    if batch_dir.exists() {
        fs::remove_dir_all(&batch_dir)
            .map_err(|e| format!("Failed to clean existing batch dir {:?}: {}", batch_dir, e))?;
    }
    fs::rename(&stage_dir, &batch_dir).map_err(|e| {
        format!(
            "Failed atomic move {:?} -> {:?}: {}",
            stage_dir, batch_dir, e
        )
    })?;

    let summary = KuwikiReviewBatchSummary {
        schema_version: "kuwiki-review-batch-summary-v1".to_string(),
        batch_id: batch_id.to_string(),
        corpus_id: corpus_id.to_string(),
        batch_size,
        input_queue_sha256: manifest.input_oov_review_queue_sha256,
        experimental_fingerprint: exp_fingerprint,
        doc_count_min,
        doc_count_median,
        doc_count_max,
        token_count_min,
        token_count_median,
        token_count_max,
        gte_100_docs_count,
        gte_500_docs_count,
        gte_1000_docs_count,
        contexts_1_count,
        contexts_2_count,
        contexts_3_count,
        contexts_lacking_count,
        non_ascii_candidate_count,
        special_targets: special_targets_presence,
        output_dir: batch_dir.to_string_lossy().to_string(),
    };

    Ok(summary)
}
