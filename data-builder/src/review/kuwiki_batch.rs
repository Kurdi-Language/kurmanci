//! First Kuwiki-Backed Human Vocabulary Review Batch Generator (`kuwiki-vocabulary-review-batch-v1`).
//!
//! Generates a deterministic, reviewable first batch of top 1,000 highest-attestation
//! `kuwiki` OOV candidates (`oov-review-queue.jsonl`) for human Kurmancî review.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
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

/// Context reference (without copyright text) for committed `candidates.jsonl`.
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
    pub context_references: Vec<ContextReference>,
    pub decision_status: String,
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
    pub local_guide_path: String,
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

        // 4. Strict deduplication check
        if !seen_normalized.insert(rec.normalized_token.clone()) {
            return Err(format!(
                "Queue invariant failure at {:?}:line {}: duplicate normalized_token '{}'",
                queue_path, line_num, rec.normalized_token
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

    // 6. Strict Batch Size Contract
    if eligible_records.len() < batch_size {
        return Err(format!(
            "Requested batch size {} exceeds total eligible queue records {}",
            batch_size,
            eligible_records.len()
        ));
    }

    let mut candidates: Vec<KuwikiReviewBatchCandidate> = Vec::with_capacity(batch_size);

    for (b_idx, queue_rec) in eligible_records.iter().take(batch_size).enumerate() {
        let batch_rank = b_idx + 1;

        if batch_rank != queue_rec.rank {
            return Err(format!(
                "Batch rank mapping mismatch: batch_rank is {}, original_queue_rank is {}",
                batch_rank, queue_rec.rank
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

        candidates.push(candidate);
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

    // 1. candidates.jsonl (COMMITTED — contains context_references, NO copyright snippet text)
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

    // 4. Generate LOCAL/REPORT Human Review Guide (`data/reports/vocabulary-review/kuwiki-batch-001/review-guide.md`)
    let local_reports_dir = root.join("data/reports/vocabulary-review").join(batch_id);
    fs::create_dir_all(&local_reports_dir).map_err(|e| {
        format!(
            "Failed to create local reports dir {:?}: {}",
            local_reports_dir, e
        )
    })?;

    let local_guide_path = local_reports_dir.join("review-guide.md");
    let mut guide_file = File::create(&local_guide_path).map_err(|e| {
        format!(
            "Failed to create local review guide {:?}: {}",
            local_guide_path, e
        )
    })?;

    writeln!(
        guide_file,
        "# Kurmancî Human Vocabulary Review Guide — {}",
        batch_id
    )
    .unwrap();
    writeln!(guide_file).unwrap();
    writeln!(
        guide_file,
        "This is a reviewable first batch of top {batch_size} highest-attestation **{corpus_id}** OOV candidates generated for human lexical review."
    )
    .unwrap();
    writeln!(guide_file).unwrap();
    writeln!(guide_file, "## Batch Provenance & Integrity").unwrap();
    writeln!(guide_file, "- **Batch ID**: `{}`", batch_id).unwrap();
    writeln!(
        guide_file,
        "- **Source Corpus**: `{}` (version `{}`)",
        corpus_id, source_version
    )
    .unwrap();
    writeln!(
        guide_file,
        "- **Input Queue SHA-256**: `{}`",
        input_queue_sha256
    )
    .unwrap();
    writeln!(
        guide_file,
        "- **Experimental Lexicon Fingerprint**: `{}`",
        exp_fingerprint
    )
    .unwrap();
    writeln!(
        guide_file,
        "- **Selection Policy**: `top-{}-eligible`",
        batch_size
    )
    .unwrap();
    writeln!(
        guide_file,
        "- **Committed Candidates File**: `data/review-batches/{}/candidates.jsonl` (`{}`)",
        batch_id, candidates_sha256
    )
    .unwrap();
    writeln!(guide_file).unwrap();

    writeln!(guide_file, "## Batch Descriptive Statistics").unwrap();
    writeln!(guide_file, "- **Total Candidates**: `{}`", batch_size).unwrap();
    writeln!(
        guide_file,
        "- **Document Attestation Count**: min=`{}`, median=`{}`, max=`{}`",
        doc_count_min, doc_count_median, doc_count_max
    )
    .unwrap();
    writeln!(
        guide_file,
        "- **Token Occurrence Count**: min=`{}`, median=`{}`, max=`{}`",
        token_count_min, token_count_median, token_count_max
    )
    .unwrap();
    writeln!(
        guide_file,
        "- **Attestation Thresholds**: `>=100 docs`: `{}`, `>=500 docs`: `{}`, `>=1,000 docs`: `{}`",
        gte_100_docs_count, gte_500_docs_count, gte_1000_docs_count
    )
    .unwrap();
    writeln!(
        guide_file,
        "- **Context Coverage**: 3+ contexts: `{}`, 2 contexts: `{}`, 1 context: `{}`, Lacking: `{}`",
        contexts_3_count, contexts_2_count, contexts_1_count, contexts_lacking_count
    )
    .unwrap();
    writeln!(
        guide_file,
        "- **Candidates containing non-ASCII characters**: `{}`",
        non_ascii_candidate_count
    )
    .unwrap();
    writeln!(guide_file).unwrap();

    writeln!(guide_file, "## Special Diagnostic Targets Status").unwrap();
    writeln!(
        guide_file,
        "| Target | Normalized | Present in Batch | Batch Rank | Docs | Tokens |"
    )
    .unwrap();
    writeln!(
        guide_file,
        "| :--- | :--- | :---: | :---: | :---: | :---: |"
    )
    .unwrap();
    for st in &special_targets_presence {
        let pres_str = if st.present_in_batch { "Yes" } else { "No" };
        let rank_str = st
            .batch_rank
            .map(|r| r.to_string())
            .unwrap_or_else(|| "-".to_string());
        writeln!(
            guide_file,
            "| `{}` | `{}` | {} | {} | {} | {} |",
            st.target, st.normalized_target, pres_str, rank_str, st.document_count, st.token_count
        )
        .unwrap();
    }
    writeln!(guide_file).unwrap();

    writeln!(
        guide_file,
        "## Candidate Review Table (Ranks 1..{})",
        batch_size
    )
    .unwrap();
    writeln!(
        guide_file,
        "All candidates begin in `pending` state awaiting human Kurmancî review."
    )
    .unwrap();
    writeln!(guide_file).unwrap();

    for (b_idx, cand) in candidates.iter().enumerate() {
        let orig_rec = &eligible_records[b_idx];

        writeln!(
            guide_file,
            "### Rank {}: `{}` (Normalized: `{}`)",
            cand.batch_rank, cand.token, cand.normalized_token
        )
        .unwrap();
        writeln!(
            guide_file,
            "- **Attestation**: `{}` documents | `{}` token occurrences | Zipf: `{:.2}`",
            cand.document_count,
            cand.token_count,
            cand.zipf_milli as f64 / 1000.0
        )
        .unwrap();
        writeln!(
            guide_file,
            "- **Pack Membership**: Seed: `{}` | Reviewed: `{}` | Experimental-Full: `{}`",
            cand.in_seed, cand.in_reviewed, cand.in_experimental_full
        )
        .unwrap();
        writeln!(guide_file, "- **Decision Box**: `[ ] Pending Human Review`").unwrap();

        if !orig_rec.representative_contexts.is_empty() {
            writeln!(
                guide_file,
                "- **Representative Contexts (Local Generated View)**:"
            )
            .unwrap();
            for (c_idx, ctx) in orig_rec.representative_contexts.iter().enumerate() {
                writeln!(
                    guide_file,
                    "  {}. `[{}]` *\"{}\"*",
                    c_idx + 1,
                    ctx.document_id,
                    ctx.snippet
                )
                .unwrap();
            }
        }
        writeln!(guide_file).unwrap();
    }
    drop(guide_file);

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
        local_guide_path: local_guide_path.to_string_lossy().to_string(),
    };

    Ok(summary)
}
