//! Fail-closed source-specific adapter for validating `kuwiki-batch-001` human decisions.

use serde::{Deserialize, Serialize};
use serde_json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::pack::selection::{EntryPopulation, SelectedCandidate, SelectionCounts};
use crate::review::kuwiki_batch::KuwikiReviewBatchCandidate;
use crate::review::schema::{
    compute_entry_id, validate_decision_record, ReviewDecisionRecord, ReviewDecisionStatus,
    ReviewTargetType,
};
use crate::sources::SourceRegistry;

pub const EXPECTED_KUWIKI_BATCH_ID: &str = "kuwiki-batch-001";
pub const EXPECTED_KUWIKI_CANDIDATES_SHA256: &str =
    "23d3871a8f6ef285ba9b6f231fe5d65f201934eaee2965d18cdec7770aeb3c1d";
pub const EXPECTED_WORKSHEET_SHA256: &str =
    "7c1341d75a2a1e8530495d9c69c45e10e7ba991f745ccf8a69a8c75db81af4b2";
pub const EXPECTED_DECISIONS_SHA256: &str =
    "4ff95ee54de2170137dc8965b16e1ebd1e3724159e6964b1bf8c47465137103f";
pub const EXPECTED_REVIEWER_ID: &str = "ferhatguneri";
pub const EXPECTED_AUDIT_CONFIRMATION_DATE: &str = "2026-09-02";

pub const EXPECTED_APPROVED_COUNT: usize = 733;
pub const EXPECTED_APPROVED_WITH_METADATA_CHANGE_COUNT: usize = 0;
pub const EXPECTED_REJECTED_FROM_DEFAULT_PACK_COUNT: usize = 214;
pub const EXPECTED_EXPERIMENTAL_ONLY_COUNT: usize = 3;
pub const EXPECTED_NEEDS_LINGUIST_COUNT: usize = 50;
pub const EXPECTED_NEEDS_SOURCE_INVESTIGATION_COUNT: usize = 0;
pub const EXPECTED_PENDING_COUNT: usize = 0;
pub const EXPECTED_TOTAL_DECISIONS_COUNT: usize = 1000;
pub const EXPECTED_DATE_POLICY_CONFIRMED_COUNT: usize = 26;

pub const EXACT_DATE_POLICY_RANKS: [usize; 26] = [
    608, 639, 669, 696, 733, 736, 741, 751, 778, 811, 841, 846, 850, 865, 882, 888, 907, 940, 956,
    968, 969, 971, 978, 979, 980, 986,
];

pub const EXPECTED_KUWIKI_BATCH_002_ID: &str = "kuwiki-batch-002";
pub const EXPECTED_KUWIKI_BATCH_002_CANDIDATES_SHA256: &str =
    "f1d0dd010f9093399807e8d78c8cd1e13c5147a3b5ca5058f0c5bc6990b83308";
pub const EXPECTED_KUWIKI_BATCH_002_WORKSHEET_SHA256: &str =
    "c5f94bdec5fddcb2980bcc98bd1d89a7edede154c8813a934b26f4f4046471f7";
pub const EXPECTED_KUWIKI_BATCH_002_DECISIONS_SHA256: &str =
    "cdaaaaf98f05c0747b0623ccd48dfccabda2e4ffc8607077b2230a6c8b0984f4";
pub const EXPECTED_KUWIKI_BATCH_002_REVIEWER_ID: &str = "ferhatguneri";
pub const EXPECTED_KUWIKI_BATCH_002_AUDIT_CONFIRMATION_DATE: &str = "2026-09-10";

