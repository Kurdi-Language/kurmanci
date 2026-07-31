//! Deterministic Bigram Language Model Extraction, Sentence Segmentation,
//! Fixed-Point Probability Calculation, Pruning, and Report Suite.

use crate::corpus::registry::CorpusRegistry;
use crate::corpus::tokenizer::tokenize_text;
use crate::validate::SourceLexiconEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Pruning configuration loaded from `data-builder/config/ngrams.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NgramConfig {
    pub pruning: PruningConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningConfig {
    pub minimum_count: u64,
    pub maximum_predictions_per_context: usize,
}

impl Default for NgramConfig {
    fn default() -> Self {
        Self {
            pruning: PruningConfig {
                minimum_count: 2,
                maximum_predictions_per_context: 16,
            },
        }
    }
}

/// A single extracted and pruned bigram record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BigramRecord {
    pub previous: String,
    pub next: String,
    pub count: u64,
    pub context_count: u64,
    pub probability_millionths: u32,
}

/// Overall statistical summary report for the bigram extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NgramSummaryReport {
    pub total_sentences: usize,
    pub total_bigram_tokens: usize,
    pub raw_distinct_bigrams: usize,
    pub pruned_distinct_bigrams: usize,
    pub distinct_context_count: usize,
    pub lexicon_coverage_percent: f64,
    pub pack_eligible_bigrams: usize,
    pub excluded_oov_contexts: usize,
    pub excluded_oov_predictions: usize,
}

/// Returned stats from `build_corpus_bigrams`.
#[derive(Debug, Clone)]
pub struct BigramBuildStats {
    pub total_sentences: usize,
    pub total_bigram_tokens: usize,
    pub records: Vec<BigramRecord>,
}

/// Splits input document text into sentences using Unicode punctuation rules.
///
/// Primary sentence terminators: `.`, `!`, `?`, `…` (`U+2026`), `۔` (`U+06D4`).
/// Any maximal consecutive sequence of sentence terminators (e.g. `...`, `?!`, `!!!`)
/// creates exactly one sentence boundary. Empty sentences are discarded.
/// Semicolons `;`, colons `:`, and commas `,` remain internal to the sentence.
pub fn split_into_sentences(text: &str) -> Vec<Vec<String>> {
    let is_terminator = |c: char| c == '.' || c == '!' || c == '?' || c == '…' || c == '\u{06D4}';

    let mut sentences = Vec::new();
    let mut current_segment = String::new();
    let mut in_terminator_seq = false;

    for ch in text.chars() {
        if is_terminator(ch) {
            if !in_terminator_seq {
                let trimmed = current_segment.trim();
                if !trimmed.is_empty() {
                    let tokens = tokenize_text(trimmed);
                    if !tokens.is_empty() {
                        sentences.push(tokens);
                    }
                }
                current_segment.clear();
                in_terminator_seq = true;
            }
        } else {
            in_terminator_seq = false;
            current_segment.push(ch);
        }
    }

    let trimmed = current_segment.trim();
    if !trimmed.is_empty() {
        let tokens = tokenize_text(trimmed);
        if !tokens.is_empty() {
            sentences.push(tokens);
        }
    }

    sentences
}

/// Builds deterministic bigram statistics across all registered imported text corpora.
pub fn build_corpus_bigrams<P: AsRef<Path>>(root_dir: P) -> Result<BigramBuildStats, String> {
    let root = root_dir.as_ref();
    let registry_path = root.join("data/source-registry/corpora.toml");
    let registry = CorpusRegistry::load_from_file(&registry_path)
        .map_err(|e| format!("Failed to load corpus registry {:?}: {}", registry_path, e))?;

    // Load pruning config if available
    let config_path = root.join("data-builder/config/ngrams.toml");
    let config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read {:?}: {}", config_path, e))?;
        toml::from_str::<NgramConfig>(&content)
            .map_err(|e| format!("Invalid ngrams.toml config format: {}", e))?
    } else {
        NgramConfig::default()
    };

    if config.pruning.minimum_count < 1 {
        return Err("pruning.minimum_count must be >= 1".to_string());
    }
    if config.pruning.maximum_predictions_per_context < 1
        || config.pruning.maximum_predictions_per_context > u16::MAX as usize
    {
        return Err("pruning.maximum_predictions_per_context out of valid u16 bounds".to_string());
    }

    // Load canonical lexicon words for vocabulary coverage checks
    let lexicon_path = root.join("data/reviewed/lexicon.jsonl");
    let mut canonical_words = BTreeSet::new();
    if lexicon_path.exists() {
        let file = File::open(&lexicon_path)
            .map_err(|e| format!("Failed to open {:?}: {}", lexicon_path, e))?;
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<SourceLexiconEntry>(&line) {
                canonical_words.insert(entry.normalized);
            }
        }
    }

    let mut raw_bigram_counts: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut context_totals: BTreeMap<String, u64> = BTreeMap::new();

    let mut total_sentences = 0usize;
    let mut total_bigram_tokens = 0usize;

    // Scan corpora in exact registry order
    for entry in &registry.corpora {
        let corpus_dir = root.join(format!("data/imported/{}", entry.corpus_id));
        if !corpus_dir.exists() {
            return Err(format!(
                "Imported corpus directory missing for registered corpus '{}': {:?}",
                entry.corpus_id, corpus_dir
            ));
        }

        let mut sorted_files = entry.files.clone();
        sorted_files.sort_by(|a, b| a.path.cmp(&b.path));

        for cfile in &sorted_files {
            let filename = Path::new(&cfile.path)
                .file_name()
                .ok_or_else(|| format!("Invalid file path in corpus: {}", cfile.path))?;
            let file_path = corpus_dir.join(filename);
            if !file_path.exists() {
                return Err(format!(
                    "Imported corpus file missing for '{}': {:?}",
                    entry.corpus_id, file_path
                ));
            }

            // Verify checksum before reading
            let file_bytes = fs::read(&file_path)
                .map_err(|e| format!("Failed to read corpus file {:?}: {}", file_path, e))?;
            let actual_hash = format!("{:x}", Sha256::digest(&file_bytes));
            if actual_hash != cfile.sha256 {
                return Err(format!(
                    "Corpus file checksum mismatch for {:?}: expected {}, got {}",
                    file_path, cfile.sha256, actual_hash
                ));
            }

            let reader = BufReader::new(&file_bytes[..]);

            for line_res in reader.lines() {
                let line = line_res.map_err(|e| format!("Read error in {:?}: {}", file_path, e))?;
                if line.trim().is_empty() {
                    continue;
                }

                let sentences = split_into_sentences(&line);
                for sentence in sentences {
                    total_sentences += 1;
                    if sentence.len() < 2 {
                        continue;
                    }

                    for window in sentence.windows(2) {
                        let prev = &window[0];
                        let next = &window[1];

                        total_bigram_tokens += 1;
                        *raw_bigram_counts
                            .entry((prev.clone(), next.clone()))
                            .or_insert(0) += 1;
                        *context_totals.entry(prev.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let raw_distinct_bigrams = raw_bigram_counts.len();

    // Compute unpruned records with original context counts & fixed-point integer probabilities
    let mut unpruned_records = Vec::with_capacity(raw_distinct_bigrams);
    for ((prev, next), count) in raw_bigram_counts {
        let context_count = *context_totals.get(&prev).unwrap_or(&count);
        if context_count == 0 || count > context_count {
            return Err(format!(
                "Invalid context count {} for count {} in ({}, {})",
                context_count, count, prev, next
            ));
        }

        let numerator = u128::from(count)
            .checked_mul(1_000_000)
            .and_then(|v| v.checked_add(u128::from(context_count / 2)))
            .ok_or_else(|| format!("Probability numerator overflow for ({}, {})", prev, next))?;

        let prob = numerator / u128::from(context_count);
        let probability_millionths = u32::try_from(prob)
            .map_err(|_| format!("Probability overflow for ({}, {})", prev, next))?;

        if probability_millionths > 1_000_000 {
            return Err(format!(
                "Probability {} exceeds 1,000,000 for ({}, {})",
                probability_millionths, prev, next
            ));
        }

        unpruned_records.push(BigramRecord {
            previous: prev,
            next,
            count,
            context_count,
            probability_millionths,
        });
    }

    // Apply Pruning Rules:
    // Rule 1: Remove records below minimum_count
    let count_filtered: Vec<BigramRecord> = unpruned_records
        .into_iter()
        .filter(|r| r.count >= config.pruning.minimum_count)
        .collect();

    let count_pruned_out = raw_distinct_bigrams - count_filtered.len();

    // Group by previous word
    let mut grouped: BTreeMap<String, Vec<BigramRecord>> = BTreeMap::new();
    for rec in count_filtered {
        grouped.entry(rec.previous.clone()).or_default().push(rec);
    }

    let mut pruned_records = Vec::new();
    let mut cap_pruned_out = 0usize;

    for (_prev, mut group) in grouped {
        // Rule 2: Sort by count DESC, then next ASC
        group.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.next.cmp(&b.next)));

        // Rule 3: Retain at most maximum_predictions_per_context
        if group.len() > config.pruning.maximum_predictions_per_context {
            cap_pruned_out += group.len() - config.pruning.maximum_predictions_per_context;
            group.truncate(config.pruning.maximum_predictions_per_context);
        }
        pruned_records.extend(group);
    }

    // Deterministic output order: previous ASC, count DESC, next ASC
    pruned_records.sort_by(|a, b| {
        a.previous
            .cmp(&b.previous)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.next.cmp(&b.next))
    });

    let pruned_distinct_bigrams = pruned_records.len();

    // OOV Analysis
    let mut pack_eligible_bigrams = 0usize;
    let mut oov_contexts = BTreeSet::new();
    let mut oov_predictions = BTreeSet::new();
    let mut oov_records = Vec::new();

    let mut distinct_contexts = BTreeSet::new();

    for rec in &pruned_records {
        distinct_contexts.insert(&rec.previous);
        let prev_in = canonical_words.contains(&rec.previous);
        let next_in = canonical_words.contains(&rec.next);

        if prev_in && next_in {
            pack_eligible_bigrams += 1;
        } else {
            if !prev_in {
                oov_contexts.insert(rec.previous.clone());
            }
            if !next_in {
                oov_predictions.insert(rec.next.clone());
            }
            oov_records.push(rec.clone());
        }
    }

    let lexicon_coverage_percent = if pruned_distinct_bigrams > 0 {
        (pack_eligible_bigrams as f64 / pruned_distinct_bigrams as f64) * 100.0
    } else {
        0.0
    };

    let summary = NgramSummaryReport {
        total_sentences,
        total_bigram_tokens,
        raw_distinct_bigrams,
        pruned_distinct_bigrams,
        distinct_context_count: distinct_contexts.len(),
        lexicon_coverage_percent: (lexicon_coverage_percent * 100.0).round() / 100.0,
        pack_eligible_bigrams,
        excluded_oov_contexts: oov_contexts.len(),
        excluded_oov_predictions: oov_predictions.len(),
    };

    // Staged write to data/build/bigrams.jsonl and data/reports/ngrams/
    let build_dir = root.join("data/build");
    fs::create_dir_all(&build_dir)
        .map_err(|e| format!("Failed to create build dir {:?}: {}", build_dir, e))?;

    let jsonl_path = build_dir.join("bigrams.jsonl");
    let mut jsonl_file = File::create(&jsonl_path)
        .map_err(|e| format!("Failed to create {:?}: {}", jsonl_path, e))?;

    for rec in &pruned_records {
        let line = serde_json::to_string(rec)
            .map_err(|e| format!("Failed to serialize bigram record: {}", e))?;
        writeln!(jsonl_file, "{}", line)
            .map_err(|e| format!("Failed to write to {:?}: {}", jsonl_path, e))?;
    }

    // Write reports
    write_ngram_reports(
        root,
        &config,
        &summary,
        &pruned_records,
        &oov_records,
        count_pruned_out,
        cap_pruned_out,
    )?;

    Ok(BigramBuildStats {
        total_sentences,
        total_bigram_tokens,
        records: pruned_records,
    })
}

fn write_ngram_reports(
    root: &Path,
    config: &NgramConfig,
    summary: &NgramSummaryReport,
    records: &[BigramRecord],
    oov_records: &[BigramRecord],
    count_pruned_out: usize,
    cap_pruned_out: usize,
) -> Result<(), String> {
    let output_dir = root.join("data/reports/ngrams");
    let stage_dir = output_dir.with_extension(format!(
        "tmp_stage_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));

    if stage_dir.exists() {
        let _ = fs::remove_dir_all(&stage_dir);
    }
    fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("Failed to create stage report dir {:?}: {}", stage_dir, e))?;

    // 1. summary.json
    let summary_json = serde_json::to_string_pretty(summary)
        .map_err(|e| format!("Failed to serialize summary report: {}", e))?;
    fs::write(stage_dir.join("summary.json"), summary_json)
        .map_err(|e| format!("Failed to write summary.json: {}", e))?;

    // 2. top-bigrams.json (Top 100 by count)
    let mut top_records = records.to_vec();
    top_records.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.previous.cmp(&b.previous))
    });
    top_records.truncate(100);
    let top_json = serde_json::to_string_pretty(&top_records)
        .map_err(|e| format!("Failed to serialize top-bigrams: {}", e))?;
    fs::write(stage_dir.join("top-bigrams.json"), top_json)
        .map_err(|e| format!("Failed to write top-bigrams.json: {}", e))?;

    // 3. context-distribution.json
    let mut context_map: BTreeMap<String, usize> = BTreeMap::new();
    for rec in records {
        *context_map.entry(rec.previous.clone()).or_default() += 1;
    }
    let context_json = serde_json::to_string_pretty(&context_map)
        .map_err(|e| format!("Failed to serialize context-distribution: {}", e))?;
    fs::write(stage_dir.join("context-distribution.json"), context_json)
        .map_err(|e| format!("Failed to write context-distribution.json: {}", e))?;

    // 4. out-of-vocabulary.json
    let oov_json = serde_json::to_string_pretty(oov_records)
        .map_err(|e| format!("Failed to serialize OOV records: {}", e))?;
    fs::write(stage_dir.join("out-of-vocabulary.json"), oov_json)
        .map_err(|e| format!("Failed to write out-of-vocabulary.json: {}", e))?;

    // 5. pruning-summary.json
    let pruning_summary = serde_json::json!({
        "minimum_count_threshold": config.pruning.minimum_count,
        "maximum_predictions_per_context": config.pruning.maximum_predictions_per_context,
        "raw_distinct_bigrams": summary.raw_distinct_bigrams,
        "pruned_distinct_bigrams": summary.pruned_distinct_bigrams,
        "count_pruned_out": count_pruned_out,
        "capacity_pruned_out": cap_pruned_out,
        "total_records_pruned": count_pruned_out + cap_pruned_out,
    });
    let pruning_json = serde_json::to_string_pretty(&pruning_summary)
        .map_err(|e| format!("Failed to serialize pruning summary: {}", e))?;
    fs::write(stage_dir.join("pruning-summary.json"), pruning_json)
        .map_err(|e| format!("Failed to write pruning-summary.json: {}", e))?;

    // 6. README.md
    let readme_content = format!(
        r#"# Kurmancî Bigram Statistical Reports

- **Total Sentences**: {}
- **Total Bigram Tokens**: {}
- **Raw Distinct Bigrams**: {}
- **Pruned Distinct Bigrams**: {}
- **Distinct Contexts**: {}
- **Pack Eligible Bigrams**: {}
- **Lexicon Coverage**: {:.2}%
- **Count Pruned Out**: {}
- **Capacity Pruned Out**: {}

## Determinism & Manifest
Generated artifacts are 100% reproducible and recorded in `artifacts.sha256`.
"#,
        summary.total_sentences,
        summary.total_bigram_tokens,
        summary.raw_distinct_bigrams,
        summary.pruned_distinct_bigrams,
        summary.distinct_context_count,
        summary.pack_eligible_bigrams,
        summary.lexicon_coverage_percent,
        count_pruned_out,
        cap_pruned_out
    );
    fs::write(stage_dir.join("README.md"), readme_content)
        .map_err(|e| format!("Failed to write README.md: {}", e))?;

    // 7. artifacts.sha256 manifest
    let build_bigrams_path = root.join("data/build/bigrams.jsonl");
    let bigrams_bytes = fs::read(&build_bigrams_path).map_err(|e| {
        format!(
            "Failed to read data/build/bigrams.jsonl for manifest: {}",
            e
        )
    })?;
    let bigrams_hash = format!("{:x}", Sha256::digest(&bigrams_bytes));

    let report_files = [
        "summary.json",
        "top-bigrams.json",
        "context-distribution.json",
        "out-of-vocabulary.json",
        "pruning-summary.json",
        "README.md",
    ];

    let mut manifest_content = format!("{}  data/build/bigrams.jsonl\n", bigrams_hash);
    for file in &report_files {
        let content = fs::read(stage_dir.join(file))
            .map_err(|e| format!("Failed to read report file {} for manifest: {}", file, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        manifest_content.push_str(&format!("{}  data/reports/ngrams/{}\n", hash, file));
    }

    fs::write(stage_dir.join("artifacts.sha256"), manifest_content)
        .map_err(|e| format!("Failed to write artifacts.sha256: {}", e))?;

    // Atomic backup and rollback swap
    let backup_dir = output_dir.with_extension(format!(
        "tmp_backup_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));

    if backup_dir.exists() {
        let _ = fs::remove_dir_all(&backup_dir);
    }

    if output_dir.exists() {
        fs::rename(&output_dir, &backup_dir).map_err(|e| {
            format!(
                "Failed to move output dir {:?} to backup: {}",
                output_dir, e
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
        Err(err) => {
            if backup_dir.exists() {
                let _ = fs::rename(&backup_dir, &output_dir);
            }
            Err(format!("Failed to install ngram reports: {}", err))
        }
    }
}
