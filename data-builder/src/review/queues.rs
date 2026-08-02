//! Mechanical review queue generator (`review-queue-v1`).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::audit::classify::{classify_script, classify_shape, ScriptClass};
use crate::corpus::importer::LockFileGuard;
use crate::importers::hunspell::{parse_hunspell_source, HunspellSourceEvent};
use crate::review::schema::{compute_conflict_group_id, compute_entry_id};
use crate::sources::SourceRegistry;
use unicode_segmentation::UnicodeSegmentation;

pub const REVIEW_QUEUE_SCHEMA_VERSION: &str = "review-queue-v1";

/// Maximum number of entries containing a character to consider it rare under versioned rule `RARE_CODE_POINT_V1`.
pub const RARE_CODE_POINT_MAX_ENTRIES: usize = 5;

/// Maximum grapheme count for a short form under versioned rule `SHORT_AND_LONG_FORM_V1`.
pub const SHORT_FORM_MAX_GRAPHEMES: usize = 1;

/// Minimum grapheme count for a long form under versioned rule `SHORT_AND_LONG_FORM_V1`.
pub const LONG_FORM_MIN_GRAPHEMES: usize = 25;

/// Validates that a source revision string is a full 40-character hexadecimal commit SHA.
pub fn validate_commit_sha(sha: &str) -> Result<(), String> {
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "Source revision '{}' is not a valid 40-character hexadecimal commit SHA",
            sha
        ));
    }
    Ok(())
}

/// Summary report emitted when review queues are generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewQueueSummary {
    pub source_id: String,
    pub source_revision: String,
    pub total_imported_records: usize,
    pub parser_rejections_count: usize,
    pub metadata_conflict_groups_count: usize,
    pub suspicious_entries_count: usize,
    pub unusual_scripts_count: usize,
    pub mixed_scripts_count: usize,
    pub non_letter_tokens_count: usize,
    pub rare_code_points_count: usize,
    pub unexpected_code_points_count: usize,
    pub short_and_long_forms_count: usize,
    pub capitalization_anomalies_count: usize,
    pub multiword_entries_count: usize,
    pub hunspell_only_entries_count: usize,
}

/// Member record evidence inside a metadata conflict group queue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictGroupMemberEvidence {
    pub entry_id: String,
    pub display: String,
    pub source_lines: Vec<usize>,
    pub flags: String,
    pub morphology: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_of_speech: Option<String>,
}

/// Conflict group queue record schema (`METADATA_CONFLICT_V1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataConflictGroupQueueRecord {
    pub schema_version: String,
    pub rule_id: String,
    pub rule_version: String,
    pub target_type: String,
    pub target_id: String,
    pub normalized: String,
    pub member_entry_ids: Vec<String>,
    pub members: Vec<ConflictGroupMemberEvidence>,
    pub differing_fields: Vec<String>,
    pub reason_codes: Vec<String>,
    pub suggested_action: String,
    pub generated_status: String,
    pub effective_review_status: String,
    pub decision_entry_id: Option<String>,
    pub queue_categories: Vec<String>,
}