pub const EXPECTED_KUWIKI_BATCH_002_APPROVED_COUNT: usize = 592;
pub const EXPECTED_KUWIKI_BATCH_002_APPROVED_WITH_METADATA_CHANGE_COUNT: usize = 0;
pub const EXPECTED_KUWIKI_BATCH_002_REJECTED_FROM_DEFAULT_PACK_COUNT: usize = 297;
pub const EXPECTED_KUWIKI_BATCH_002_EXPERIMENTAL_ONLY_COUNT: usize = 2;
pub const EXPECTED_KUWIKI_BATCH_002_NEEDS_LINGUIST_COUNT: usize = 109;
pub const EXPECTED_KUWIKI_BATCH_002_NEEDS_SOURCE_INVESTIGATION_COUNT: usize = 0;
pub const EXPECTED_KUWIKI_BATCH_002_PENDING_COUNT: usize = 0;
pub const EXPECTED_KUWIKI_BATCH_002_TOTAL_DECISIONS_COUNT: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionCountsManifest {
    pub approved: usize,
    pub approved_with_metadata_change: usize,
    pub rejected_from_default_pack: usize,
    pub experimental_only: usize,
    pub needs_linguist: usize,
    pub needs_source_investigation: usize,
    pub pending: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionProvenanceManifest {
    pub schema_version: String,
    pub source_id: String,
    pub batch_id: String,
    pub candidate_sha256: String,
    pub worksheet_sha256: String,
    pub decisions_sha256: String,
    pub reviewer_id: String,
    pub audit_confirmation_date: String,
    pub counts: DecisionCountsManifest,
    pub human_confirmed_date_year_policy_count: usize,
    pub unresolved_auto_decisions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KuwikiBatchManifest {
    pub schema_version: String,
    pub batch_id: String,
    pub source_corpus_id: String,
    pub selection_policy: String,
    pub batch_size: usize,
    pub candidates_file: String,
    pub candidates_sha256: String,
}

/// Verified snapshot of `kuwiki-batch-001` candidates and human review decisions.
#[derive(Debug, Clone)]
pub struct KuwikiDecisionsSnapshot {
    pub batch_id: String,
    pub candidate_artifact_sha256: String,
    pub decision_file_sha256: String,
    pub batch_manifest_sha256: String,
    pub decision_provenance_manifest_sha256: String,
    pub candidates: Vec<KuwikiReviewBatchCandidate>,
    pub decisions: Vec<ReviewDecisionRecord>,
    pub counts_by_status: BTreeMap<String, usize>,
}

fn calculate_file_sha256<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let mut file = File::open(&path)
        .map_err(|e| format!("Failed to open for hashing {:?}: {}", path.as_ref(), e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .map_err(|e| format!("Failed to hash file {:?}: {}", path.as_ref(), e))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Reusable helper for fail-closed verification of `artifacts.sha256` manifests.
pub fn verify_artifacts_sha256_manifest<P: AsRef<Path>, Q: AsRef<Path>>(
    artifacts_path: P,
    target_dir: Q,
    required_files: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let art_path = artifacts_path.as_ref();
    let dir = target_dir.as_ref();

    if !art_path.exists() {
        return Err(format!("Missing artifacts.sha256 file at {:?}", art_path));
    }

    let content = std::fs::read_to_string(art_path)
        .map_err(|e| format!("Failed to read artifacts.sha256 at {:?}: {}", art_path, e))?;

    let required_set: BTreeSet<&str> = required_files.iter().copied().collect();
    let mut parsed_entries: BTreeMap<String, String> = BTreeMap::new();
    let mut seen_filenames: BTreeSet<String> = BTreeSet::new();

    for (l_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(format!(
                "Malformed line {} in artifacts.sha256 at {:?}: '{}'",
                l_idx + 1,
                art_path,
                line
            ));
        }

        let hash = parts[0];
        let filename = parts[1];

        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "Malformed line {} in artifacts.sha256 at {:?}: hash '{}' is not 64 hex digits",
                l_idx + 1,
                art_path,
                hash
            ));
        }

        if !required_set.contains(filename) {
            return Err(format!(
                "Unexpected or disallowed filename '{}' in artifacts.sha256 at {:?}",
                filename, art_path
            ));
        }

        if !seen_filenames.insert(filename.to_string()) {
            return Err(format!(
                "Duplicate entry for filename '{}' in artifacts.sha256 at {:?}",
                filename, art_path
            ));
        }

        parsed_entries.insert(filename.to_string(), hash.to_lowercase());
    }

    for req in required_files {
        if !parsed_entries.contains_key(*req) {
            return Err(format!(
                "Missing required entry '{}' in artifacts.sha256 at {:?}",
                req, art_path
            ));
        }
    }

    for (filename, declared_sha) in &parsed_entries {
        let file_path = dir.join(filename);
        if !file_path.exists() {
            return Err(format!(
                "File '{}' referenced in artifacts.sha256 missing at {:?}",
                filename, file_path
            ));
        }
        let actual_sha = calculate_file_sha256(&file_path)?;
        if actual_sha != *declared_sha {
            return Err(format!(
                "Mismatched hash for file '{}' in artifacts.sha256 at {:?}: declared '{}', actual '{}'",
                filename, art_path, declared_sha, actual_sha
            ));
        }
    }

    Ok(parsed_entries)
}

/// Spec for a Kuwiki decision batch.
#[derive(Debug, Clone)]
pub struct KuwikiDecisionBatchSpec {
    pub batch_id: &'static str,
    pub candidates_sha256: &'static str,
    pub worksheet_sha256: &'static str,
    pub decisions_sha256: &'static str,
    pub reviewer_id: &'static str,
    pub audit_confirmation_date: &'static str,
    pub selection_policy: &'static str,
    pub human_confirmed_date_year_policy_count: usize,
    pub expected_counts: DecisionCountsManifest,
    pub date_policy_ranks: Option<&'static [usize]>,
    pub require_decision_artifacts_sha256: bool,
}

pub static BATCH_001_SPEC: KuwikiDecisionBatchSpec = KuwikiDecisionBatchSpec {
    batch_id: EXPECTED_KUWIKI_BATCH_ID,
    candidates_sha256: EXPECTED_KUWIKI_CANDIDATES_SHA256,
    worksheet_sha256: EXPECTED_WORKSHEET_SHA256,
    decisions_sha256: EXPECTED_DECISIONS_SHA256,
    reviewer_id: EXPECTED_REVIEWER_ID,
    audit_confirmation_date: EXPECTED_AUDIT_CONFIRMATION_DATE,
    selection_policy: "top-1000-eligible",
    human_confirmed_date_year_policy_count: 26,
    expected_counts: DecisionCountsManifest {
        approved: EXPECTED_APPROVED_COUNT,
        approved_with_metadata_change: EXPECTED_APPROVED_WITH_METADATA_CHANGE_COUNT,
        rejected_from_default_pack: EXPECTED_REJECTED_FROM_DEFAULT_PACK_COUNT,
        experimental_only: EXPECTED_EXPERIMENTAL_ONLY_COUNT,
        needs_linguist: EXPECTED_NEEDS_LINGUIST_COUNT,
        needs_source_investigation: EXPECTED_NEEDS_SOURCE_INVESTIGATION_COUNT,
        pending: EXPECTED_PENDING_COUNT,
        total: EXPECTED_TOTAL_DECISIONS_COUNT,
    },
    date_policy_ranks: Some(&EXACT_DATE_POLICY_RANKS),
    require_decision_artifacts_sha256: false,
};

