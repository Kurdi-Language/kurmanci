//! Human review decision merger and audit report generator (`controlled-review-report-v1`).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::corpus::importer::LockFileGuard;
use crate::review::queues::{EntryQueueRecord, MetadataConflictGroupQueueRecord};
use crate::review::schema::{
    validate_decision_record, ReviewDecisionRecord, ReviewDecisionStatus, ReviewTargetType,
};

pub const CONTROLLED_REVIEW_REPORT_SCHEMA_VERSION: &str = "controlled-review-report-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReportProvenance {
    pub decisions_sha256: String,
    pub queue_manifest_sha256: String,
    pub source_revision: String,
    pub imported_lexicon_sha256: String,
}

/// Summary report emitted when human review decisions are validated and joined.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewMergerSummary {
    pub schema_version: String,
    pub source_id: String,
    pub total_decisions_count: usize,
    pub approved_count: usize,
    pub approved_with_metadata_change_count: usize,
    pub rejected_from_default_count: usize,
    pub experimental_only_count: usize,
    pub unresolved_count: usize,
    pub orphan_decisions_count: usize,
    pub decision_file_sha256: String,
    pub provenance: ReviewReportProvenance,
}

fn remove_dir_or_file<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    let p = path.as_ref();
    if p.is_dir() {
        fs::remove_dir_all(p)
    } else if p.exists() || p.symlink_metadata().is_ok() {
        fs::remove_file(p)
    } else {
        Ok(())
    }
}

/// Verifies existing review queues, decisions, and report artifacts without regenerating reports.
pub fn load_validated_review_snapshot<P: AsRef<Path>>(
    source_id: &str,
    root_dir: P,
) -> Result<ReviewMergerSummary, String> {
    let root = root_dir.as_ref();
    let decisions_dir = root.join(format!("data/review-decisions/{}", source_id));
    let decisions_file_path = decisions_dir.join("decisions.jsonl");

    if !decisions_file_path.exists() {
        return Err(format!(
            "Review decisions file missing at {:?}. Initialize data/review-decisions/{}/decisions.jsonl first.",
            decisions_file_path, source_id
        ));
    }

    let queues_dir = root.join(format!("data/review-queues/{}", source_id));
    if !queues_dir.exists() {
        return Err(format!(
            "Review queues directory missing at {:?}. Run generate-review-queues first.",
            queues_dir
        ));
    }

    // Verify queue manifest
    let q_manifest = queues_dir.join("artifacts.sha256");
    if !q_manifest.exists() {
        return Err(format!("Queue manifest missing at {:?}", q_manifest));
    }
    let q_manifest_content = fs::read_to_string(&q_manifest).map_err(|e| e.to_string())?;
    for line in q_manifest_content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(format!("Malformed queue manifest line: {}", line));
        }
        let expected_hash = parts[0];
        let rel_path = parts[1];
        let actual_path = root.join(rel_path);
        let actual_hash = format!(
            "{:x}",
            Sha256::digest(&fs::read(&actual_path).map_err(|e| e.to_string())?)
        );
        if actual_hash != expected_hash {
            return Err(format!(
                "Tampered queue file detected at {:?}: hash mismatch",
                actual_path
            ));
        }
    }

    // Verify review report directory
    let reports_dir = root.join("data/reports/controlled-lexicon-review");
    let r_manifest = reports_dir.join("artifacts.sha256");
    if !r_manifest.exists() {
        return Err(format!(
            "Controlled review report manifest missing at {:?}. Re-run validate-review-decisions.",
            r_manifest
        ));
    }
    let r_manifest_content = fs::read_to_string(&r_manifest).map_err(|e| e.to_string())?;
    for line in r_manifest_content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(format!("Malformed report manifest line: {}", line));
        }
        let expected_hash = parts[0];
        let rel_path = parts[1];
        let actual_path = root.join(rel_path);
        let actual_hash = format!(
            "{:x}",
            Sha256::digest(&fs::read(&actual_path).map_err(|e| e.to_string())?)
        );
        if actual_hash != expected_hash {
            return Err(format!(
                "Tampered review report detected at {:?}: hash mismatch",
                actual_path
            ));
        }
    }

    // Load summary
    let summary_path = reports_dir.join("summary.json");
    let summary_bytes = fs::read(&summary_path).map_err(|e| e.to_string())?;
    let summary: ReviewMergerSummary =
        serde_json::from_slice(&summary_bytes).map_err(|e| e.to_string())?;

    Ok(summary)
}

