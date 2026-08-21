//! Bulk Core Vocabulary Review-Batch Generator.
//!
//! Generates a deterministic, ranked 1,000-entry human review batch from existing
//! KurdishHunspell review queues (`data/review-queues/kurdish-hunspell-kmr/hunspell-only.jsonl`)
//! joined with real corpus frequencies (`data/build/frequencies.jsonl`) and audit flags.
//!
//! Output artifacts:
//! - `data/reports/vocabulary-review/top-1000.tsv`
//! - `data/reports/vocabulary-review/top-1000.jsonl`
//! - `data/reports/vocabulary-review/summary.json`

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::review::queues::EntryQueueRecord;
use crate::review::schema::{ReviewDecisionRecord, ReviewDecisionStatus};

pub const VOCABULARY_REVIEW_SUMMARY_SCHEMA: &str = "vocabulary-review-batch-summary-v1";

/// Imported metadata retained for human review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VocabularyImportedMetadata {
    pub flags: String,
    pub morphology: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_of_speech: Option<String>,
}

/// Single record in the generated vocabulary review batch (`top-1000.jsonl`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabularyReviewRecord {
    pub rank: usize,
    pub target_id: String,
    pub source_id: String,
    pub source_revision: String,
    pub source_lines: Vec<usize>,
    pub form: String,
    pub normalized: String,
    pub imported_metadata: VocabularyImportedMetadata,
    pub token_count: u64,
    pub document_count: u64,
    pub zipf: f64,
    pub audit_flags: Vec<String>,
    pub decision_status: String,
}

/// Summary object emitted when the vocabulary review batch is generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabularyReviewBatchSummary {
    pub schema_version: String,
    pub generator: String,
    pub candidate_pool: String,
    pub total_pool_candidates: usize,
    pub excluded_existing_decisions: usize,
    pub eligible_pending_candidates: usize,
    pub batch_size: usize,
    pub clean_candidates_count: usize,
    pub corpus_matched_count: usize,
    pub output_tsv: String,
    pub output_jsonl: String,
}

/// Real corpus frequency data loaded from `data/build/frequencies.jsonl`.
#[derive(Debug, Clone, Deserialize)]
struct CorpusFrequencyItem {
    pub word: String,
    #[serde(default)]
    pub token_count: u64,
    #[serde(default)]
    pub document_count: u64,
    #[serde(default)]
    pub zipf: f64,
}

/// Minimal queue record structure for joining target_id/normalized to audit flags.
#[derive(Debug, Clone, Deserialize)]
struct GenericQueueTarget {
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub member_entry_ids: Option<Vec<String>>,
}

/// Candidate item evaluated during review-batch ranking.
struct CandidateEvalItem {
    pub record: EntryQueueRecord,
    pub token_count: u64,
    pub document_count: u64,
    pub zipf: f64,
    pub audit_flags: Vec<String>,
}

/// Loads existing decisions from `decisions.jsonl` to exclude active/human-reviewed decision target IDs.
///
/// Statuses other than `Unreviewed` (e.g. `Approved`, `ApprovedWithMetadataChange`, `RejectedFromDefaultPack`,
/// `ExperimentalOnly`, `NeedsLinguist`, `NeedsSourceInvestigation`) cause the target ID to be excluded.
/// Unreviewed decisions do NOT exclude a candidate.
pub fn load_existing_decision_target_ids<P: AsRef<Path>>(decisions_path: P) -> Result<HashSet<String>, String> {
    let path = decisions_path.as_ref();
    if !path.exists() {
        return Ok(HashSet::new());
    }

    let file = File::open(path)
        .map_err(|e| format!("Failed to open decisions file at {}: {}", path.display(), e))?;
    let reader = BufReader::new(file);
    let mut ids = HashSet::new();

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("Error reading decisions line {}: {}", line_idx + 1, e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rec: ReviewDecisionRecord = serde_json::from_str(trimmed)
            .map_err(|e| format!("Failed parsing decision JSON at line {}: {}", line_idx + 1, e))?;

        if rec.review_status != ReviewDecisionStatus::Unreviewed {
            ids.insert(rec.target_id);
        }
    }

    Ok(ids)
}