pub static BATCH_002_SPEC: KuwikiDecisionBatchSpec = KuwikiDecisionBatchSpec {
    batch_id: EXPECTED_KUWIKI_BATCH_002_ID,
    candidates_sha256: EXPECTED_KUWIKI_BATCH_002_CANDIDATES_SHA256,
    worksheet_sha256: EXPECTED_KUWIKI_BATCH_002_WORKSHEET_SHA256,
    decisions_sha256: EXPECTED_KUWIKI_BATCH_002_DECISIONS_SHA256,
    reviewer_id: EXPECTED_KUWIKI_BATCH_002_REVIEWER_ID,
    audit_confirmation_date: EXPECTED_KUWIKI_BATCH_002_AUDIT_CONFIRMATION_DATE,
    selection_policy: "top-1000-eligible",
    human_confirmed_date_year_policy_count: 0,
    expected_counts: DecisionCountsManifest {
        approved: EXPECTED_KUWIKI_BATCH_002_APPROVED_COUNT,
        approved_with_metadata_change:
            EXPECTED_KUWIKI_BATCH_002_APPROVED_WITH_METADATA_CHANGE_COUNT,
        rejected_from_default_pack: EXPECTED_KUWIKI_BATCH_002_REJECTED_FROM_DEFAULT_PACK_COUNT,
        experimental_only: EXPECTED_KUWIKI_BATCH_002_EXPERIMENTAL_ONLY_COUNT,
        needs_linguist: EXPECTED_KUWIKI_BATCH_002_NEEDS_LINGUIST_COUNT,
        needs_source_investigation: EXPECTED_KUWIKI_BATCH_002_NEEDS_SOURCE_INVESTIGATION_COUNT,
        pending: EXPECTED_KUWIKI_BATCH_002_PENDING_COUNT,
        total: EXPECTED_KUWIKI_BATCH_002_TOTAL_DECISIONS_COUNT,
    },
    date_policy_ranks: None,
    require_decision_artifacts_sha256: true,
};

pub static KNOWN_BATCH_SPECS: &[&KuwikiDecisionBatchSpec] = &[&BATCH_001_SPEC, &BATCH_002_SPEC];