/// Validates human review decisions in `data/review-decisions/<source-id>/decisions.jsonl`
/// against review queues in `data/review-queues/<source-id>/` and writes merged audit reports under `data/reports/controlled-lexicon-review/`.
pub fn validate_review_decisions<P: AsRef<Path>>(
    source_id: &str,
    root_dir: P,
) -> Result<ReviewMergerSummary, String> {
    let root = root_dir.as_ref();
    let lock_path = root.join("data/review-decisions.lock");
    let lock = LockFileGuard::acquire(&lock_path)?;

    let decisions_dir = root.join(format!("data/review-decisions/{}", source_id));
    let decisions_file_path = decisions_dir.join("decisions.jsonl");

    if !decisions_file_path.exists() {
        return Err(format!(
            "Review decisions file missing at {:?}. Initialize data/review-decisions/{}/decisions.jsonl first.",
            decisions_file_path, source_id
        ));
    }

    // 1. Require queue directory and verify exact queue artifacts.sha256
    let queues_dir = root.join(format!("data/review-queues/{}", source_id));
    if !queues_dir.exists() {
        return Err(format!(
            "Review queues directory missing at {:?}. Run generate-review-queues first.",
            queues_dir
        ));
    }

    let expected_filenames: BTreeSet<&str> = [
        "artifacts.sha256",
        "capitalization-anomalies.jsonl",
        "digit-only.jsonl",
        "hunspell-only.jsonl",
        "metadata-conflict-groups.jsonl",
        "mixed-scripts.jsonl",
        "multiword-entries.jsonl",
        "no-letter.jsonl",
        "parser-rejections.jsonl",
        "punctuation-only.jsonl",
        "README.md",
        "rare-code-points.jsonl",
        "short-and-long-forms.jsonl",
        "summary.json",
        "suspicious-entries.jsonl",
        "symbol-only.jsonl",
        "unexpected-code-points.jsonl",
        "unusual-scripts.jsonl",
    ]
    .into_iter()
    .collect();

    // Check directory file set matches expected exact set (reject unmanifested/stale extra files!)
    let dir_entries = fs::read_dir(&queues_dir)
        .map_err(|e| format!("Failed to read review queues dir {:?}: {}", queues_dir, e))?;
    let mut actual_filenames = BTreeSet::new();
    for entry_res in dir_entries {
        let entry = entry_res.map_err(|e| format!("Dir entry error: {}", e))?;
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy().to_string();
        if !expected_filenames.contains(name_str.as_str()) {
            return Err(format!(
                "Unexpected file {:?} found in review queues directory {:?}",
                name_str, queues_dir
            ));
        }
        actual_filenames.insert(name_str);
    }

    if actual_filenames
        != expected_filenames
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>()
    {
        return Err(
            "Queue directory file set does not match expected exact queue artifact set".to_string(),
        );
    }

    let manifest_path = queues_dir.join("artifacts.sha256");
    if !manifest_path.exists() {
        return Err(format!(
            "Review queue manifest missing at {:?}. Re-run generate-review-queues.",
            manifest_path
        ));
    }

    let manifest_content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Failed to read queue manifest {:?}: {}", manifest_path, e))?;

    let mut manifest_filenames = BTreeSet::new();
    manifest_filenames.insert("artifacts.sha256".to_string());

    for line in manifest_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(format!(
                "Malformed entry in queue manifest {:?}: '{}'",
                manifest_path, trimmed
            ));
        }
        let expected_hash = parts[0];
        let rel_path = parts[1];

        // Reject path traversal
        if rel_path.contains("..") || rel_path.starts_with('/') {
            return Err(format!(
                "Path traversal rejected in queue manifest: '{}'",
                rel_path
            ));
        }

        let prefix = format!("data/review-queues/{}/", source_id);
        if !rel_path.starts_with(&prefix) {
            return Err(format!(
                "Manifest path '{}' is outside expected queue directory '{}'",
                rel_path, prefix
            ));
        }

        let fname = rel_path.trim_start_matches(&prefix).to_string();
        manifest_filenames.insert(fname);

        let abs_path = root.join(rel_path);
        if !abs_path.exists() {
            return Err(format!(
                "Missing queue artifact {:?} declared in manifest",
                abs_path
            ));
        }
        let content = fs::read(&abs_path)
            .map_err(|e| format!("Failed to read queue artifact {:?}: {}", abs_path, e))?;
        let actual_hash = format!("{:x}", Sha256::digest(&content));
        if actual_hash != expected_hash {
            return Err(format!(
                "Checksum mismatch for queue artifact {:?}: expected {}, got {}",
                abs_path, expected_hash, actual_hash
            ));
        }
    }

    let expected_names_set: BTreeSet<String> =
        expected_filenames.iter().map(|s| s.to_string()).collect();
    if manifest_filenames != expected_names_set {
        return Err(
            "Manifest file set does not match expected exact queue artifact set".to_string(),
        );
    }

    println!("=== Kurmancî Review Decision Validator & Merger ===");
    println!("  Source ID: {}", source_id);

    // Read and hash decisions file
    let decisions_bytes = fs::read(&decisions_file_path).map_err(|e| {
        format!(
            "Failed to read decisions file {:?}: {}",
            decisions_file_path, e
        )
    })?;
    let decision_file_sha256 = format!("{:x}", Sha256::digest(&decisions_bytes));

    let mut decisions: Vec<ReviewDecisionRecord> = Vec::new();
    let mut seen_targets: BTreeSet<(String, String)> = BTreeSet::new();

    let reader = BufReader::new(&decisions_bytes[..]);
    for (line_idx, line_res) in reader.lines().enumerate() {
        let line_num = line_idx + 1;
        let line =
            line_res.map_err(|e| format!("Error reading decisions line {}: {}", line_num, e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let record: ReviewDecisionRecord = serde_json::from_str(trimmed)
            .map_err(|e| format!("JSON parse error in decisions line {}: {}", line_num, e))?;

        validate_decision_record(&record)
            .map_err(|e| format!("Validation error in decisions line {}: {}", line_num, e))?;

        let target_type_str = match record.target_type {
            ReviewTargetType::Entry => "entry",
            ReviewTargetType::ConflictGroup => "conflict_group",
        };
        let target_key = (target_type_str.to_string(), record.target_id.clone());

        if !seen_targets.insert(target_key.clone()) {
            return Err(format!(
                "Duplicate decision target tuple ({:?}, '{}') at line {}",
                target_key.0, target_key.1, line_num
            ));
        }

        decisions.push(record);
    }

    // Load all valid target IDs & entry records from mechanical review queues
    let mut valid_queue_targets: BTreeSet<(String, String)> = BTreeSet::new();
    let mut valid_entries_map: BTreeMap<String, EntryQueueRecord> = BTreeMap::new();

    for fname in &expected_filenames {
        if fname.ends_with(".jsonl") {
            let path = queues_dir.join(fname);
            let f = File::open(&path)
                .map_err(|e| format!("Failed to open queue file {:?}: {}", path, e))?;
            for (l_idx, line_res) in BufReader::new(f).lines().enumerate() {
                let line = line_res.map_err(|e| {
                    format!(
                        "Read error in queue file {:?} at line {}: {}",
                        path,
                        l_idx + 1,
                        e
                    )
                })?;
                if line.trim().is_empty() {
                    continue;
                }
                let val: serde_json::Value = serde_json::from_str(&line).map_err(|e| {
                    format!(
                        "JSON parse error in queue file {:?} at line {}: {}",
                        path,
                        l_idx + 1,
                        e
                    )
                })?;
                let t_type = val
                    .get("target_type")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| {
                        format!(
                            "Missing target_type in queue file {:?} at line {}",
                            path,
                            l_idx + 1
                        )
                    })?;
                let t_id = val
                    .get("target_id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| {
                        format!(
                            "Missing target_id in queue file {:?} at line {}",
                            path,
                            l_idx + 1
                        )
                    })?;

                let target_id_str = t_id.to_string();
                let target_key = (t_type.to_string(), target_id_str.clone());

                if t_type == "entry" {
                    let entry_rec: EntryQueueRecord = serde_json::from_value(val).map_err(|e| {
                        format!(
                            "Invalid entry queue record in {:?} at line {}: {}",
                            path,
                            l_idx + 1,
                            e
                        )
                    })?;
                    valid_entries_map.insert(target_id_str, entry_rec);
                } else if t_type == "conflict_group" {
                    let _cg_rec: MetadataConflictGroupQueueRecord = serde_json::from_value(val)
                        .map_err(|e| {
                            format!(
                                "Invalid conflict group queue record in {:?} at line {}: {}",
                                path,
                                l_idx + 1,
                                e
                            )
                        })?;
                } else {
                    return Err(format!(
                        "Unknown target_type '{}' in queue file {:?} at line {}",
                        t_type,
                        path,
                        l_idx + 1
                    ));
                }

                valid_queue_targets.insert(target_key);
            }
        }
    }

    if valid_queue_targets.is_empty() {
        return Err(format!(
            "No valid queue targets loaded from review queues in {:?}. Re-run generate-review-queues.",
            queues_dir
        ));
    }

    // Categorize decisions & perform target-aware validation for metadata changes
    let mut approved_records = Vec::new();
    let mut metadata_change_records = Vec::new();
    let mut rejected_records = Vec::new();
    let mut experimental_records = Vec::new();
    let mut unresolved_records = Vec::new();
    let mut orphan_records = Vec::new();

    for dec in decisions {
        let target_type_str = match dec.target_type {
            ReviewTargetType::Entry => "entry",
            ReviewTargetType::ConflictGroup => "conflict_group",
        };
        let target_key = (target_type_str.to_string(), dec.target_id.clone());

        if !valid_queue_targets.contains(&target_key) {
            orphan_records.push(dec);
            continue; // Isolate orphan decisions so they do NOT affect valid status counts!
        }

        if dec.review_status == ReviewDecisionStatus::ApprovedWithMetadataChange {
            if dec.target_type != ReviewTargetType::Entry {
                return Err(format!(
                    "ApprovedWithMetadataChange decision targeting '{}' must target an 'entry', not 'conflict_group'",
                    dec.target_id
                ));
            }

            let target_entry = valid_entries_map.get(&dec.target_id).ok_or_else(|| {
                format!(
                    "Target entry record '{}' not found in queue entries",
                    dec.target_id
                )
            })?;

            let repl = dec.replacement_metadata.as_ref().ok_or_else(|| {
                format!(
                    "ApprovedWithMetadataChange decision targeting '{}' missing replacement_metadata",
                    dec.target_id
                )
            })?;

            // Perform target-aware check: verify at least ONE replacement field actually differs from the target entry!
            let display_changed = repl.display != target_entry.display;
            let flags_changed = repl
                .flags
                .as_ref()
                .map(|f| f != &target_entry.flags)
                .unwrap_or(false);
            let morph_changed = repl
                .morphology
                .as_ref()
                .map(|m| m != &target_entry.morphology)
                .unwrap_or(false);
            let pos_changed = repl
                .part_of_speech
                .as_ref()
                .map(|p| Some(p) != target_entry.part_of_speech.as_ref())
                .unwrap_or(false);

            if !display_changed && !flags_changed && !morph_changed && !pos_changed {
                return Err(format!(
                    "ApprovedWithMetadataChange decision targeting '{}' has replacement metadata identical to the original target entry",
                    dec.target_id
                ));
            }
        }

        match dec.review_status {
            ReviewDecisionStatus::Approved => approved_records.push(dec),
            ReviewDecisionStatus::ApprovedWithMetadataChange => metadata_change_records.push(dec),
            ReviewDecisionStatus::RejectedFromDefaultPack => rejected_records.push(dec),
            ReviewDecisionStatus::ExperimentalOnly => experimental_records.push(dec),
            ReviewDecisionStatus::Unreviewed
            | ReviewDecisionStatus::NeedsLinguist
            | ReviewDecisionStatus::NeedsSourceInvestigation => unresolved_records.push(dec),
        }
    }

    let registry_path = root.join("data/source-registry/sources.toml");
    let registry = crate::sources::SourceRegistry::load_from_file(&registry_path)?;
    let src_entry = registry
        .sources
        .iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| format!("Source '{}' not found in sources.toml", source_id))?;
    let source_revision = src_entry.version.clone();

    let queue_manifest_path = queues_dir.join("artifacts.sha256");
    let queue_manifest_bytes = fs::read(&queue_manifest_path).map_err(|e| e.to_string())?;
    let queue_manifest_sha256 = format!("{:x}", Sha256::digest(&queue_manifest_bytes));

    let imported_file_path = root.join(format!("data/imported/{}/lexicon.jsonl", source_id));
    let imported_file_bytes = fs::read(&imported_file_path).map_err(|e| e.to_string())?;
    let imported_lexicon_sha256 = format!("{:x}", Sha256::digest(&imported_file_bytes));

    let summary = ReviewMergerSummary {
        schema_version: CONTROLLED_REVIEW_REPORT_SCHEMA_VERSION.to_string(),
        source_id: source_id.to_string(),
        total_decisions_count: approved_records.len()
            + metadata_change_records.len()
            + rejected_records.len()
            + experimental_records.len()
            + unresolved_records.len(),
        approved_count: approved_records.len(),
        approved_with_metadata_change_count: metadata_change_records.len(),
        rejected_from_default_count: rejected_records.len(),
        experimental_only_count: experimental_records.len(),
        unresolved_count: unresolved_records.len(),
        orphan_decisions_count: orphan_records.len(),
        decision_file_sha256: decision_file_sha256.clone(),
        provenance: ReviewReportProvenance {
            decisions_sha256: decision_file_sha256,
            queue_manifest_sha256,
            source_revision,
            imported_lexicon_sha256,
        },
    };

    let reports_dir = root.join("data/reports/controlled-lexicon-review");
    let stage_reports_dir = root.join("data/reports/controlled-lexicon-review.tmp_stage");
    let backup_reports_dir = root.join("data/reports/controlled-lexicon-review.tmp_backup");

    if stage_reports_dir.exists() {
        remove_dir_or_file(&stage_reports_dir)
            .map_err(|e| format!("Failed to clean stage reports dir: {}", e))?;
    }
    fs::create_dir_all(&stage_reports_dir)
        .map_err(|e| format!("Failed to create stage reports dir: {}", e))?;

    let write_records = |name: &str, mut items: Vec<ReviewDecisionRecord>| -> Result<(), String> {
        items.sort_by(|a, b| a.target_id.cmp(&b.target_id));
        let rpath = stage_reports_dir.join(name);
        let mut w =
            File::create(&rpath).map_err(|e| format!("Create {:?} failed: {}", rpath, e))?;
        for item in items {
            let json = serde_json::to_string(&item).map_err(|e| e.to_string())?;
            writeln!(w, "{}", json).map_err(|e| format!("Write {:?} failed: {}", rpath, e))?;
        }
        Ok(())
    };

    write_records("approved.jsonl", approved_records)?;
    write_records("metadata-changes.jsonl", metadata_change_records)?;
    write_records("rejected-from-default.jsonl", rejected_records)?;
    write_records("experimental-only.jsonl", experimental_records)?;
    write_records("unresolved.jsonl", unresolved_records)?;
    write_records("orphan-decisions.jsonl", orphan_records)?;

    fs::write(
        stage_reports_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Write summary.json failed: {}", e))?;

    let readme = format!(
        "# Controlled Lexicon Review Audit Report\n\n\
        - **Schema Version**: {}\n\
        - **Source ID**: {}\n\
        - **Decisions File SHA-256**: {}\n\
        - **Total Decisions**: {}\n\
        - **Approved**: {}\n\
        - **Approved with Metadata Change**: {}\n\
        - **Rejected from Default**: {}\n\
        - **Experimental Only**: {}\n\
        - **Unresolved**: {}\n\
        - **Orphan Decisions**: {}\n",
        CONTROLLED_REVIEW_REPORT_SCHEMA_VERSION,
        source_id,
        summary.decision_file_sha256,
        summary.total_decisions_count,
        summary.approved_count,
        summary.approved_with_metadata_change_count,
        summary.rejected_from_default_count,
        summary.experimental_only_count,
        summary.unresolved_count,
        summary.orphan_decisions_count
    );
    fs::write(stage_reports_dir.join("README.md"), readme)
        .map_err(|e| format!("Write README.md failed: {}", e))?;

    let report_files = [
        "approved.jsonl",
        "metadata-changes.jsonl",
        "rejected-from-default.jsonl",
        "experimental-only.jsonl",
        "unresolved.jsonl",
        "orphan-decisions.jsonl",
        "summary.json",
        "README.md",
    ];

    let mut manifest_entries = Vec::new();
    for name in &report_files {
        let fpath = stage_reports_dir.join(name);
        let content =
            fs::read(&fpath).map_err(|e| format!("Read report file {:?} failed: {}", fpath, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        let rel_path = format!("data/reports/controlled-lexicon-review/{}", name);
        manifest_entries.push(format!("{} {}", hash, rel_path));
    }
    manifest_entries.sort();
    let manifest_bytes = manifest_entries.join("\n") + "\n";
    fs::write(stage_reports_dir.join("artifacts.sha256"), manifest_bytes)
        .map_err(|e| format!("Write artifacts.sha256 failed: {}", e))?;

    // Atomic Staged Swap
    if backup_reports_dir.exists() {
        remove_dir_or_file(&backup_reports_dir)
            .map_err(|e| format!("Failed to clean backup reports dir: {}", e))?;
    }
    if reports_dir.exists() {
        fs::rename(&reports_dir, &backup_reports_dir)
            .map_err(|e| format!("Failed to rename reports dir to backup: {}", e))?;
    }

    match fs::rename(&stage_reports_dir, &reports_dir) {
        Ok(()) => {
            if backup_reports_dir.exists() {
                if let Err(e) = remove_dir_or_file(&backup_reports_dir) {
                    eprintln!(
                        "Warning: failed to clean up backup dir {:?}: {}",
                        backup_reports_dir, e
                    );
                }
            }
        }
        Err(err) => {
            if backup_reports_dir.exists() {
                if let Err(rollback_err) = fs::rename(&backup_reports_dir, &reports_dir) {
                    return Err(format!(
                        "Failed to install reports dir {:?}: {}; rollback also failed: {}",
                        reports_dir, err, rollback_err
                    ));
                }
            }
            return Err(format!(
                "Failed to install reports dir {:?}: {}",
                reports_dir, err
            ));
        }
    }

    println!("⚡ REVIEW DECISIONS VALIDATED SUCCESSFULLY! Reports written to data/reports/controlled-lexicon-review/");
    lock.release()?;
    Ok(summary)
}