/// Standard entry review queue record schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryQueueRecord {
    pub schema_version: String,
    pub rule_id: String,
    pub rule_version: String,
    pub target_type: String,
    pub target_id: String,
    pub display: String,
    pub normalized: String,
    pub source_id: String,
    pub source_revision: String,
    pub source_lines: Vec<usize>,
    pub flags: String,
    pub morphology: Vec<String>,
    #[serde(default)]
    pub part_of_speech: Option<String>,
    pub reason_codes: Vec<String>,
    pub suggested_action: String,
    pub generated_status: String,
    pub effective_review_status: String,
    pub decision_entry_id: Option<String>,
    pub queue_categories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportedLexiconItem {
    pub word: String,
    pub normalized: String,
    #[serde(default)]
    pub flags: String,
    #[serde(default)]
    pub morphology: Vec<String>,
    #[serde(default)]
    pub part_of_speech: Option<String>,
    pub source_line_num: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct RejectionReportItem {
    pub raw_line: String,
    #[serde(default)]
    pub source_line_num: Option<usize>,
    #[serde(default)]
    pub line_num: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConflictReportItem {
    pub normalized: String,
    pub word_a: String,
    pub line_a: usize,
    pub flags_a: String,
    pub word_b: String,
    pub line_b: usize,
    pub flags_b: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SuspiciousEntryItem {
    pub word: String,
    pub normalized: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CharacterInventoryEntry {
    pub character: String,
    #[serde(default)]
    pub entries_containing_in_normalized: usize,
    #[serde(default)]
    pub occurrences_in_normalized: usize,
    #[serde(default)]
    pub script: String,
}

#[derive(Debug, Clone, Deserialize)]
struct QualityAuditSeedComparisonReport {
    #[serde(default)]
    pub seed_words: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AuditSummaryInput {
    pub suspicious_entries_count: usize,
    pub importer_cross_check: AuditCrossCheckInput,
    pub shape_analysis_brief: AuditShapeBriefInput,
}

#[derive(Debug, Clone, Deserialize)]
struct AuditCrossCheckInput {
    pub audit_metadata_conflict_groups: usize,
    pub importer_rejected_entries: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct AuditShapeBriefInput {
    #[serde(default)]
    pub punctuation_only: usize,
    #[serde(default)]
    pub symbol_only: usize,
    #[serde(default)]
    pub digit_only: usize,
    #[serde(default)]
    pub no_letters: usize,
}

#[derive(Debug, Clone)]
struct RawSourceRecord {
    pub word: String,
    pub normalized: String,
    pub flags: String,
    pub morphology: Vec<String>,
    pub part_of_speech: Option<String>,
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

/// Generates mechanical review queues for a source ID under `data/review-queues/<source-id>/`.
pub fn generate_review_queues<P: AsRef<Path>>(
    source_id: &str,
    root_dir: P,
) -> Result<ReviewQueueSummary, String> {
    let root = root_dir.as_ref();
    let lock_path = root.join("data/review-queues.lock");
    let lock = LockFileGuard::acquire(&lock_path)?;

    // Load source registry to get immutable source revision
    let registry_path = root.join("data/source-registry/sources.toml");
    let registry = SourceRegistry::load_from_file(&registry_path)?;
    let source_entry = registry
        .sources
        .iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| format!("Source ID '{}' not found in source registry", source_id))?;

    let source_revision = source_entry.version.clone();
    validate_commit_sha(&source_revision)?;

    // Locate raw source file and parse all physical source lines for 100% complete evidence
    let dic_file_meta = source_entry
        .files
        .iter()
        .find(|f| f.path.ends_with(".dic"))
        .ok_or_else(|| format!("No .dic file registered for source '{}'", source_id))?;
    let mut raw_dic_path = root.join(&dic_file_meta.path);
    if !raw_dic_path.exists() {
        raw_dic_path = root
            .join("data/raw")
            .join(&source_entry.source_id)
            .join(&dic_file_meta.path);
    }
    if !raw_dic_path.exists() {
        return Err(format!(
            "Raw source .dic file missing at {:?}",
            raw_dic_path
        ));
    }

    let raw_file = File::open(&raw_dic_path)
        .map_err(|e| format!("Failed to open raw .dic file {:?}: {}", raw_dic_path, e))?;
    let source_events = parse_hunspell_source(BufReader::new(raw_file));

    let mut source_records_by_line: BTreeMap<usize, RawSourceRecord> = BTreeMap::new();
    for ev in source_events {
        if let HunspellSourceEvent::Parsed {
            source_line_num,
            entry,
            normalized,
            part_of_speech,
            ..
        } = ev
        {
            let flags_str = entry.flags.clone();
            let pos_opt = if part_of_speech.trim().is_empty() {
                None
            } else {
                Some(part_of_speech)
            };
            source_records_by_line.insert(
                source_line_num,
                RawSourceRecord {
                    word: entry.raw_word,
                    normalized,
                    flags: flags_str,
                    morphology: entry.morphology,
                    part_of_speech: pos_opt,
                },
            );
        }
    }

    // Check imported lexicon
    let imported_file_path = root.join(format!("data/imported/{}/lexicon.jsonl", source_id));
    if !imported_file_path.exists() {
        return Err(format!(
            "Imported lexicon missing at {:?}. Run import-hunspell first.",
            imported_file_path
        ));
    }

    // Check quality audit inputs
    let audit_dir = root.join(format!("data/reports/{}/quality-audit", source_id));
    if !audit_dir.exists() {
        return Err(format!(
            "Quality audit report directory missing at {:?}. Run audit-lexicon first.",
            audit_dir
        ));
    }

    // Read audit summary for mandatory count cross-checks
    let audit_summary_path = audit_dir.join("summary.json");
    if !audit_summary_path.exists() {
        return Err(format!(
            "Required audit summary missing at {:?}",
            audit_summary_path
        ));
    }
    let audit_summary_content = fs::read_to_string(&audit_summary_path)
        .map_err(|e| format!("Failed to read {:?}: {}", audit_summary_path, e))?;
    let audit_summary_input: AuditSummaryInput = serde_json::from_str(&audit_summary_content)
        .map_err(|e| {
            format!(
                "Failed to parse audit summary {:?}: {}",
                audit_summary_path, e
            )
        })?;

    println!("=== Kurmancî Review Queue Generator ===");
    println!("  Source ID:       {}", source_id);
    println!("  Source Revision: {}", source_revision);

    // Read imported records
    let file = File::open(&imported_file_path).map_err(|e| {
        format!(
            "Failed to open imported lexicon {:?}: {}",
            imported_file_path, e
        )
    })?;
    let reader = BufReader::new(file);

    let mut imported_records: Vec<ImportedLexiconItem> = Vec::new();
    let mut seen_entry_ids: BTreeSet<String> = BTreeSet::new();

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line_num = line_idx + 1;
        let line = line_res
            .map_err(|e| format!("Error reading imported lexicon line {}: {}", line_num, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let item: ImportedLexiconItem = serde_json::from_str(&line).map_err(|e| {
            format!(
                "JSON parse error in imported lexicon {:?} line {}: {}",
                imported_file_path, line_num, e
            )
        })?;

        let entry_id = compute_entry_id(
            source_id,
            &source_revision,
            &item.word,
            &item.normalized,
            &item.flags,
            &item.morphology,
        )?;

        if !seen_entry_ids.insert(entry_id.clone()) {
            return Err(format!(
                "Fatal consistency error: duplicate entry_id '{}' generated for record '{}'",
                entry_id, item.word
            ));
        }

        imported_records.push(item);
    }

    // Load quality audit classification artifacts using typed schemas and strict line checks
    let suspicious_path = audit_dir.join("suspicious-entries.jsonl");
    let mut suspicious_set: BTreeSet<(String, String)> = BTreeSet::new();
    if suspicious_path.exists() {
        let f = File::open(&suspicious_path).map_err(|e| e.to_string())?;
        for (line_idx, line_res) in BufReader::new(f).lines().enumerate() {
            let line = line_res.map_err(|e| {
                format!(
                    "Read error in suspicious-entries line {}: {}",
                    line_idx + 1,
                    e
                )
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let item: SuspiciousEntryItem = serde_json::from_str(&line).map_err(|e| {
                format!(
                    "JSON error in suspicious-entries line {}: {}",
                    line_idx + 1,
                    e
                )
            })?;
            suspicious_set.insert((item.word, item.normalized));
        }
    }

    let seed_comp_path = audit_dir.join("manual-seed-comparison.json");
    if !seed_comp_path.exists() {
        return Err(format!(
            "Required audit artifact missing at {:?}",
            seed_comp_path
        ));
    }
    let seed_comp_content = fs::read_to_string(&seed_comp_path)
        .map_err(|e| format!("Failed to read {:?}: {}", seed_comp_path, e))?;
    let r_seed: QualityAuditSeedComparisonReport = serde_json::from_str(&seed_comp_content)
        .map_err(|e| format!("Failed to parse {:?}: {}", seed_comp_path, e))?;
    let seed_words: BTreeSet<String> = r_seed.seed_words.into_iter().collect();

    // Load character inventory report with strict error handling
    let char_inv_path = audit_dir.join("character-inventory.json");
    if !char_inv_path.exists() {
        return Err(format!(
            "Required audit artifact missing at {:?}",
            char_inv_path
        ));
    }
    let char_inv_content = fs::read_to_string(&char_inv_path)
        .map_err(|e| format!("Failed to read {:?}: {}", char_inv_path, e))?;
    let char_entries: Vec<CharacterInventoryEntry> = serde_json::from_str(&char_inv_content)
        .map_err(|e| {
            format!(
                "Failed to parse character inventory {:?}: {}",
                char_inv_path, e
            )
        })?;

    let mut rare_chars_set: BTreeSet<char> = BTreeSet::new();
    let mut unexpected_chars_set: BTreeSet<char> = BTreeSet::new();
    for entry in char_entries {
        if let Some(c) = entry.character.chars().next() {
            if entry.entries_containing_in_normalized < RARE_CODE_POINT_MAX_ENTRIES
                && entry.occurrences_in_normalized > 0
            {
                rare_chars_set.insert(c);
            }
            if entry.script != "Latin" && entry.script != "Common" && entry.script != "Inherited" {
                unexpected_chars_set.insert(c);
            }
        }
    }

    let stage_queues_dir = root.join(format!("data/review-queues/{}.tmp_stage", source_id));
    let queues_dir = root.join(format!("data/review-queues/{}", source_id));
    let backup_queues_dir = root.join(format!("data/review-queues/{}.tmp_backup", source_id));

    if stage_queues_dir.exists() {
        remove_dir_or_file(&stage_queues_dir)
            .map_err(|e| format!("Failed to clean stage queues dir: {}", e))?;
    }
    fs::create_dir_all(&stage_queues_dir)
        .map_err(|e| format!("Failed to create stage queues dir: {}", e))?;

    // Build queues
    let mut conflict_group_records: Vec<MetadataConflictGroupQueueRecord> = Vec::new();
    let mut parser_rejection_records: Vec<EntryQueueRecord> = Vec::new();
    let mut suspicious_records: Vec<EntryQueueRecord> = Vec::new();
    let mut unusual_script_records: Vec<EntryQueueRecord> = Vec::new();
    let mut mixed_script_records: Vec<EntryQueueRecord> = Vec::new();
    let mut punct_records: Vec<EntryQueueRecord> = Vec::new();
    let mut symbol_records: Vec<EntryQueueRecord> = Vec::new();
    let mut digit_records: Vec<EntryQueueRecord> = Vec::new();
    let mut no_letter_records: Vec<EntryQueueRecord> = Vec::new();
    let mut rare_code_point_records: Vec<EntryQueueRecord> = Vec::new();
    let mut unexpected_code_point_records: Vec<EntryQueueRecord> = Vec::new();
    let mut short_and_long_records: Vec<EntryQueueRecord> = Vec::new();
    let mut cap_anomaly_records: Vec<EntryQueueRecord> = Vec::new();
    let mut multiword_records: Vec<EntryQueueRecord> = Vec::new();
    let mut hunspell_only_records: Vec<EntryQueueRecord> = Vec::new();

    let mut seen_group_ids: BTreeSet<String> = BTreeSet::new();

    // 1. Metadata Conflict Groups Queue (reconstructed with 100% complete source evidence)
    let conflicts_report_path = root.join(format!("data/reports/{}/conflicts.jsonl", source_id));
    if conflicts_report_path.exists() {
        let f = File::open(&conflicts_report_path).map_err(|e| e.to_string())?;
        let mut conflicts_by_norm: BTreeMap<String, BTreeSet<(String, usize, String)>> =
            BTreeMap::new();

        for (line_idx, line_res) in BufReader::new(f).lines().enumerate() {
            let line = line_res.map_err(|e| {
                format!("Read error in conflicts.jsonl line {}: {}", line_idx + 1, e)
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let item: ConflictReportItem = serde_json::from_str(&line).map_err(|e| {
                format!("JSON error in conflicts.jsonl line {}: {}", line_idx + 1, e)
            })?;

            let set = conflicts_by_norm.entry(item.normalized).or_default();
            set.insert((item.word_a, item.line_a, item.flags_a));
            set.insert((item.word_b, item.line_b, item.flags_b));
        }

        for (normalized, member_tuples) in &conflicts_by_norm {
            let mut members = Vec::new();
            let mut member_ids = Vec::new();
            let mut flags_set = BTreeSet::new();
            let mut morph_set: BTreeSet<Vec<String>> = BTreeSet::new();
            let mut pos_set = BTreeSet::new();

            for (_word, line_num, _flags) in member_tuples {
                let src_rec = source_records_by_line.get(line_num).ok_or_else(|| {
                    format!(
                        "Fatal error: conflict group member at line {} not found in source file",
                        line_num
                    )
                })?;

                let entry_id = compute_entry_id(
                    source_id,
                    &source_revision,
                    &src_rec.word,
                    &src_rec.normalized,
                    &src_rec.flags,
                    &src_rec.morphology,
                )?;

                member_ids.push(entry_id.clone());
                flags_set.insert(src_rec.flags.clone());
                morph_set.insert(src_rec.morphology.clone());
                if let Some(ref pos) = src_rec.part_of_speech {
                    pos_set.insert(pos.clone());
                }

                members.push(ConflictGroupMemberEvidence {
                    entry_id,
                    display: src_rec.word.clone(),
                    source_lines: vec![*line_num],
                    flags: src_rec.flags.clone(),
                    morphology: src_rec.morphology.clone(),
                    part_of_speech: src_rec.part_of_speech.clone(),
                });
            }

            member_ids.sort();
            members.sort_by(|a, b| a.entry_id.cmp(&b.entry_id));

            let group_id = compute_conflict_group_id(normalized, &member_ids)?;
            if !seen_group_ids.insert(group_id.clone()) {
                continue;
            }

            let mut differing = Vec::new();
            if flags_set.len() > 1 {
                differing.push("flags".to_string());
            }
            if morph_set.len() > 1 {
                differing.push("morphology".to_string());
            }
            if pos_set.len() > 1 {
                differing.push("part_of_speech".to_string());
            }

            conflict_group_records.push(MetadataConflictGroupQueueRecord {
                schema_version: REVIEW_QUEUE_SCHEMA_VERSION.to_string(),
                rule_id: "METADATA_CONFLICT_V1".to_string(),
                rule_version: "1".to_string(),
                target_type: "conflict_group".to_string(),
                target_id: group_id,
                normalized: normalized.to_string(),
                member_entry_ids: member_ids,
                members,
                differing_fields: differing,
                reason_codes: vec!["METADATA_CONFLICT".to_string()],
                suggested_action: "manual_review".to_string(),
                generated_status: "unreviewed".to_string(),
                effective_review_status: "unreviewed".to_string(),
                decision_entry_id: None,
                queue_categories: vec!["metadata-conflict-groups".to_string()],
            });
        }
    }

    // 2. Standard Queues for Imported Records
    for item in &imported_records {
        let entry_id = compute_entry_id(
            source_id,
            &source_revision,
            &item.word,
            &item.normalized,
            &item.flags,
            &item.morphology,
        )?;

        let make_record =
            |rule_id: &str, reason: &str, action: &str, cat: &str| -> EntryQueueRecord {
                EntryQueueRecord {
                    schema_version: REVIEW_QUEUE_SCHEMA_VERSION.to_string(),
                    rule_id: rule_id.to_string(),
                    rule_version: "1".to_string(),
                    target_type: "entry".to_string(),
                    target_id: entry_id.clone(),
                    display: item.word.clone(),
                    normalized: item.normalized.clone(),
                    source_id: source_id.to_string(),
                    source_revision: source_revision.clone(),
                    source_lines: vec![item.source_line_num],
                    flags: item.flags.clone(),
                    morphology: item.morphology.clone(),
                    part_of_speech: item.part_of_speech.clone(),
                    reason_codes: vec![reason.to_string()],
                    suggested_action: action.to_string(),
                    generated_status: "unreviewed".to_string(),
                    effective_review_status: "unreviewed".to_string(),
                    decision_entry_id: None,
                    queue_categories: vec![cat.to_string()],
                }
            };

        if suspicious_set.contains(&(item.word.clone(), item.normalized.clone())) {
            suspicious_records.push(make_record(
                "SUSPICIOUS_ENTRY_V1",
                "SUSPICIOUS_ENTRY",
                "manual_review",
                "suspicious-entries",
            ));
        }

        let script_class = classify_script(&item.word);
        match script_class {
            ScriptClass::ArabicOnly
            | ScriptClass::CyrillicOnly
            | ScriptClass::OtherSingleScript(_) => {
                unusual_script_records.push(make_record(
                    "UNUSUAL_SCRIPT_V1",
                    "UNUSUAL_SCRIPT",
                    "manual_review",
                    "unusual-scripts",
                ));
            }
            ScriptClass::LatinArabicMixed
            | ScriptClass::LatinCyrillicMixed
            | ScriptClass::OtherMixedScript => {
                mixed_script_records.push(make_record(
                    "MIXED_SCRIPT_V1",
                    "MIXED_SCRIPT",
                    "manual_review",
                    "mixed-scripts",
                ));
            }
            _ => {}
        }

        let shape = classify_shape(&item.word, &item.normalized);
        if shape.is_punctuation_only {
            punct_records.push(make_record(
                "PUNCTUATION_ONLY_V1",
                "PUNCTUATION_ONLY",
                "manual_review",
                "punctuation-only",
            ));
        }
        if shape.is_symbol_only {
            symbol_records.push(make_record(
                "SYMBOL_ONLY_V1",
                "SYMBOL_ONLY",
                "manual_review",
                "symbol-only",
            ));
        }
        if shape.is_digit_only {
            digit_records.push(make_record(
                "DIGIT_ONLY_V1",
                "DIGIT_ONLY",
                "manual_review",
                "digit-only",
            ));
        }
        if shape.has_no_letters
            && !shape.is_punctuation_only
            && !shape.is_symbol_only
            && !shape.is_digit_only
        {
            no_letter_records.push(make_record(
                "NO_LETTER_V1",
                "NO_LETTER",
                "manual_review",
                "no-letter",
            ));
        }
        if shape.is_uppercase_only || shape.is_mixed_case {
            cap_anomaly_records.push(make_record(
                "CAPITALIZATION_ANOMALY_V1",
                "CAPITALIZATION_ANOMALY",
                "manual_review",
                "capitalization-anomalies",
            ));
        }
        if shape.is_multiword {
            multiword_records.push(make_record(
                "MULTIWORD_ENTRY_V1",
                "MULTIWORD_ENTRY",
                "manual_review",
                "multiword-entries",
            ));
        }

        if item.normalized.chars().any(|c| rare_chars_set.contains(&c)) {
            rare_code_point_records.push(make_record(
                "RARE_CODE_POINT_V1",
                "RARE_CODE_POINT",
                "manual_review",
                "rare-code-points",
            ));
        }
        if item
            .normalized
            .chars()
            .any(|c| unexpected_chars_set.contains(&c))
        {
            unexpected_code_point_records.push(make_record(
                "UNEXPECTED_CODE_POINT_V1",
                "UNEXPECTED_CODE_POINT",
                "manual_review",
                "unexpected-code-points",
            ));
        }

        let grapheme_len = item.normalized.graphemes(true).count();
        if grapheme_len <= SHORT_FORM_MAX_GRAPHEMES || grapheme_len >= LONG_FORM_MIN_GRAPHEMES {
            short_and_long_records.push(make_record(
                "SHORT_AND_LONG_FORM_V1",
                "SHORT_OR_LONG_FORM",
                "manual_review",
                "short-and-long-forms",
            ));
        }

        if !seed_words.contains(&item.normalized) {
            hunspell_only_records.push(make_record(
                "HUNSPELL_ONLY_V1",
                "HUNSPELL_ONLY",
                "retain",
                "hunspell-only",
            ));
        }
    }

    // 3. Parser Rejections Queue (strictly requiring source_line_num from rejection record)
    let rejected_path = root.join(format!("data/reports/{}/rejected.jsonl", source_id));
    if rejected_path.exists() {
        let f = File::open(&rejected_path).map_err(|e| e.to_string())?;
        for (idx, line_res) in BufReader::new(f).lines().enumerate() {
            let line = line_res
                .map_err(|e| format!("Read error in rejected.jsonl line {}: {}", idx + 1, e))?;
            if line.trim().is_empty() {
                continue;
            }
            let item: RejectionReportItem = serde_json::from_str(&line).map_err(|e| {
                format!("JSON parse error in rejected.jsonl line {}: {}", idx + 1, e)
            })?;

            let src_line = item.source_line_num.or(item.line_num).ok_or_else(|| {
                format!(
                    "Parser rejection record at line {} in {:?} missing required source_line_num",
                    idx + 1,
                    rejected_path
                )
            })?;

            let entry_id = compute_entry_id(
                source_id,
                &source_revision,
                &item.raw_line,
                &item.raw_line,
                "",
                &[],
            )?;
            parser_rejection_records.push(EntryQueueRecord {
                schema_version: REVIEW_QUEUE_SCHEMA_VERSION.to_string(),
                rule_id: "PARSER_REJECTION_V1".to_string(),
                rule_version: "1".to_string(),
                target_type: "entry".to_string(),
                target_id: entry_id,
                display: item.raw_line.clone(),
                normalized: item.raw_line,
                source_id: source_id.to_string(),
                source_revision: source_revision.clone(),
                source_lines: vec![src_line],
                flags: String::new(),
                morphology: Vec::new(),
                part_of_speech: None,
                reason_codes: vec!["PARSER_REJECTION".to_string()],
                suggested_action: "investigate_source_or_parser".to_string(),
                generated_status: "unreviewed".to_string(),
                effective_review_status: "unreviewed".to_string(),
                decision_entry_id: None,
                queue_categories: vec!["parser-rejections".to_string()],
            });
        }
    }

    let write_entry_queue =
        |name: &str, mut items: Vec<EntryQueueRecord>| -> Result<usize, String> {
            items.sort_by(|a, b| {
                a.normalized
                    .cmp(&b.normalized)
                    .then_with(|| a.display.cmp(&b.display))
                    .then_with(|| a.source_id.cmp(&b.source_id))
                    .then_with(|| a.source_revision.cmp(&b.source_revision))
                    .then_with(|| a.target_id.cmp(&b.target_id))
            });
            let count = items.len();
            let qpath = stage_queues_dir.join(name);
            let mut w =
                File::create(&qpath).map_err(|e| format!("Create {:?} failed: {}", qpath, e))?;
            for rec in items {
                let json = serde_json::to_string(&rec).map_err(|e| e.to_string())?;
                writeln!(w, "{}", json).map_err(|e| format!("Write {:?} failed: {}", qpath, e))?;
            }
            Ok(count)
        };

    conflict_group_records.sort_by(|a, b| {
        a.normalized
            .cmp(&b.normalized)
            .then_with(|| a.target_id.cmp(&b.target_id))
    });
    let c_groups_count = conflict_group_records.len();
    let cg_path = stage_queues_dir.join("metadata-conflict-groups.jsonl");
    let mut cg_w =
        File::create(&cg_path).map_err(|e| format!("Create {:?} failed: {}", cg_path, e))?;
    for rec in conflict_group_records {
        let json = serde_json::to_string(&rec).map_err(|e| e.to_string())?;
        writeln!(cg_w, "{}", json).map_err(|e| format!("Write {:?} failed: {}", cg_path, e))?;
    }

    let p_rej_count = write_entry_queue("parser-rejections.jsonl", parser_rejection_records)?;
    let susp_count = write_entry_queue("suspicious-entries.jsonl", suspicious_records)?;
    let unus_count = write_entry_queue("unusual-scripts.jsonl", unusual_script_records)?;
    let mix_count = write_entry_queue("mixed-scripts.jsonl", mixed_script_records)?;
    let p_count = write_entry_queue("punctuation-only.jsonl", punct_records)?;
    let s_count = write_entry_queue("symbol-only.jsonl", symbol_records)?;
    let d_count = write_entry_queue("digit-only.jsonl", digit_records)?;
    let nl_count = write_entry_queue("no-letter.jsonl", no_letter_records)?;
    let r_cp_count = write_entry_queue("rare-code-points.jsonl", rare_code_point_records)?;
    let unex_cp_count = write_entry_queue(
        "unexpected-code-points.jsonl",
        unexpected_code_point_records,
    )?;
    let sl_count = write_entry_queue("short-and-long-forms.jsonl", short_and_long_records)?;
    let cap_count = write_entry_queue("capitalization-anomalies.jsonl", cap_anomaly_records)?;
    let mw_count = write_entry_queue("multiword-entries.jsonl", multiword_records)?;
    let hun_only_count = write_entry_queue("hunspell-only.jsonl", hunspell_only_records)?;

    let summary = ReviewQueueSummary {
        source_id: source_id.to_string(),
        source_revision: source_revision.clone(),
        total_imported_records: imported_records.len(),
        parser_rejections_count: p_rej_count,
        metadata_conflict_groups_count: c_groups_count,
        suspicious_entries_count: susp_count,
        unusual_scripts_count: unus_count,
        mixed_scripts_count: mix_count,
        non_letter_tokens_count: p_count + s_count + d_count + nl_count,
        rare_code_points_count: r_cp_count,
        unexpected_code_points_count: unex_cp_count,
        short_and_long_forms_count: sl_count,
        capitalization_anomalies_count: cap_count,
        multiword_entries_count: mw_count,
        hunspell_only_entries_count: hun_only_count,
    };

    // Mandatory Audit Summary Count Cross-Check
    if summary.suspicious_entries_count != audit_summary_input.suspicious_entries_count {
        return Err(format!(
            "Quality audit count cross-check failed for 'suspicious_entries_count': queue has {}, audit has {}",
            summary.suspicious_entries_count, audit_summary_input.suspicious_entries_count
        ));
    }
    if summary.metadata_conflict_groups_count
        != audit_summary_input
            .importer_cross_check
            .audit_metadata_conflict_groups
    {
        return Err(format!(
            "Quality audit count cross-check failed for 'metadata_conflict_groups_count': queue has {}, audit has {}",
            summary.metadata_conflict_groups_count, audit_summary_input.importer_cross_check.audit_metadata_conflict_groups
        ));
    }
    if summary.parser_rejections_count
        != audit_summary_input
            .importer_cross_check
            .importer_rejected_entries
    {
        return Err(format!(
            "Quality audit count cross-check failed for 'parser_rejections_count': queue has {}, audit has {}",
            summary.parser_rejections_count, audit_summary_input.importer_cross_check.importer_rejected_entries
        ));
    }
    if summary.non_letter_tokens_count != audit_summary_input.shape_analysis_brief.no_letters {
        return Err(format!(
            "Quality audit count cross-check failed for 'non_letter_tokens_count': queue has {}, audit has {}",
            summary.non_letter_tokens_count, audit_summary_input.shape_analysis_brief.no_letters
        ));
    }

    fs::write(
        stage_queues_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Write summary.json failed: {}", e))?;

    let readme = format!(
        "# Kurmancî Review Queues — {}\n\n\
        - **Source ID**: {}\n\
        - **Source Revision**: {}\n\
        - **Total Imported Records**: {}\n\
        - **Metadata Conflict Groups**: {}\n\
        - **Suspicious Entries**: {}\n\
        - **Hunspell-Only Entries**: {}\n\n\
        > **IMPORTANT**: Never edit generated review queues in `data/review-queues/`. Only edit `decisions.jsonl`. The queue generator is allowed to replace `review-queues/`. It is never allowed to rewrite `review-decisions/`. If a queue appears incorrect, fix the importer or audit—not the generated queue.\n",
        source_id, source_id, source_revision, summary.total_imported_records, summary.metadata_conflict_groups_count, summary.suspicious_entries_count, summary.hunspell_only_entries_count
    );
    fs::write(stage_queues_dir.join("README.md"), readme)
        .map_err(|e| format!("Write README.md failed: {}", e))?;

    // Generate artifacts.sha256
    let queue_files = [
        "parser-rejections.jsonl",
        "metadata-conflict-groups.jsonl",
        "suspicious-entries.jsonl",
        "unusual-scripts.jsonl",
        "mixed-scripts.jsonl",
        "punctuation-only.jsonl",
        "symbol-only.jsonl",
        "digit-only.jsonl",
        "no-letter.jsonl",
        "rare-code-points.jsonl",
        "unexpected-code-points.jsonl",
        "short-and-long-forms.jsonl",
        "capitalization-anomalies.jsonl",
        "multiword-entries.jsonl",
        "hunspell-only.jsonl",
        "summary.json",
        "README.md",
    ];

    let mut manifest_entries = Vec::new();
    for name in &queue_files {
        let fpath = stage_queues_dir.join(name);
        let content =
            fs::read(&fpath).map_err(|e| format!("Read queue file {:?} failed: {}", fpath, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        let rel_path = format!("data/review-queues/{}/{}", source_id, name);
        manifest_entries.push(format!("{} {}", hash, rel_path));
    }
    manifest_entries.sort();
    let manifest_bytes = manifest_entries.join("\n") + "\n";
    fs::write(stage_queues_dir.join("artifacts.sha256"), manifest_bytes)
        .map_err(|e| format!("Write artifacts.sha256 failed: {}", e))?;

    // Atomic Staged Swap
    if backup_queues_dir.exists() {
        remove_dir_or_file(&backup_queues_dir)
            .map_err(|e| format!("Failed to clean backup queues dir: {}", e))?;
    }
    if queues_dir.exists() {
        fs::rename(&queues_dir, &backup_queues_dir)
            .map_err(|e| format!("Failed to rename queues dir to backup: {}", e))?;
    }

    match fs::rename(&stage_queues_dir, &queues_dir) {
        Ok(()) => {
            if backup_queues_dir.exists() {
                if let Err(e) = remove_dir_or_file(&backup_queues_dir) {
                    eprintln!(
                        "Warning: failed to clean up backup dir {:?}: {}",
                        backup_queues_dir, e
                    );
                }
            }
        }
        Err(err) => {
            if backup_queues_dir.exists() {
                if let Err(rollback_err) = fs::rename(&backup_queues_dir, &queues_dir) {
                    return Err(format!(
                        "Failed to install review queues dir {:?}: {}; rollback also failed: {}",
                        queues_dir, err, rollback_err
                    ));
                }
            }
            return Err(format!(
                "Failed to install review queues dir {:?}: {}",
                queues_dir, err
            ));
        }
    }

    println!(
        "⚡ REVIEW QUEUES GENERATED SUCCESSFULLY under data/review-queues/{}/",
        source_id
    );
    lock.release()?;
    Ok(summary)
}
