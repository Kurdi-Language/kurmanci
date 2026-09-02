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

/// Loads and performs strict fail-closed validation of `kuwiki-batch-001` human review decisions.
pub fn load_and_validate_kuwiki_decisions<P: AsRef<Path>>(
    root_dir: P,
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
        .any(|s| s.source_id == EXPECTED_KUWIKI_BATCH_ID);

    if !is_registered {
        return Ok(None);
    }

    // Source is registered -> ALL authoritative files are REQUIRED (fail-closed)
    let batch_dir = root
        .join("data/review-batches")
        .join(EXPECTED_KUWIKI_BATCH_ID);
    let candidates_path = batch_dir.join("candidates.jsonl");
    let batch_manifest_path = batch_dir.join("manifest.json");
    let batch_artifacts_path = batch_dir.join("artifacts.sha256");

    let decisions_dir = root
        .join("data/review-decisions")
        .join(EXPECTED_KUWIKI_BATCH_ID);
    let decisions_path = decisions_dir.join("decisions.jsonl");
    let decision_provenance_path = decisions_dir.join("manifest.json");

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

    // 1. Verify candidate artifact SHA-256
    let cand_file_sha256 = calculate_file_sha256(&candidates_path)?;
    if cand_file_sha256 != EXPECTED_KUWIKI_CANDIDATES_SHA256 {
        return Err(format!(
            "Candidate batch SHA-256 mismatch: actual '{}', expected '{}'",
            cand_file_sha256, EXPECTED_KUWIKI_CANDIDATES_SHA256
        ));
    }

    // 2. Verify batch manifest SHA-256 and content
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

    if batch_manifest.batch_id != EXPECTED_KUWIKI_BATCH_ID {
        return Err(format!(
            "Batch manifest batch_id mismatch: got '{}', expected '{}'",
            batch_manifest.batch_id, EXPECTED_KUWIKI_BATCH_ID
        ));
    }
    if batch_manifest.source_corpus_id != "kuwiki" {
        return Err(format!(
            "Batch manifest source_corpus_id mismatch: got '{}', expected 'kuwiki'",
            batch_manifest.source_corpus_id
        ));
    }
    if batch_manifest.batch_size != EXPECTED_TOTAL_DECISIONS_COUNT {
        return Err(format!(
            "Batch manifest batch_size mismatch: got {}, expected {}",
            batch_manifest.batch_size, EXPECTED_TOTAL_DECISIONS_COUNT
        ));
    }
    if batch_manifest.candidates_sha256 != EXPECTED_KUWIKI_CANDIDATES_SHA256 {
        return Err(format!(
            "Batch manifest candidates_sha256 mismatch: got '{}', expected '{}'",
            batch_manifest.candidates_sha256, EXPECTED_KUWIKI_CANDIDATES_SHA256
        ));
    }
    if batch_manifest.selection_policy != "top-1000-eligible" {
        return Err(format!(
            "Batch manifest selection_policy mismatch: got '{}', expected 'top-1000-eligible'",
            batch_manifest.selection_policy
        ));
    }

    // 3. Verify batch artifacts.sha256 chain
    let artifacts_content = std::fs::read_to_string(&batch_artifacts_path).map_err(|e| {
        format!(
            "Failed to read artifacts.sha256 {:?}: {}",
            batch_artifacts_path, e
        )
    })?;
    let mut artifact_hashes = BTreeMap::new();
    for line in artifacts_content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(format!("Malformed line in artifacts.sha256: '{}'", line));
        }
        artifact_hashes.insert(parts[1].to_string(), parts[0].to_string());
    }

    let exp_cand_hash = artifact_hashes
        .get("candidates.jsonl")
        .ok_or_else(|| "Missing candidates.jsonl entry in batch artifacts.sha256".to_string())?;
    if exp_cand_hash != &cand_file_sha256 {
        return Err(format!(
            "artifacts.sha256 candidates.jsonl hash mismatch: artifact {}, actual {}",
            exp_cand_hash, cand_file_sha256
        ));
    }

    let exp_man_hash = artifact_hashes
        .get("manifest.json")
        .ok_or_else(|| "Missing manifest.json entry in batch artifacts.sha256".to_string())?;
    if exp_man_hash != &batch_manifest_sha256 {
        return Err(format!(
            "artifacts.sha256 manifest.json hash mismatch: artifact {}, actual {}",
            exp_man_hash, batch_manifest_sha256
        ));
    }

    // 4. Verify decision file SHA-256
    let decision_file_sha256 = calculate_file_sha256(&decisions_path)?;

    // 5. Verify decision provenance manifest
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
    if dev_prov.source_id != EXPECTED_KUWIKI_BATCH_ID {
        return Err(format!(
            "Decision provenance source_id mismatch: got '{}', expected '{}'",
            dev_prov.source_id, EXPECTED_KUWIKI_BATCH_ID
        ));
    }
    if dev_prov.batch_id != EXPECTED_KUWIKI_BATCH_ID {
        return Err(format!(
            "Decision provenance batch_id mismatch: got '{}', expected '{}'",
            dev_prov.batch_id, EXPECTED_KUWIKI_BATCH_ID
        ));
    }
    if dev_prov.candidate_sha256 != EXPECTED_KUWIKI_CANDIDATES_SHA256 {
        return Err(format!(
            "Decision provenance candidate_sha256 mismatch: got '{}', expected '{}'",
            dev_prov.candidate_sha256, EXPECTED_KUWIKI_CANDIDATES_SHA256
        ));
    }
    if dev_prov.worksheet_sha256 != EXPECTED_WORKSHEET_SHA256 {
        return Err(format!(
            "Decision provenance worksheet_sha256 mismatch: got '{}', expected '{}'",
            dev_prov.worksheet_sha256, EXPECTED_WORKSHEET_SHA256
        ));
    }
    if dev_prov.decisions_sha256 != decision_file_sha256 {
        return Err(format!(
            "Decision provenance decisions_sha256 mismatch: got '{}', actual file SHA '{}'",
            dev_prov.decisions_sha256, decision_file_sha256
        ));
    }
    if dev_prov.decisions_sha256 != EXPECTED_DECISIONS_SHA256 {
        return Err(format!(
            "Decision provenance decisions_sha256 mismatch: got '{}', expected '{}'",
            dev_prov.decisions_sha256, EXPECTED_DECISIONS_SHA256
        ));
    }
    if dev_prov.reviewer_id != EXPECTED_REVIEWER_ID {
        return Err(format!(
            "Decision provenance reviewer_id mismatch: got '{}', expected '{}'",
            dev_prov.reviewer_id, EXPECTED_REVIEWER_ID
        ));
    }
    if dev_prov.audit_confirmation_date != EXPECTED_AUDIT_CONFIRMATION_DATE {
        return Err(format!(
            "Decision provenance audit_confirmation_date mismatch: got '{}', expected '{}'",
            dev_prov.audit_confirmation_date, EXPECTED_AUDIT_CONFIRMATION_DATE
        ));
    }
    if dev_prov.counts.approved != EXPECTED_APPROVED_COUNT {
        return Err(format!(
            "Decision provenance approved count mismatch: got {}, expected {}",
            dev_prov.counts.approved, EXPECTED_APPROVED_COUNT
        ));
    }
    if dev_prov.counts.approved_with_metadata_change != EXPECTED_APPROVED_WITH_METADATA_CHANGE_COUNT
    {
        return Err(format!(
            "Decision provenance approved_with_metadata_change mismatch: got {}, expected {}",
            dev_prov.counts.approved_with_metadata_change,
            EXPECTED_APPROVED_WITH_METADATA_CHANGE_COUNT
        ));
    }
    if dev_prov.counts.rejected_from_default_pack != EXPECTED_REJECTED_FROM_DEFAULT_PACK_COUNT {
        return Err(format!(
            "Decision provenance rejected count mismatch: got {}, expected {}",
            dev_prov.counts.rejected_from_default_pack, EXPECTED_REJECTED_FROM_DEFAULT_PACK_COUNT
        ));
    }
    if dev_prov.counts.experimental_only != EXPECTED_EXPERIMENTAL_ONLY_COUNT {
        return Err(format!(
            "Decision provenance experimental count mismatch: got {}, expected {}",
            dev_prov.counts.experimental_only, EXPECTED_EXPERIMENTAL_ONLY_COUNT
        ));
    }
    if dev_prov.counts.needs_linguist != EXPECTED_NEEDS_LINGUIST_COUNT {
        return Err(format!(
            "Decision provenance needs_linguist count mismatch: got {}, expected {}",
            dev_prov.counts.needs_linguist, EXPECTED_NEEDS_LINGUIST_COUNT
        ));
    }
    if dev_prov.counts.needs_source_investigation != EXPECTED_NEEDS_SOURCE_INVESTIGATION_COUNT {
        return Err(format!(
            "Decision provenance needs_source_investigation mismatch: got {}, expected {}",
            dev_prov.counts.needs_source_investigation, EXPECTED_NEEDS_SOURCE_INVESTIGATION_COUNT
        ));
    }
    if dev_prov.counts.pending != EXPECTED_PENDING_COUNT {
        return Err(format!(
            "Decision provenance pending count mismatch: got {}, expected {}",
            dev_prov.counts.pending, EXPECTED_PENDING_COUNT
        ));
    }
    if dev_prov.counts.total != EXPECTED_TOTAL_DECISIONS_COUNT {
        return Err(format!(
            "Decision provenance total count mismatch: got {}, expected {}",
            dev_prov.counts.total, EXPECTED_TOTAL_DECISIONS_COUNT
        ));
    }
    if dev_prov.human_confirmed_date_year_policy_count != EXPECTED_DATE_POLICY_CONFIRMED_COUNT {
        return Err(format!(
            "Decision provenance date/year policy count mismatch: got {}, expected {}",
            dev_prov.human_confirmed_date_year_policy_count, EXPECTED_DATE_POLICY_CONFIRMED_COUNT
        ));
    }
    if dev_prov.unresolved_auto_decisions != 0 {
        return Err(format!(
            "Decision provenance unresolved_auto_decisions mismatch: got {}, expected 0",
            dev_prov.unresolved_auto_decisions
        ));
    }

    // Read candidate batch records and build deterministic target_id lookup map
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

        let cand_target_id = compute_entry_id(
            EXPECTED_KUWIKI_BATCH_ID,
            EXPECTED_KUWIKI_CANDIDATES_SHA256,
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

    if candidates.len() != EXPECTED_TOTAL_DECISIONS_COUNT {
        return Err(format!(
            "Candidate count mismatch: got {}, expected {}",
            candidates.len(),
            EXPECTED_TOTAL_DECISIONS_COUNT
        ));
    }

    let d_file = File::open(&decisions_path)
        .map_err(|e| format!("Failed to open decisions file {:?}: {}", decisions_path, e))?;
    let mut decisions = Vec::new();
    for (l_idx, line_res) in BufReader::new(d_file).lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error decisions line {}: {}", l_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let dec: ReviewDecisionRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON error decisions line {}: {}", l_idx + 1, e))?;
        decisions.push(dec);
    }

    let counts_by_status = validate_kuwiki_decision_records(&candidates, &decisions)?;

    Ok(Some(KuwikiDecisionsSnapshot {
        batch_id: EXPECTED_KUWIKI_BATCH_ID.to_string(),
        candidate_artifact_sha256: cand_file_sha256,
        decision_file_sha256,
        batch_manifest_sha256,
        decision_provenance_manifest_sha256,
        candidates,
        decisions,
        counts_by_status,
    }))
}

