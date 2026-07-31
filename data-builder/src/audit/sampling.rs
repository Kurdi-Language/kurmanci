//! Deterministic stratified sampling for the quality audit.
//!
//! Produces a review sample using quantile buckets, category queues,
//! and deterministic tie-breaking.

use crate::audit::analysis::AcceptedAnalysis;
use crate::audit::classify::{classify_script, is_possible_proper_noun, ScriptClass};
use crate::audit::input::AuditInputs;
use crate::importers::ImportedLexiconRecord;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A single review sample record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSampleRecord {
    pub normalized: String,
    pub word: String,
    pub source_line_num: usize,
    pub flags: String,
    pub morphology: Vec<String>,
    pub part_of_speech: String,
    pub sample_groups: Vec<String>,
}

/// Review sample metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSample {
    pub total_imported_records: usize,
    pub records_emitted: usize,
    pub selection_method: String,
    pub records: Vec<ReviewSampleRecord>,
}

/// Generates a deterministic stratified review sample.
pub fn generate_review_sample(
    inputs: &AuditInputs,
    accepted_analysis: &AcceptedAnalysis,
) -> ReviewSample {
    let records = &inputs.imported_records;
    if records.is_empty() {
        return ReviewSample {
            total_imported_records: 0,
            records_emitted: 0,
            selection_method: "deterministic_stratified_sample".to_string(),
            records: Vec::new(),
        };
    }

    // Key = (normalized, source_line_num) for deduplication
    let mut selected: BTreeMap<(String, usize), Vec<String>> = BTreeMap::new();

    // 1. Global stride: divide into 10 quantile buckets, pick 5 from each
    let bucket_size = (records.len() / 10).max(1);
    for bucket_idx in 0..10 {
        let start = bucket_idx * bucket_size;
        if start >= records.len() {
            break;
        }
        let end = ((bucket_idx + 1) * bucket_size).min(records.len());
        let bucket_len = end - start;
        if bucket_len == 0 {
            continue;
        }

        let stride = (bucket_len / 5).max(1);
        for i in 0..5 {
            let idx = start + i * stride;
            if idx >= end {
                break;
            }
            let rec = &records[idx];
            selected
                .entry((rec.normalized.clone(), rec.source_line_num))
                .or_default()
                .push(format!("global_stride_bucket_{}", bucket_idx));
        }
    }

    // 2. Category queues: shortest entries (by grapheme length)
    let mut by_length: Vec<(usize, usize)> = records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            use unicode_segmentation::UnicodeSegmentation;
            (r.normalized.graphemes(true).count(), i)
        })
        .collect();
    by_length.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    // Shortest 10
    for &(_, idx) in by_length.iter().take(10) {
        let rec = &records[idx];
        selected
            .entry((rec.normalized.clone(), rec.source_line_num))
            .or_default()
            .push("shortest".to_string());
    }

    // Longest 10
    for &(_, idx) in by_length.iter().rev().take(10) {
        let rec = &records[idx];
        selected
            .entry((rec.normalized.clone(), rec.source_line_num))
            .or_default()
            .push("longest".to_string());
    }

    // 3. Mixed-script entries (up to 10)
    let mut mixed_count = 0;
    for rec in records {
        if mixed_count >= 10 {
            break;
        }
        let script = classify_script(&rec.normalized);
        if matches!(
            script,
            ScriptClass::LatinArabicMixed
                | ScriptClass::LatinCyrillicMixed
                | ScriptClass::OtherMixedScript
        ) {
            selected
                .entry((rec.normalized.clone(), rec.source_line_num))
                .or_default()
                .push("mixed_script".to_string());
            mixed_count += 1;
        }
    }

    // 4. Entries with rare characters (up to 10)
    let mut rare_count = 0;
    for entry in &accepted_analysis.suspicious_entries {
        if rare_count >= 10 {
            break;
        }
        if entry.category == "RARE_CODEPOINT" {
            selected
                .entry((entry.normalized.clone(), entry.source_line_num))
                .or_default()
                .push("rare_character".to_string());
            rare_count += 1;
        }
    }

    // 5. Proper noun candidates (up to 10)
    let mut proper_count = 0;
    for rec in records {
        if proper_count >= 10 {
            break;
        }
        if is_possible_proper_noun(&rec.word) {
            selected
                .entry((rec.normalized.clone(), rec.source_line_num))
                .or_default()
                .push("possible_proper_noun".to_string());
            proper_count += 1;
        }
    }

    // 6. Manual seed overlap (up to 10)
    let seed_normalized: BTreeSet<String> = inputs
        .manual_seed
        .iter()
        .map(|s| crate::normalize::normalize_text(&s.normalized))
        .collect();
    let mut seed_count = 0;
    for rec in records {
        if seed_count >= 10 {
            break;
        }
        if seed_normalized.contains(&rec.normalized) {
            selected
                .entry((rec.normalized.clone(), rec.source_line_num))
                .or_default()
                .push("manual_seed_overlap".to_string());
            seed_count += 1;
        }
    }

    // Build the final record list: lookup each selected key in records
    let records_by_key: BTreeMap<(String, usize), &ImportedLexiconRecord> = records
        .iter()
        .map(|r| ((r.normalized.clone(), r.source_line_num), r))
        .collect();

    let mut sample_records: Vec<ReviewSampleRecord> = selected
        .into_iter()
        .filter_map(|(key, groups)| {
            records_by_key.get(&key).map(|rec| ReviewSampleRecord {
                normalized: rec.normalized.clone(),
                word: rec.word.clone(),
                source_line_num: rec.source_line_num,
                flags: rec.flags.clone(),
                morphology: rec.morphology.clone(),
                part_of_speech: rec.part_of_speech.clone(),
                sample_groups: groups,
            })
        })
        .collect();

    // Sort deterministically: by first sample_group, then normalized, then line number
    sample_records.sort_by(|a, b| {
        a.sample_groups
            .first()
            .cmp(&b.sample_groups.first())
            .then_with(|| a.normalized.cmp(&b.normalized))
            .then_with(|| a.source_line_num.cmp(&b.source_line_num))
    });

    let emitted = sample_records.len();

    ReviewSample {
        total_imported_records: records.len(),
        records_emitted: emitted,
        selection_method: "deterministic_stratified_sample".to_string(),
        records: sample_records,
    }
}
