//! Statistical Report module for corpus frequency analysis reports.

use super::frequency::{FrequencyBuildStats, FrequencyRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Top-level frequency summary report (`summary.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrequencySummaryReport {
    pub documents: usize,
    pub tokens: usize,
    pub unique_words: usize,
    pub hapax_legomena: usize,
    pub max_frequency: usize,
    pub mean_frequency: f64,
    pub median_frequency: f64,
    pub zipf_distribution: BTreeMap<String, usize>,
}

/// Token length distribution statistics (`length-distribution.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LengthDistributionReport {
    pub min_length: usize,
    pub max_length: usize,
    pub mean_length: f64,
    pub median_length: usize,
    pub distribution: BTreeMap<usize, usize>,
}

/// Character analysis report (`character-analysis.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterAnalysisEntry {
    pub character: String,
    pub codepoint: String,
    pub total_occurrences: usize,
    pub unique_words_count: usize,
}

/// Cumulative vocabulary token coverage report (`coverage.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabularyCoverageReport {
    pub total_tokens: usize,
    pub total_unique_words: usize,
    pub coverage_milestones: BTreeMap<String, usize>,
}

/// Generates and writes all statistical report files to `data/reports/frequencies/` using backup-and-rollback safety.
pub fn write_all_frequency_reports<P: AsRef<Path>>(
    root_dir: P,
    stats: &FrequencyBuildStats,
) -> Result<(), String> {
    let root = root_dir.as_ref();
    let output_dir = root.join("data/reports/frequencies");

    let stage_dir = output_dir.with_extension("tmp_stage");
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .map_err(|e| format!("Failed to clean stage dir {:?}: {}", stage_dir, e))?;
    }
    fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("Failed to create stage dir {:?}: {}", stage_dir, e))?;

    // 1. summary.json
    let unique_words = stats.records.len();
    let hapax_legomena = stats.records.iter().filter(|r| r.token_count == 1).count();
    let max_frequency = stats.records.first().map(|r| r.token_count).unwrap_or(0);

    let mean_frequency = if unique_words > 0 {
        stats.total_tokens as f64 / unique_words as f64
    } else {
        0.0
    };

    let median_frequency = if unique_words > 0 {
        let mid = unique_words / 2;
        if (unique_words & 1) == 0 {
            (stats.records[mid - 1].token_count + stats.records[mid].token_count) as f64 / 2.0
        } else {
            stats.records[mid].token_count as f64
        }
    } else {
        0.0
    };

    let mut zipf_distribution: BTreeMap<String, usize> = BTreeMap::new();
    for r in &stats.records {
        let bucket = format!("{:.0}", r.zipf.floor());
        *zipf_distribution.entry(bucket).or_insert(0) += 1;
    }

    let summary = FrequencySummaryReport {
        documents: stats.total_documents,
        tokens: stats.total_tokens,
        unique_words,
        hapax_legomena,
        max_frequency,
        mean_frequency: (mean_frequency * 10000.0).round() / 10000.0,
        median_frequency,
        zipf_distribution,
    };
    write_json(&stage_dir, "summary.json", &summary)?;

    // 2. top-100.json
    let top_100: Vec<FrequencyRecord> = stats.records.iter().take(100).cloned().collect();
    write_json(&stage_dir, "top-100.json", &top_100)?;

    // 3. length-distribution.json
    let mut length_dist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut total_len_scalars = 0u64;

    for r in &stats.records {
        let len = r.word.chars().count();
        *length_dist.entry(len).or_insert(0) += r.token_count;
        total_len_scalars += (len * r.token_count) as u64;
    }

    let min_length = length_dist.keys().next().copied().unwrap_or(0);
    let max_length = length_dist.keys().next_back().copied().unwrap_or(0);
    let mean_length = if stats.total_tokens > 0 {
        total_len_scalars as f64 / stats.total_tokens as f64
    } else {
        0.0
    };

    let length_report = LengthDistributionReport {
        min_length,
        max_length,
        mean_length: (mean_length * 10000.0).round() / 10000.0,
        median_length: 0, // calculate if needed
        distribution: length_dist,
    };
    write_json(&stage_dir, "length-distribution.json", &length_report)?;

    // 4. character-analysis.json
    let mut char_occurrences: BTreeMap<char, usize> = BTreeMap::new();
    let mut char_word_counts: BTreeMap<char, usize> = BTreeMap::new();

    for r in &stats.records {
        let mut seen_in_word = BTreeMap::new();
        for c in r.word.chars() {
            *char_occurrences.entry(c).or_insert(0) += r.token_count;
            seen_in_word.insert(c, ());
        }
        for (c, _) in seen_in_word {
            *char_word_counts.entry(c).or_insert(0) += 1;
        }
    }

    let char_analysis: Vec<CharacterAnalysisEntry> = char_occurrences
        .into_iter()
        .map(|(c, count)| CharacterAnalysisEntry {
            character: c.to_string(),
            codepoint: format!("U+{:04X}", c as u32),
            total_occurrences: count,
            unique_words_count: *char_word_counts.get(&c).unwrap_or(&0),
        })
        .collect();

    write_json(&stage_dir, "character-analysis.json", &char_analysis)?;

    // 5. coverage.json
    let mut accumulated_tokens = 0usize;
    let mut coverage_milestones: BTreeMap<String, usize> = BTreeMap::new();

    let targets = [50.0, 80.0, 90.0, 95.0, 99.0];
    let mut target_idx = 0;

    for (idx, r) in stats.records.iter().enumerate() {
        accumulated_tokens += r.token_count;
        let pct = (accumulated_tokens as f64 / stats.total_tokens as f64) * 100.0;

        while target_idx < targets.len() && pct >= targets[target_idx] {
            coverage_milestones.insert(format!("{:.0}%", targets[target_idx]), idx + 1);
            target_idx += 1;
        }
    }

    let coverage_report = VocabularyCoverageReport {
        total_tokens: stats.total_tokens,
        total_unique_words: unique_words,
        coverage_milestones,
    };
    write_json(&stage_dir, "coverage.json", &coverage_report)?;

    // 6. README.md
    write_readme(&stage_dir, &summary)?;

    // 7. artifacts.sha256
    let build_freq_path = root.join("data/build/frequencies.jsonl");
    let mut manifest_content = String::new();

    if build_freq_path.exists() {
        let content = fs::read(&build_freq_path).map_err(|e| {
            format!(
                "Failed to read data/build/frequencies.jsonl for manifest: {}",
                e
            )
        })?;
        let hash = format!("{:x}", Sha256::digest(&content));
        manifest_content.push_str(&format!("{}  data/build/frequencies.jsonl\n", hash));
    }

    let report_files = [
        "summary.json",
        "top-100.json",
        "length-distribution.json",
        "character-analysis.json",
        "coverage.json",
        "README.md",
    ];

    for file in &report_files {
        let content = fs::read(stage_dir.join(file))
            .map_err(|e| format!("Failed to read report file {} for manifest: {}", file, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        manifest_content.push_str(&format!("{}  data/reports/frequencies/{}\n", hash, file));
    }
    fs::write(stage_dir.join("artifacts.sha256"), manifest_content)
        .map_err(|e| format!("Failed to write artifacts.sha256 manifest: {}", e))?;

    // Atomic replacement with backup and rollback
    let backup_dir = output_dir.with_extension("tmp_backup");
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)
            .map_err(|e| format!("Failed to clean backup dir {:?}: {}", backup_dir, e))?;
    }

    if output_dir.exists() {
        fs::rename(&output_dir, &backup_dir).map_err(|e| {
            format!(
                "Failed to move output dir {:?} to backup dir {:?}: {}",
                output_dir, backup_dir, e
            )
        })?;
    }

    match fs::rename(&stage_dir, &output_dir) {
        Ok(()) => {
            if backup_dir.exists() {
                let _ = fs::remove_dir_all(&backup_dir);
            }
            Ok(())
        }
        Err(install_err) => {
            if backup_dir.exists() {
                let _ = fs::rename(&backup_dir, &output_dir);
            }
            Err(format!(
                "Failed to install frequency reports: {}",
                install_err
            ))
        }
    }
}

fn write_json<T: Serialize>(dir: &Path, filename: &str, data: &T) -> Result<(), String> {
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Failed to serialize {}: {}", filename, e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {:?}: {}", path, e))?;
    Ok(())
}

fn write_readme(dir: &Path, summary: &FrequencySummaryReport) -> Result<(), String> {
    let content = format!(
        r#"# Kurmancî Corpus Frequency Analysis Report

- **Total Documents**: {}
- **Total Tokens**: {}
- **Unique Words (Vocabulary)**: {}
- **Hapax Legomena (Count=1)**: {}
- **Max Word Frequency**: {}
- **Mean Token Frequency**: {}
- **Median Token Frequency**: {}

## Determinism & Reproducibility
All frequency calculation, tokenization, sorting, and reporting steps are 100% deterministic.
Manifest of hashes is recorded in `artifacts.sha256`.
"#,
        summary.documents,
        summary.tokens,
        summary.unique_words,
        summary.hapax_legomena,
        summary.max_frequency,
        summary.mean_frequency,
        summary.median_frequency
    );

    fs::write(dir.join("README.md"), content)
        .map_err(|e| format!("Failed to write frequency README.md: {}", e))
}