/// Loads corpus frequencies from `data/build/frequencies.jsonl`.
///
/// `data/build/frequencies.jsonl` is a REQUIRED prerequisite. Fails if missing.
pub fn load_corpus_frequencies<P: AsRef<Path>>(freq_path: P) -> Result<BTreeMap<String, (u64, u64, f64)>, String> {
    let path = freq_path.as_ref();
    if !path.exists() {
        return Err(format!(
            "Corpus frequencies file missing at '{}'. Run 'cargo run -p kurmanci-data-builder -- build-frequencies' first.",
            path.display()
        ));
    }

    let file = File::open(path)
        .map_err(|e| format!("Failed to open frequencies file at {}: {}", path.display(), e))?;
    let reader = BufReader::new(file);
    let mut map = BTreeMap::new();

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("Error reading frequencies line {}: {}", line_idx + 1, e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item: CorpusFrequencyItem = serde_json::from_str(trimmed)
            .map_err(|e| format!("Failed parsing frequency JSON at line {}: {}", line_idx + 1, e))?;
        map.insert(item.word, (item.token_count, item.document_count, item.zipf));
    }

    Ok(map)
}

/// Loads audit flags across all expected audit queues in `data/review-queues/kurdish-hunspell-kmr/`.
///
/// Fails loudly on malformed JSON with filename, line number, and parse error.
pub fn load_audit_flags<P: AsRef<Path>>(queues_dir: P) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let dir = queues_dir.as_ref();
    let mut flags_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    if !dir.exists() {
        return Ok(flags_map);
    }

    let queue_flag_specs = [
        ("metadata-conflict-groups.jsonl", "metadata_conflict"),
        ("suspicious-entries.jsonl", "suspicious_entry"),
        ("multiword-entries.jsonl", "multiword_entry"),
        ("capitalization-anomalies.jsonl", "capitalization_anomaly"),
        ("mixed-scripts.jsonl", "unusual_script"),
        ("unusual-scripts.jsonl", "unusual_script"),
        ("rare-code-points.jsonl", "unexpected_code_point"),
        ("unexpected-code-points.jsonl", "unexpected_code_point"),
        ("parser-rejections.jsonl", "parser_rejection"),
        ("symbol-only.jsonl", "non_letter_token"),
        ("punctuation-only.jsonl", "non_letter_token"),
        ("digit-only.jsonl", "non_letter_token"),
        ("no-letter.jsonl", "non_letter_token"),
        ("short-and-long-forms.jsonl", "short_or_long_form"),
    ];

    for (filename, flag_name) in queue_flag_specs {
        let path = dir.join(filename);
        if !path.exists() {
            continue;
        }

        let file = File::open(&path)
            .map_err(|e| format!("Failed opening queue file {}: {}", path.display(), e))?;
        let reader = BufReader::new(file);

        for (line_idx, line_res) in reader.lines().enumerate() {
            let line = line_res.map_err(|e| format!("Error reading queue line {} in {}: {}", line_idx + 1, path.display(), e))?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let gt: GenericQueueTarget = serde_json::from_str(trimmed).map_err(|e| {
                format!(
                    "Failed parsing audit queue file '{}' at line {}: {}",
                    filename,
                    line_idx + 1,
                    e
                )
            })?;

            if let Some(tid) = gt.target_id {
                flags_map.entry(tid).or_default().insert(flag_name.to_string());
            }
            if let Some(members) = gt.member_entry_ids {
                for member_id in members {
                    flags_map.entry(member_id).or_default().insert(flag_name.to_string());
                }
            }
        }
    }

    Ok(flags_map)
}