fn load_and_validate_kuwiki_decision_batch_internal<P: AsRef<Path>>(
    root_dir: P,
    spec: &KuwikiDecisionBatchSpec,
) -> Result<Option<KuwikiDecisionsSnapshot>, String> {
    let root = root_dir.as_ref();
    let registry_path = root.join("data/source-registry/sources.toml");
    if !registry_path.exists() {
        return Ok(None);
    }

    let registry = SourceRegistry::load_from_file(&registry_path)?;
    let is_registered = registry
        .sources
        .iter()
        .any(|s| s.source_id == spec.batch_id);

    if !is_registered {
        return Ok(None);
    }

    let batch_dir = root.join("data/review-batches").join(spec.batch_id);
    let candidates_path = batch_dir.join("candidates.jsonl");
    let batch_manifest_path = batch_dir.join("manifest.json");
    let batch_artifacts_path = batch_dir.join("artifacts.sha256");

    let decisions_dir = root.join("data/review-decisions").join(spec.batch_id);
    let decisions_path = decisions_dir.join("decisions.jsonl");
    let decision_provenance_path = decisions_dir.join("manifest.json");
    let decisions_artifacts_path = decisions_dir.join("artifacts.sha256");

    if !candidates_path.exists() {
        return Err(format!(
            "Authoritative candidate file missing at {:?}",
            candidates_path
        ));
    }
    if !decisions_path.exists() {
        return Err(format!(
            "Authoritative decision file missing at {:?}",
            decisions_path
        ));
    }
    if !batch_manifest_path.exists() {
        return Err(format!(
            "Authoritative batch manifest missing at {:?}",
            batch_manifest_path
        ));
    }
    if !batch_artifacts_path.exists() {
        return Err(format!(
            "Authoritative batch artifacts.sha256 missing at {:?}",
            batch_artifacts_path
        ));
    }
    if !decision_provenance_path.exists() {
        return Err(format!(
            "Authoritative decision provenance manifest missing at {:?}",
            decision_provenance_path
        ));
    }
    if spec.require_decision_artifacts_sha256 && !decisions_artifacts_path.exists() {
        return Err(format!(
            "Authoritative decision artifacts.sha256 missing at {:?}",
            decisions_artifacts_path
        ));
    }

    // 1. Verify candidate artifacts.sha256 chain
    let cand_artifacts_map = verify_artifacts_sha256_manifest(
        &batch_artifacts_path,
        &batch_dir,
        &["candidates.jsonl", "manifest.json"],
    )?;
    let cand_file_sha256 = cand_artifacts_map["candidates.jsonl"].clone();
    if cand_file_sha256 != spec.candidates_sha256 {
        return Err(format!(
            "Candidate batch SHA-256 mismatch for batch {}: actual '{}', expected '{}'",
            spec.batch_id, cand_file_sha256, spec.candidates_sha256
        ));
    }

    // 2. Verify decision artifacts.sha256 chain (if required or present)
    if spec.require_decision_artifacts_sha256 || decisions_artifacts_path.exists() {
        verify_artifacts_sha256_manifest(
            &decisions_artifacts_path,
            &decisions_dir,
            &["decisions.jsonl", "manifest.json"],
        )?;
    }

    // 3. Verify batch manifest content
    let batch_manifest_sha256 = calculate_file_sha256(&batch_manifest_path)?;
    let batch_manifest_bytes = std::fs::read(&batch_manifest_path).map_err(|e| {
        format!(
            "Failed to read batch manifest {:?}: {}",
            batch_manifest_path, e
        )
    })?;
    let batch_manifest: KuwikiBatchManifest = serde_json::from_slice(&batch_manifest_bytes)
        .map_err(|e| {
            format!(
                "JSON error in batch manifest {:?}: {}",
                batch_manifest_path, e
            )
        })?;

    if batch_manifest.batch_id != spec.batch_id {
        return Err(format!(
            "Batch manifest batch_id mismatch: got '{}', expected '{}'",
            batch_manifest.batch_id, spec.batch_id
        ));
    }
    if batch_manifest.source_corpus_id != "kuwiki" {
        return Err(format!(
            "Batch manifest source_corpus_id mismatch: got '{}', expected 'kuwiki'",
            batch_manifest.source_corpus_id
        ));
    }
    if batch_manifest.batch_size != spec.expected_counts.total {
        return Err(format!(
            "Batch manifest batch_size mismatch: got {}, expected {}",
            batch_manifest.batch_size, spec.expected_counts.total
        ));
    }
    if batch_manifest.selection_policy != spec.selection_policy {
        return Err(format!(
            "Batch manifest selection_policy mismatch: got '{}', expected '{}'",
            batch_manifest.selection_policy, spec.selection_policy
        ));
    }
    if batch_manifest.candidates_sha256 != spec.candidates_sha256 {
        return Err(format!(
            "Batch manifest candidates_sha256 mismatch: got '{}', expected '{}'",
            batch_manifest.candidates_sha256, spec.candidates_sha256
        ));
    }

    // 4. Verify decision file & provenance manifest
    let decision_file_sha256 = calculate_file_sha256(&decisions_path)?;
    let decision_provenance_manifest_sha256 = calculate_file_sha256(&decision_provenance_path)?;
    let dev_prov_bytes = std::fs::read(&decision_provenance_path).map_err(|e| {
        format!(
            "Failed to read decision provenance manifest {:?}: {}",
            decision_provenance_path, e
        )
    })?;
    let dev_prov: DecisionProvenanceManifest =
        serde_json::from_slice(&dev_prov_bytes).map_err(|e| {
            format!(
                "JSON error in decision provenance manifest {:?}: {}",
                decision_provenance_path, e
            )
        })?;

    if dev_prov.schema_version != "kuwiki-decision-provenance-v1" {
        return Err(format!(
            "Decision provenance schema_version mismatch: got '{}', expected 'kuwiki-decision-provenance-v1'",
            dev_prov.schema_version
        ));
    }
    if dev_prov.source_id != spec.batch_id {
        return Err(format!(
            "Decision provenance source_id mismatch: got '{}', expected '{}'",
            dev_prov.source_id, spec.batch_id
        ));
    }
    if dev_prov.batch_id != spec.batch_id {
        return Err(format!(
            "Decision provenance batch_id mismatch: got '{}', expected '{}'",
            dev_prov.batch_id, spec.batch_id
        ));
    }
    if dev_prov.candidate_sha256 != spec.candidates_sha256 {
        return Err(format!(
            "Decision provenance candidate_sha256 mismatch: got '{}', expected '{}'",
            dev_prov.candidate_sha256, spec.candidates_sha256
        ));
    }
    if dev_prov.worksheet_sha256 != spec.worksheet_sha256 {
        return Err(format!(
            "Decision provenance worksheet_sha256 mismatch: got '{}', expected '{}'",
            dev_prov.worksheet_sha256, spec.worksheet_sha256
        ));
    }
    if dev_prov.decisions_sha256 != decision_file_sha256 {
        return Err(format!(
            "Decision provenance decisions_sha256 mismatch: got '{}', actual file SHA '{}'",
            dev_prov.decisions_sha256, decision_file_sha256
        ));
    }
    if dev_prov.decisions_sha256 != spec.decisions_sha256 {
        return Err(format!(
            "Decision provenance decisions_sha256 mismatch: got '{}', expected '{}'",
            dev_prov.decisions_sha256, spec.decisions_sha256
        ));
    }
    if dev_prov.reviewer_id != spec.reviewer_id {
        return Err(format!(
            "Decision provenance reviewer_id mismatch: got '{}', expected '{}'",
            dev_prov.reviewer_id, spec.reviewer_id
        ));
    }
    if dev_prov.audit_confirmation_date != spec.audit_confirmation_date {
        return Err(format!(
            "Decision provenance audit_confirmation_date mismatch: got '{}', expected '{}'",
            dev_prov.audit_confirmation_date, spec.audit_confirmation_date
        ));
    }
    if dev_prov.counts.approved != spec.expected_counts.approved {
        return Err(format!(
            "Decision provenance approved count mismatch: got {}, expected {}",
            dev_prov.counts.approved, spec.expected_counts.approved
        ));
    }
    if dev_prov.counts.approved_with_metadata_change
        != spec.expected_counts.approved_with_metadata_change
    {
        return Err(format!(
            "Decision provenance approved_with_metadata_change mismatch: got {}, expected {}",
            dev_prov.counts.approved_with_metadata_change,
            spec.expected_counts.approved_with_metadata_change
        ));
    }
    if dev_prov.counts.rejected_from_default_pack != spec.expected_counts.rejected_from_default_pack
    {
        return Err(format!(
            "Decision provenance rejected count mismatch: got {}, expected {}",
            dev_prov.counts.rejected_from_default_pack,
            spec.expected_counts.rejected_from_default_pack
        ));
    }
    if dev_prov.counts.experimental_only != spec.expected_counts.experimental_only {
        return Err(format!(
            "Decision provenance experimental count mismatch: got {}, expected {}",
            dev_prov.counts.experimental_only, spec.expected_counts.experimental_only
        ));
    }
    if dev_prov.counts.needs_linguist != spec.expected_counts.needs_linguist {
        return Err(format!(
            "Decision provenance needs_linguist count mismatch: got {}, expected {}",
            dev_prov.counts.needs_linguist, spec.expected_counts.needs_linguist
        ));
    }
    if dev_prov.counts.needs_source_investigation != spec.expected_counts.needs_source_investigation
    {
        return Err(format!(
            "Decision provenance needs_source_investigation mismatch: got {}, expected {}",
            dev_prov.counts.needs_source_investigation,
            spec.expected_counts.needs_source_investigation
        ));
    }
    if dev_prov.counts.pending != spec.expected_counts.pending {
        return Err(format!(
            "Decision provenance pending count mismatch: got {}, expected {}",
            dev_prov.counts.pending, spec.expected_counts.pending
        ));
    }
    if dev_prov.counts.total != spec.expected_counts.total {
        return Err(format!(
            "Decision provenance total count mismatch: got {}, expected {}",
            dev_prov.counts.total, spec.expected_counts.total
        ));
    }
    if dev_prov.unresolved_auto_decisions != 0 {
        return Err(format!(
            "Decision provenance unresolved_auto_decisions mismatch: got {}, expected 0",
            dev_prov.unresolved_auto_decisions
        ));
    }
    if dev_prov.human_confirmed_date_year_policy_count
        != spec.human_confirmed_date_year_policy_count
    {
        return Err(format!(
            "Decision provenance human_confirmed_date_year_policy_count mismatch: got {}, expected {}",
            dev_prov.human_confirmed_date_year_policy_count,
            spec.human_confirmed_date_year_policy_count
        ));
    }

    // 5. Read candidate batch records
    let c_file = File::open(&candidates_path)
        .map_err(|e| format!("Failed to open candidate file {:?}: {}", candidates_path, e))?;
    let mut candidates = Vec::new();
    let mut target_to_candidate = BTreeMap::new();

    for (l_idx, line_res) in BufReader::new(c_file).lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error candidate line {}: {}", l_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let cand: KuwikiReviewBatchCandidate = serde_json::from_str(&line)
            .map_err(|e| format!("JSON error candidate line {}: {}", l_idx + 1, e))?;

        if cand.batch_rank != l_idx + 1 {
            return Err(format!(
                "Candidate rank discontinuity at line {}: candidate rank is {}, expected {}",
                l_idx + 1,
                cand.batch_rank,
                l_idx + 1
            ));
        }

        let canonical = crate::normalize::normalize_text(&cand.token);
        if cand.normalized_token != canonical {
            return Err(format!(
                "Candidate line {} token '{}' normalized_token '{}' is inconsistent with normalize_text '{}'",
                l_idx + 1, cand.token, cand.normalized_token, canonical
            ));
        }

        let cand_target_id = compute_entry_id(
            spec.batch_id,
            spec.candidates_sha256,
            &cand.token,
            &cand.normalized_token,
            "",
            &[],
        )?;

        if target_to_candidate
            .insert(cand_target_id.clone(), cand.clone())
            .is_some()
        {
            return Err(format!(
                "Duplicate candidate target_id '{}' at candidate line {}",
                cand_target_id,
                l_idx + 1
            ));
        }

        candidates.push(cand);
    }

    if candidates.len() != spec.expected_counts.total {
        return Err(format!(
            "Candidate count mismatch: got {}, expected {}",
            candidates.len(),
            spec.expected_counts.total
        ));
    }

    // 6. Read decisions file
    let d_file = File::open(&decisions_path)
        .map_err(|e| format!("Failed to open decisions file {:?}: {}", decisions_path, e))?;
    let mut decisions = Vec::new();
    let mut target_to_decision = BTreeMap::new();
    let mut counts_by_status = BTreeMap::new();
    let exact_date_ranks_set: BTreeSet<usize> = spec
        .date_policy_ranks
        .map(|r| r.iter().copied().collect())
        .unwrap_or_default();
    let mut actual_date_policy_ranks = BTreeSet::new();

    for (l_idx, line_res) in BufReader::new(d_file).lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error decisions line {}: {}", l_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let dec: ReviewDecisionRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON error decisions line {}: {}", l_idx + 1, e))?;

        validate_decision_record(&dec)
            .map_err(|e| format!("Validation error at decisions record {}: {}", l_idx + 1, e))?;

        if dec.source_id != spec.batch_id {
            return Err(format!(
                "Source ID mismatch at index {}: got '{}', expected '{}'",
                l_idx + 1,
                dec.source_id,
                spec.batch_id
            ));
        }

        if dec.target_type != ReviewTargetType::Entry {
            return Err(format!(
                "Target type mismatch at index {}: expected 'entry'",
                l_idx + 1
            ));
        }

        let cand_for_rank = target_to_candidate.get(&dec.target_id).ok_or_else(|| {
            format!(
                "Decision at line {} has target_id '{}' which is not present in candidate batch {}",
                l_idx + 1,
                dec.target_id,
                spec.batch_id
            )
        })?;

        let expected_evidence = format!(
            "data/review-batches/{}/candidates.jsonl:rank-{}",
            spec.batch_id, cand_for_rank.batch_rank
        );
        if dec.evidence != vec![expected_evidence.clone()] {
            return Err(format!(
                "Decision evidence mismatch at line {} for target_id '{}': got {:?}, expected [{:?}]",
                l_idx + 1,
                dec.target_id,
                dec.evidence,
                expected_evidence
            ));
        }

        let status_str = match dec.review_status {
            ReviewDecisionStatus::Approved => "approved",
            ReviewDecisionStatus::ApprovedWithMetadataChange => "approved_with_metadata_change",
            ReviewDecisionStatus::RejectedFromDefaultPack => "rejected_from_default_pack",
            ReviewDecisionStatus::ExperimentalOnly => "experimental_only",
            ReviewDecisionStatus::NeedsLinguist => "needs_linguist",
            ReviewDecisionStatus::NeedsSourceInvestigation => "needs_source_investigation",
            ReviewDecisionStatus::Unreviewed => "unreviewed",
        };

        if status_str == "unreviewed" {
            return Err(format!(
                "Unreviewed decision status at index {}: human decision required",
                l_idx + 1
            ));
        }

        *counts_by_status.entry(status_str.to_string()).or_insert(0) += 1;

        if spec.date_policy_ranks.is_some() {
            let notes_combined = format!(
                "{} {}",
                dec.review_notes.as_deref().unwrap_or_default(),
                serde_json::to_string(&dec.evidence).unwrap_or_default()
            );

            if notes_combined
                .to_lowercase()
                .contains("human-confirmed date/year policy")
            {
                if dec.review_status != ReviewDecisionStatus::RejectedFromDefaultPack {
                    return Err(format!(
                        "Date/year policy decision for target_id '{}' has status {:?}, expected RejectedFromDefaultPack",
                        dec.target_id,
                        dec.review_status
                    ));
                }
                actual_date_policy_ranks.insert(cand_for_rank.batch_rank);
            }
        }

        if target_to_decision
            .insert(dec.target_id.clone(), dec.clone())
            .is_some()
        {
            return Err(format!(
                "Duplicate target_id '{}' in decisions file at line {}",
                dec.target_id,
                l_idx + 1
            ));
        }

        decisions.push(dec);
    }

    if decisions.len() != spec.expected_counts.total {
        return Err(format!(
            "Decisions count mismatch: got {}, expected {}",
            decisions.len(),
            spec.expected_counts.total
        ));
    }

    let cand_target_ids: BTreeSet<&String> = target_to_candidate.keys().collect();
    let dec_target_ids: BTreeSet<&String> = target_to_decision.keys().collect();

    if cand_target_ids != dec_target_ids {
        let missing_in_decisions: Vec<&&String> =
            cand_target_ids.difference(&dec_target_ids).collect();
        let orphan_in_decisions: Vec<&&String> =
            dec_target_ids.difference(&cand_target_ids).collect();
        return Err(format!(
            "Target ID set mismatch between candidates and decisions for batch {}: missing decisions {:?}, orphan decisions {:?}",
            spec.batch_id, missing_in_decisions, orphan_in_decisions
        ));
    }

    if spec.date_policy_ranks.is_some() && actual_date_policy_ranks != exact_date_ranks_set {
        return Err(format!(
            "Date/year policy ranks set mismatch: actual {:?}, expected {:?}",
            actual_date_policy_ranks, exact_date_ranks_set
        ));
    }

    let approved = *counts_by_status.get("approved").unwrap_or(&0);
    let approved_meta = *counts_by_status
        .get("approved_with_metadata_change")
        .unwrap_or(&0);
    let rejected = *counts_by_status
        .get("rejected_from_default_pack")
        .unwrap_or(&0);
    let experimental = *counts_by_status.get("experimental_only").unwrap_or(&0);
    let needs_ling = *counts_by_status.get("needs_linguist").unwrap_or(&0);
    let needs_src = *counts_by_status
        .get("needs_source_investigation")
        .unwrap_or(&0);

    if approved != spec.expected_counts.approved {
        return Err(format!(
            "Count mismatch for 'approved': got {}, expected {}",
            approved, spec.expected_counts.approved
        ));
    }
    if approved_meta != spec.expected_counts.approved_with_metadata_change {
        return Err(format!(
            "Count mismatch for 'approved_with_metadata_change': got {}, expected {}",
            approved_meta, spec.expected_counts.approved_with_metadata_change
        ));
    }
    if rejected != spec.expected_counts.rejected_from_default_pack {
        return Err(format!(
            "Count mismatch for 'rejected_from_default_pack': got {}, expected {}",
            rejected, spec.expected_counts.rejected_from_default_pack
        ));
    }
    if experimental != spec.expected_counts.experimental_only {
        return Err(format!(
            "Count mismatch for 'experimental_only': got {}, expected {}",
            experimental, spec.expected_counts.experimental_only
        ));
    }
    if needs_ling != spec.expected_counts.needs_linguist {
        return Err(format!(
            "Count mismatch for 'needs_linguist': got {}, expected {}",
            needs_ling, spec.expected_counts.needs_linguist
        ));
    }
    if needs_src != spec.expected_counts.needs_source_investigation {
        return Err(format!(
            "Count mismatch for 'needs_source_investigation': got {}, expected {}",
            needs_src, spec.expected_counts.needs_source_investigation
        ));
    }

    Ok(Some(KuwikiDecisionsSnapshot {
        batch_id: spec.batch_id.to_string(),
        candidate_artifact_sha256: cand_file_sha256,
        decision_file_sha256,
        batch_manifest_sha256,
        decision_provenance_manifest_sha256,
        candidates,
        decisions,
        counts_by_status,
    }))
}

