//! Frequency-to-Lexicon Join module.
//!
//! Joins deterministic corpus frequency data (`data/build/frequencies.jsonl`)
//! to canonical lexicon entries by exact normalized word matching.

use crate::validate::{FrequencyMetadata, SourceLexiconEntry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Summary report emitted after frequency-to-lexicon join.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrequencyJoinSummaryReport {
    pub lexicon_entries: usize,
    pub frequency_records: usize,
    pub matched_entries: usize,
    pub unmatched_lexicon_entries: usize,
    pub unmatched_frequency_records: usize,
    pub lexicon_coverage_percent: f64,
}

/// Joins frequency records from `frequencies.jsonl` into `SourceLexiconEntry` slices.
pub fn join_frequencies_to_lexicon<P: AsRef<Path>>(
    root_dir: P,
    lexicon_entries: &mut [SourceLexiconEntry],
) -> Result<FrequencyJoinSummaryReport, String> {
    let root = root_dir.as_ref();
    let freq_path = root.join("data/build/frequencies.jsonl");

    let mut freq_map: BTreeMap<String, FrequencyMetadata> = BTreeMap::new();
    let mut total_freq_records = 0usize;

    if freq_path.exists() {
        let file =
            File::open(&freq_path).map_err(|e| format!("Failed to open {:?}: {}", freq_path, e))?;
        let reader = BufReader::new(file);

        for (line_num, line_res) in reader.lines().enumerate() {
            let line = line_res.map_err(|e| format!("Read error in {:?}: {}", freq_path, e))?;
            if line.trim().is_empty() {
                continue;
            }

            let val: serde_json::Value = serde_json::from_str(&line).map_err(|e| {
                format!(
                    "Invalid JSON on line {} in {:?}: {}",
                    line_num + 1,
                    freq_path,
                    e
                )
            })?;

            let word = val["word"]
                .as_str()
                .ok_or_else(|| format!("Line {}: missing 'word' field", line_num + 1))?
                .to_string();

            let token_count = val["token_count"]
                .as_u64()
                .ok_or_else(|| format!("Line {}: invalid 'token_count'", line_num + 1))?;

            let document_count = val["document_count"]
                .as_u64()
                .ok_or_else(|| format!("Line {}: invalid 'document_count'", line_num + 1))?;

            let zipf_f64 = val["zipf"]
                .as_f64()
                .ok_or_else(|| format!("Line {}: invalid 'zipf' float", line_num + 1))?;

            if !zipf_f64.is_finite() || zipf_f64 < 0.0 {
                return Err(format!(
                    "Line {}: 'zipf' value {} is not finite or negative",
                    line_num + 1,
                    zipf_f64
                ));
            }

            let milli_f = (zipf_f64 * 1000.0).round();
            if milli_f < 0.0 || milli_f > u32::MAX as f64 {
                return Err(format!(
                    "Line {}: 'zipf_milli' overflow for value {}",
                    line_num + 1,
                    zipf_f64
                ));
            }
            let zipf_milli = milli_f as u32;

            if freq_map.contains_key(&word) {
                return Err(format!(
                    "Line {}: Duplicate frequency record for word '{}'",
                    line_num + 1,
                    word
                ));
            }

            total_freq_records += 1;
            freq_map.insert(
                word,
                FrequencyMetadata {
                    token_count,
                    document_count,
                    zipf_milli,
                },
            );
        }
    }

    let mut matched_entries = 0usize;
    let mut matched_freq_words = BTreeSet::new();

    for entry in lexicon_entries.iter_mut() {
        if let Some(meta) = freq_map.get(&entry.normalized) {
            entry.frequency_metadata = Some(meta.clone());
            matched_entries += 1;
            matched_freq_words.insert(entry.normalized.clone());
        } else {
            entry.frequency_metadata = Some(FrequencyMetadata {
                token_count: 0,
                document_count: 0,
                zipf_milli: 0,
            });
        }
    }

    let total_lexicon_entries = lexicon_entries.len();
    let unmatched_lexicon_entries = total_lexicon_entries - matched_entries;
    let unmatched_frequency_records = total_freq_records.saturating_sub(matched_freq_words.len());

    let lexicon_coverage_percent = if total_lexicon_entries > 0 {
        (matched_entries as f64 / total_lexicon_entries as f64) * 100.0
    } else {
        0.0
    };

    let report = FrequencyJoinSummaryReport {
        lexicon_entries: total_lexicon_entries,
        frequency_records: total_freq_records,
        matched_entries,
        unmatched_lexicon_entries,
        unmatched_frequency_records,
        lexicon_coverage_percent: (lexicon_coverage_percent * 100.0).round() / 100.0,
    };

    // Write join report
    let report_dir = root.join("data/reports/frequency-join");
    fs::create_dir_all(&report_dir)
        .map_err(|e| format!("Failed to create report dir {:?}: {}", report_dir, e))?;
    let report_json = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("Failed to serialize join summary report: {}", e))?;
    fs::write(report_dir.join("summary.json"), report_json)
        .map_err(|e| format!("Failed to write join summary.json: {}", e))?;

    Ok(report)
}