/// Generates the deterministic, ranked 1,000-entry human review batch from repository data.
pub fn generate_vocabulary_review_batch<P: AsRef<Path>>(root_dir: P) -> Result<VocabularyReviewBatchSummary, String> {
    let root = root_dir.as_ref();

    let pool_path = root.join("data/review-queues/kurdish-hunspell-kmr/hunspell-only.jsonl");
    let decisions_path = root.join("data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl");
    let freq_path = root.join("data/build/frequencies.jsonl");
    let queues_dir = root.join("data/review-queues/kurdish-hunspell-kmr");
    let report_dir = root.join("data/reports/vocabulary-review");

    if !pool_path.exists() {
        return Err(format!("Candidate pool missing at {}", pool_path.display()));
    }

    let existing_decisions = load_existing_decision_target_ids(&decisions_path)?;
    let frequencies = load_corpus_frequencies(&freq_path)?;
    let audit_flags_by_target = load_audit_flags(&queues_dir)?;

    let pool_file = File::open(&pool_path)
        .map_err(|e| format!("Failed opening candidate pool {}: {}", pool_path.display(), e))?;
    let pool_reader = BufReader::new(pool_file);

    let mut total_pool_candidates = 0;
    let mut excluded_existing_decisions = 0;
    let mut eval_items: Vec<CandidateEvalItem> = Vec::new();

    for (line_idx, line_res) in pool_reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("Error reading candidate pool line {}: {}", line_idx + 1, e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        total_pool_candidates += 1;
        let record: EntryQueueRecord = serde_json::from_str(trimmed)
            .map_err(|e| format!("Failed parsing candidate pool JSON line {}: {}", line_idx + 1, e))?;

        if existing_decisions.contains(&record.target_id) {
            excluded_existing_decisions += 1;
            continue;
        }

        let (token_count, document_count, zipf) = frequencies
            .get(&record.normalized)
            .copied()
            .unwrap_or((0, 0, 0.0));

        let flags_set = audit_flags_by_target
            .get(&record.target_id)
            .cloned()
            .unwrap_or_default();
        let audit_flags: Vec<String> = flags_set.into_iter().collect();

        eval_items.push(CandidateEvalItem {
            record,
            token_count,
            document_count,
            zipf,
            audit_flags,
        });
    }

    let eligible_pending_candidates = eval_items.len();

    // Primary ranking:
    // 1. Clean mechanically valid single-word candidates first (audit_flags is empty)
    // 2. Real corpus occurrence (token_count > 0)
    // 3. Higher document_count (descending)
    // 4. Higher token_count (descending)
    // 5. Higher zipf (descending)
    // 6. Deterministic tie-breakers: target_id (ascending), display (ascending)
    eval_items.sort_by(|a, b| {
        let a_clean = a.audit_flags.is_empty();
        let b_clean = b.audit_flags.is_empty();
        b_clean.cmp(&a_clean)
            .then_with(|| (b.token_count > 0).cmp(&(a.token_count > 0)))
            .then_with(|| b.document_count.cmp(&a.document_count))
            .then_with(|| b.token_count.cmp(&a.token_count))
            .then_with(|| b.zipf.partial_cmp(&a.zipf).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.record.target_id.cmp(&b.record.target_id))
            .then_with(|| a.record.display.cmp(&b.record.display))
    });

    let batch_size = std::cmp::min(1000, eval_items.len());
    let selected_items = &eval_items[..batch_size];

    let mut clean_candidates_count = 0;
    let mut corpus_matched_count = 0;

    let mut review_records: Vec<VocabularyReviewRecord> = Vec::with_capacity(batch_size);

    for (idx, item) in selected_items.iter().enumerate() {
        if item.audit_flags.is_empty() {
            clean_candidates_count += 1;
        }
        if item.token_count > 0 {
            corpus_matched_count += 1;
        }

        review_records.push(VocabularyReviewRecord {
            rank: idx + 1,
            target_id: item.record.target_id.clone(),
            source_id: item.record.source_id.clone(),
            source_revision: item.record.source_revision.clone(),
            source_lines: item.record.source_lines.clone(),
            form: item.record.display.clone(),
            normalized: item.record.normalized.clone(),
            imported_metadata: VocabularyImportedMetadata {
                flags: item.record.flags.clone(),
                morphology: item.record.morphology.clone(),
                part_of_speech: item.record.part_of_speech.clone(),
            },
            token_count: item.token_count,
            document_count: item.document_count,
            zipf: item.zipf,
            audit_flags: item.audit_flags.clone(),
            decision_status: "pending".to_string(),
        });
    }

    fs::create_dir_all(&report_dir)
        .map_err(|e| format!("Failed creating report directory {}: {}", report_dir.display(), e))?;

    let tsv_path = report_dir.join("top-1000.tsv");
    let jsonl_path = report_dir.join("top-1000.jsonl");
    let summary_path = report_dir.join("summary.json");

    // Write TSV
    let mut tsv_file = File::create(&tsv_path)
        .map_err(|e| format!("Failed creating TSV output at {}: {}", tsv_path.display(), e))?;
    writeln!(
        tsv_file,
        "rank\ttarget_id\tsource_id\tsource_revision\tsource_lines\tform\tnormalized\tpart_of_speech\tflags\tmorphology\ttoken_count\tdocument_count\tzipf\taudit_flags\tdecision_status"
    ).map_err(|e| format!("Failed writing TSV header: {}", e))?;

    for r in &review_records {
        let pos_str = r.imported_metadata.part_of_speech.as_deref().unwrap_or("");
        let morph_str = r.imported_metadata.morphology.join(";");
        let lines_str = r.source_lines.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(";");
        let flags_joined = r.audit_flags.join(";");

        let tc_str = if r.token_count > 0 { r.token_count.to_string() } else { "".to_string() };
        let dc_str = if r.document_count > 0 { r.document_count.to_string() } else { "".to_string() };
        let zipf_str = if r.zipf > 0.0 { format!("{:.2}", r.zipf) } else { "".to_string() };

        writeln!(
            tsv_file,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.rank,
            r.target_id,
            r.source_id,
            r.source_revision,
            lines_str,
            r.form,
            r.normalized,
            pos_str,
            r.imported_metadata.flags,
            morph_str,
            tc_str,
            dc_str,
            zipf_str,
            flags_joined,
            r.decision_status
        ).map_err(|e| format!("Failed writing TSV record rank {}: {}", r.rank, e))?;
    }

    // Write JSONL
    let mut jsonl_file = File::create(&jsonl_path)
        .map_err(|e| format!("Failed creating JSONL output at {}: {}", jsonl_path.display(), e))?;

    for r in &review_records {
        let json_line = serde_json::to_string(r)
            .map_err(|e| format!("Failed serializing review record rank {}: {}", r.rank, e))?;
        writeln!(jsonl_file, "{}", json_line)
            .map_err(|e| format!("Failed writing JSONL line rank {}: {}", r.rank, e))?;
    }

    let summary = VocabularyReviewBatchSummary {
        schema_version: VOCABULARY_REVIEW_SUMMARY_SCHEMA.to_string(),
        generator: "kurmanci-data-builder".to_string(),
        candidate_pool: "data/review-queues/kurdish-hunspell-kmr/hunspell-only.jsonl".to_string(),
        total_pool_candidates,
        excluded_existing_decisions,
        eligible_pending_candidates,
        batch_size,
        clean_candidates_count,
        corpus_matched_count,
        output_tsv: "data/reports/vocabulary-review/top-1000.tsv".to_string(),
        output_jsonl: "data/reports/vocabulary-review/top-1000.jsonl".to_string(),
    };

    let summary_json = serde_json::to_string_pretty(&summary)
        .map_err(|e| format!("Failed serializing summary JSON: {}", e))?;
    fs::write(&summary_path, summary_json)
        .map_err(|e| format!("Failed writing summary JSON at {}: {}", summary_path.display(), e))?;

    Ok(summary)
}