/// Loads and performs strict fail-closed validation of `kuwiki-batch-001` human review decisions.
pub fn load_and_validate_kuwiki_decisions<P: AsRef<Path>>(
    root_dir: P,
) -> Result<Option<KuwikiDecisionsSnapshot>, String> {
    load_and_validate_kuwiki_decision_batch_internal(root_dir, &BATCH_001_SPEC)
}

/// Loads and performs strict fail-closed validation of `kuwiki-batch-002` human review decisions.
pub fn load_and_validate_kuwiki_batch_002_decisions<P: AsRef<Path>>(
    root_dir: P,
) -> Result<Option<KuwikiDecisionsSnapshot>, String> {
    load_and_validate_kuwiki_decision_batch_internal(root_dir, &BATCH_002_SPEC)
}

/// Loads and validates all registered Kuwiki review decision batches in sequence.
pub fn load_and_validate_all_kuwiki_decisions<P: AsRef<Path>>(
    root_dir: P,
) -> Result<Vec<KuwikiDecisionsSnapshot>, String> {
    let mut snapshots = Vec::new();
    for spec in KNOWN_BATCH_SPECS {
        if let Some(s) = load_and_validate_kuwiki_decision_batch_internal(&root_dir, spec)? {
            snapshots.push(s);
        }
    }
    Ok(snapshots)
}