/// Validates candidate and decision records against batch-001 structural, date/year policy, and count invariants.
pub fn validate_kuwiki_decision_records(
    candidates: &[KuwikiReviewBatchCandidate],
    decisions: &[ReviewDecisionRecord],
) -> Result<BTreeMap<String, usize>, String> {
    let mut target_to_candidate = BTreeMap::new();
    for cand in candidates {
        let cand_target_id = compute_entry_id(
            EXPECTED_KUWIKI_BATCH_ID,
            EXPECTED_KUWIKI_CANDIDATES_SHA256,
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
                "Duplicate candidate target_id '{}'",
                cand_target_id
            ));
        }
    }

    if candidates.len() != EXPECTED_TOTAL_DECISIONS_COUNT {
        return Err(format!(
            "Candidate count mismatch: got {}, expected {}",
            candidates.len(),
            EXPECTED_TOTAL_DECISIONS_COUNT
        ));
    }

    let mut target_to_decision = BTreeMap::new();
    let mut counts_by_status = BTreeMap::new();
    let exact_date_ranks_set: BTreeSet<usize> = EXACT_DATE_POLICY_RANKS.into_iter().collect();
    let mut actual_date_policy_ranks = BTreeSet::new();

    for (l_idx, dec) in decisions.iter().enumerate() {
        validate_decision_record(dec)
            .map_err(|e| format!("Validation error at decisions record {}: {}", l_idx + 1, e))?;

        if dec.source_id != EXPECTED_KUWIKI_BATCH_ID {
            return Err(format!(
                "Source ID mismatch at index {}: got '{}', expected '{}'",
                l_idx + 1,
                dec.source_id,
                EXPECTED_KUWIKI_BATCH_ID
            ));
        }

        if dec.target_type != ReviewTargetType::Entry {
            return Err(format!(
                "Target type mismatch at index {}: expected 'entry'",
                l_idx + 1
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
            let cand = target_to_candidate.get(&dec.target_id).ok_or_else(|| {
                format!(
                    "Candidate not found for date/year policy decision target_id '{}'",
                    dec.target_id
                )
            })?;
            actual_date_policy_ranks.insert(cand.batch_rank);
        }

        if target_to_decision
            .insert(dec.target_id.clone(), dec.clone())
            .is_some()
        {
            return Err(format!(
                "Duplicate target_id '{}' in decisions file",
                dec.target_id
            ));
        }
    }

    if decisions.len() != EXPECTED_TOTAL_DECISIONS_COUNT {
        return Err(format!(
            "Decisions count mismatch: got {}, expected {}",
            decisions.len(),
            EXPECTED_TOTAL_DECISIONS_COUNT
        ));
    }

    for cand_target_id in target_to_candidate.keys() {
        if !target_to_decision.contains_key(cand_target_id) {
            return Err(format!(
                "Missing decision record for candidate target_id '{}'",
                cand_target_id
            ));
        }
    }
    for dec_target_id in target_to_decision.keys() {
        if !target_to_candidate.contains_key(dec_target_id) {
            return Err(format!(
                "Orphan decision record for target_id '{}' not present in candidates",
                dec_target_id
            ));
        }
    }

    if actual_date_policy_ranks != exact_date_ranks_set {
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

    if approved != EXPECTED_APPROVED_COUNT {
        return Err(format!(
            "Count mismatch for 'approved': got {}, expected {}",
            approved, EXPECTED_APPROVED_COUNT
        ));
    }
    if approved_meta != EXPECTED_APPROVED_WITH_METADATA_CHANGE_COUNT {
        return Err(format!(
            "Count mismatch for 'approved_with_metadata_change': got {}, expected {}",
            approved_meta, EXPECTED_APPROVED_WITH_METADATA_CHANGE_COUNT
        ));
    }
    if rejected != EXPECTED_REJECTED_FROM_DEFAULT_PACK_COUNT {
        return Err(format!(
            "Count mismatch for 'rejected_from_default_pack': got {}, expected {}",
            rejected, EXPECTED_REJECTED_FROM_DEFAULT_PACK_COUNT
        ));
    }
    if experimental != EXPECTED_EXPERIMENTAL_ONLY_COUNT {
        return Err(format!(
            "Count mismatch for 'experimental_only': got {}, expected {}",
            experimental, EXPECTED_EXPERIMENTAL_ONLY_COUNT
        ));
    }
    if needs_ling != EXPECTED_NEEDS_LINGUIST_COUNT {
        return Err(format!(
            "Count mismatch for 'needs_linguist': got {}, expected {}",
            needs_ling, EXPECTED_NEEDS_LINGUIST_COUNT
        ));
    }
    if needs_src != EXPECTED_NEEDS_SOURCE_INVESTIGATION_COUNT {
        return Err(format!(
            "Count mismatch for 'needs_source_investigation': got {}, expected {}",
            needs_src, EXPECTED_NEEDS_SOURCE_INVESTIGATION_COUNT
        ));
    }

    Ok(counts_by_status)
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