/// Selects `kuwiki-batch-001` candidate entries for controlled pack selection using target_id lookup map.
pub fn select_kuwiki_candidates_for_pack(
    pack_id: &str,
    snapshot: &KuwikiDecisionsSnapshot,
    counts: &mut SelectionCounts,
) -> Result<Vec<SelectedCandidate>, String> {
    let mut selected_candidates = Vec::new();

    let dec_map: BTreeMap<String, &ReviewDecisionRecord> = snapshot
        .decisions
        .iter()
        .map(|d| (d.target_id.clone(), d))
        .collect();

    for cand in &snapshot.candidates {
        let expected_target_id = compute_entry_id(
            &snapshot.batch_id,
            &snapshot.candidate_artifact_sha256,
            &cand.token,
            &cand.normalized_token,
            "",
            &[],
        )?;

        let dec = dec_map.get(&expected_target_id).ok_or_else(|| {
            format!(
                "Missing decision record for candidate rank {} ('{}')",
                cand.batch_rank, cand.token
            )
        })?;

        match pack_id {
            "reviewed" => match dec.review_status {
                ReviewDecisionStatus::Approved => {
                    selected_candidates.push(SelectedCandidate {
                        entry_id: dec.target_id.clone(),
                        display: cand.token.clone(),
                        normalized: cand.normalized_token.clone(),
                        population: EntryPopulation::ExternalApproved,
                        source_id: snapshot.batch_id.clone(),
                        source_lines: vec![],
                        flags: String::new(),
                        morphology: vec![],
                        part_of_speech: "unknown".to_string(),
                        status: "approved".to_string(),
                    });
                    counts.external_approved_selected += 1;
                }
                ReviewDecisionStatus::ApprovedWithMetadataChange => {
                    let repl = dec.replacement_metadata.as_ref().ok_or_else(|| {
                        format!("ApprovedWithMetadataChange missing replacement_metadata for target '{}'", dec.target_id)
                    })?;
                    selected_candidates.push(SelectedCandidate {
                        entry_id: dec.target_id.clone(),
                        display: repl.display.clone(),
                        normalized: repl.normalized.clone(),
                        population: EntryPopulation::ExternalApprovedMetadataChange,
                        source_id: snapshot.batch_id.clone(),
                        source_lines: vec![],
                        flags: repl.flags.clone().unwrap_or_default(),
                        morphology: repl.morphology.clone().unwrap_or_default(),
                        part_of_speech: repl
                            .part_of_speech
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "approved_with_metadata_change".to_string(),
                    });
                    counts.external_metadata_replacement_selected += 1;
                }
                _ => {
                    counts.external_excluded_by_status_count += 1;
                }
            },
            "experimental-full" => match dec.review_status {
                ReviewDecisionStatus::Approved => {
                    selected_candidates.push(SelectedCandidate {
                        entry_id: dec.target_id.clone(),
                        display: cand.token.clone(),
                        normalized: cand.normalized_token.clone(),
                        population: EntryPopulation::ExternalApproved,
                        source_id: snapshot.batch_id.clone(),
                        source_lines: vec![],
                        flags: String::new(),
                        morphology: vec![],
                        part_of_speech: "unknown".to_string(),
                        status: "approved".to_string(),
                    });
                    counts.external_approved_selected += 1;
                }
                ReviewDecisionStatus::ApprovedWithMetadataChange => {
                    let repl = dec.replacement_metadata.as_ref().ok_or_else(|| {
                        format!("ApprovedWithMetadataChange missing replacement_metadata for target '{}'", dec.target_id)
                    })?;
                    selected_candidates.push(SelectedCandidate {
                        entry_id: dec.target_id.clone(),
                        display: repl.display.clone(),
                        normalized: repl.normalized.clone(),
                        population: EntryPopulation::ExternalApprovedMetadataChange,
                        source_id: snapshot.batch_id.clone(),
                        source_lines: vec![],
                        flags: repl.flags.clone().unwrap_or_default(),
                        morphology: repl.morphology.clone().unwrap_or_default(),
                        part_of_speech: repl
                            .part_of_speech
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "approved_with_metadata_change".to_string(),
                    });
                    counts.external_metadata_replacement_selected += 1;
                }
                ReviewDecisionStatus::ExperimentalOnly => {
                    selected_candidates.push(SelectedCandidate {
                        entry_id: dec.target_id.clone(),
                        display: cand.token.clone(),
                        normalized: cand.normalized_token.clone(),
                        population: EntryPopulation::ExternalExperimentalOnly,
                        source_id: snapshot.batch_id.clone(),
                        source_lines: vec![],
                        flags: String::new(),
                        morphology: vec![],
                        part_of_speech: "unknown".to_string(),
                        status: "experimental_only".to_string(),
                    });
                    counts.external_experimental_selected += 1;
                }
                _ => {
                    counts.external_excluded_by_status_count += 1;
                }
            },
            _ => {
                return Err(format!(
                    "Unsupported pack_id '{}' for kuwiki selection",
                    pack_id
                ))
            }
        }
    }

    Ok(selected_candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_kuwiki_decisions_pre_validation_reorder_test() {
        let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Copy sources.toml
        std::fs::create_dir_all(root.join("data/source-registry")).unwrap();
        std::fs::copy(
            ws_root.join("data/source-registry/sources.toml"),
            root.join("data/source-registry/sources.toml"),
        )
        .unwrap();

        // Copy review-batches/kuwiki-batch-002
        let b2_dir = root.join("data/review-batches/kuwiki-batch-002");
        std::fs::create_dir_all(&b2_dir).unwrap();
        std::fs::copy(
            ws_root.join("data/review-batches/kuwiki-batch-002/candidates.jsonl"),
            b2_dir.join("candidates.jsonl"),
        )
        .unwrap();
        std::fs::copy(
            ws_root.join("data/review-batches/kuwiki-batch-002/manifest.json"),
            b2_dir.join("manifest.json"),
        )
        .unwrap();
        std::fs::copy(
            ws_root.join("data/review-batches/kuwiki-batch-002/artifacts.sha256"),
            b2_dir.join("artifacts.sha256"),
        )
        .unwrap();

        // Copy and REORDER decision records in review-decisions/kuwiki-batch-002 BEFORE loading
        let d2_dir = root.join("data/review-decisions/kuwiki-batch-002");
        std::fs::create_dir_all(&d2_dir).unwrap();

        let orig_dec_lines: Vec<String> = std::fs::read_to_string(
            ws_root.join("data/review-decisions/kuwiki-batch-002/decisions.jsonl"),
        )
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

        let mut reordered_dec_lines = orig_dec_lines.clone();
        reordered_dec_lines.reverse(); // Reverse decision line order

        let new_dec_content = reordered_dec_lines.join("\n") + "\n";
        std::fs::write(d2_dir.join("decisions.jsonl"), &new_dec_content).unwrap();

        let new_dec_sha = calculate_file_sha256(d2_dir.join("decisions.jsonl")).unwrap();

        // Update manifest.json in review-decisions with new decisions_sha256
        let orig_manifest_str = std::fs::read_to_string(
            ws_root.join("data/review-decisions/kuwiki-batch-002/manifest.json"),
        )
        .unwrap();

        let mut prov_val: serde_json::Value = serde_json::from_str(&orig_manifest_str).unwrap();
        prov_val["decisions_sha256"] = serde_json::Value::String(new_dec_sha.clone());
        let new_prov_str = serde_json::to_string_pretty(&prov_val).unwrap();
        std::fs::write(d2_dir.join("manifest.json"), &new_prov_str).unwrap();

        let new_prov_sha = calculate_file_sha256(d2_dir.join("manifest.json")).unwrap();

        // Update artifacts.sha256 in review-decisions
        let new_art_content = format!(
            "{}  decisions.jsonl\n{}  manifest.json\n",
            new_dec_sha, new_prov_sha
        );
        std::fs::write(d2_dir.join("artifacts.sha256"), new_art_content).unwrap();

        let mut reordered_spec = BATCH_002_SPEC.clone();
        let dec_sha_static: &'static str = Box::leak(new_dec_sha.into_boxed_str());
        reordered_spec.decisions_sha256 = dec_sha_static;

        let snapshot = load_and_validate_kuwiki_decision_batch_internal(root, &reordered_spec)
            .expect("Reordered decision records must validate successfully based on target_id")
            .expect("Snapshot should be present");

        assert_eq!(snapshot.decisions.len(), 1000);
    }
}
