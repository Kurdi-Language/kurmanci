//! Deterministic Bigram & Trigram Language Model Extraction, Sentence Segmentation,
//! Fixed-Point Probability Calculation, Pruning, and Report Suite.

use crate::corpus::registry::CorpusRegistry;
use crate::corpus::tokenizer::tokenize_text;
use crate::validate::SourceLexiconEntry;
use kurmanci_engine::format::{
    MAX_BIGRAM_PREDICTIONS_PER_CONTEXT, MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT, PROBABILITY_SCALE,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Config for bigram model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigramConfig {
    pub minimum_count: u64,
    pub maximum_predictions_per_context: usize,
}

impl Default for BigramConfig {
    fn default() -> Self {
        Self {
            minimum_count: 2,
            maximum_predictions_per_context: MAX_BIGRAM_PREDICTIONS_PER_CONTEXT,
        }
    }
}

/// Config for trigram model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrigramConfig {
    pub minimum_count: u64,
    pub maximum_predictions_per_context: usize,
}

impl Default for TrigramConfig {
    fn default() -> Self {
        Self {
            minimum_count: 2,
            maximum_predictions_per_context: MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT,
        }
    }
}

/// Pruning configuration loaded from `data-builder/config/ngrams.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NgramConfig {
    #[serde(default)]
    pub bigram: BigramConfig,
    #[serde(default)]
    pub trigram: TrigramConfig,
}

impl NgramConfig {
    pub fn load<P: AsRef<Path>>(root_dir: P) -> Result<Self, String> {
        let path = root_dir.as_ref().join("data-builder/config/ngrams.toml");

        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;

        toml::from_str(&content)
            .map_err(|e| format!("Invalid n-gram configuration {:?}: {}", path, e))
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

/// A single extracted and pruned trigram record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrigramRecord {
    pub previous_2: String,
    pub previous_1: String,
    pub next: String,
    pub count: u64,
    pub context_count: u64,
    pub probability_millionths: u32,
}

/// Overall statistical summary report for the bigram extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigramSummaryReport {
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

/// Overall statistical summary report for the trigram extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrigramSummaryReport {
    pub total_sentences: usize,
    pub total_trigram_tokens: usize,
    pub raw_distinct_trigrams: usize,
    pub pruned_distinct_trigrams: usize,
    pub distinct_context_count: usize,
    pub lexicon_coverage_percent: f64,
    pub pack_eligible_trigrams: usize,
    pub excluded_oov_previous_2: usize,
    pub excluded_oov_previous_1: usize,
    pub excluded_oov_next: usize,
}

/// Returned stats from `build_corpus_bigrams`.
#[derive(Debug, Clone)]
pub struct BigramBuildStats {
    pub total_sentences: usize,
    pub total_bigram_tokens: usize,
    pub records: Vec<BigramRecord>,
}

/// Returned stats from `build_corpus_trigrams`.
#[derive(Debug, Clone)]
pub struct TrigramBuildStats {
    pub total_sentences: usize,
    pub total_trigram_tokens: usize,
    pub records: Vec<TrigramRecord>,
}

/// Combined stats from `build_corpus_ngrams`.
#[derive(Debug, Clone)]
pub struct NgramBuildStats {
    pub bigram_stats: BigramBuildStats,
    pub trigram_stats: TrigramBuildStats,
}

/// Splits input document text into sentences using Unicode punctuation rules.
pub fn split_into_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        current.push(ch);
        if ch == '.' || ch == '!' || ch == '?' || ch == '…' || ch == '۔' {
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '.'
                    || next_ch == '!'
                    || next_ch == '?'
                    || next_ch == '…'
                    || next_ch == '۔'
                {
                    current.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            let trimmed = current.trim();
            if !trimmed.is_empty() && trimmed.chars().any(char::is_alphanumeric) {
                sentences.push(trimmed.to_string());
            }
            current.clear();
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() && trimmed.chars().any(char::is_alphanumeric) {
        sentences.push(trimmed.to_string());
    }

    sentences
}

/// Orchestrates both bigram and trigram extraction.
pub fn build_corpus_ngrams<P: AsRef<Path>>(root_dir: P) -> Result<NgramBuildStats, String> {
    let root = root_dir.as_ref();
    let bigram_stats = build_corpus_bigrams(root)?;
    let trigram_stats = build_corpus_trigrams(root)?;

    Ok(NgramBuildStats {
        bigram_stats,
        trigram_stats,
    })
}

/// Extracts, calculates probabilities, prunes, and formats bigram language models.
pub fn build_corpus_bigrams<P: AsRef<Path>>(root_dir: P) -> Result<BigramBuildStats, String> {
    let root = root_dir.as_ref();
    let config = NgramConfig::load(root)?;

    if config.bigram.minimum_count < 1 {
        return Err("bigram.minimum_count must be >= 1".to_string());
    }
    if config.bigram.maximum_predictions_per_context < 1
        || config.bigram.maximum_predictions_per_context > MAX_BIGRAM_PREDICTIONS_PER_CONTEXT
    {
        return Err(format!(
            "bigram.maximum_predictions_per_context must be between 1 and {}",
            MAX_BIGRAM_PREDICTIONS_PER_CONTEXT
        ));
    }

    let registry_path = root.join("data/source-registry/corpora.toml");
    let registry = CorpusRegistry::load_from_file(&registry_path)?;

    let mut registered_corpora = registry.corpora.clone();
    registered_corpora.sort_by(|a, b| a.corpus_id.cmp(&b.corpus_id));

    let mut corpus_texts = Vec::new();
    for corpus_entry in &registered_corpora {
        let imported_corpus_dir = root.join("data/imported").join(&corpus_entry.corpus_id);
        if !imported_corpus_dir.exists() {
            return Err(format!(
                "Imported corpus directory missing for '{}': {:?}",
                corpus_entry.corpus_id, imported_corpus_dir
            ));
        }

        for file_entry in &corpus_entry.files {
            let filename = Path::new(&file_entry.path)
                .file_name()
                .ok_or_else(|| format!("Invalid file path in corpus: {}", file_entry.path))?;
            let imported_file_path = imported_corpus_dir.join(filename);

            if !imported_file_path.exists() {
                return Err(format!(
                    "Imported corpus file missing for '{}': {:?}",
                    corpus_entry.corpus_id, imported_file_path
                ));
            }

            let mut file = File::open(&imported_file_path)
                .map_err(|e| format!("Failed to open {:?}: {}", imported_file_path, e))?;
            let mut hasher = Sha256::new();
            std::io::copy(&mut file, &mut hasher)
                .map_err(|e| format!("Failed to hash {:?}: {}", imported_file_path, e))?;
            let calculated_hash = format!("{:x}", hasher.finalize());

            if calculated_hash != file_entry.sha256 {
                return Err(format!(
                    "Corpus file checksum mismatch for {:?}: expected {}, calculated {}",
                    imported_file_path, file_entry.sha256, calculated_hash
                ));
            }

            let text = fs::read_to_string(&imported_file_path)
                .map_err(|e| format!("Failed to read {:?}: {}", imported_file_path, e))?;
            corpus_texts.push(text);
        }
    }

    let mut total_sentences = 0;
    let mut total_bigram_tokens = 0;
    let mut raw_counts: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut context_totals: BTreeMap<String, u64> = BTreeMap::new();

    for doc in &corpus_texts {
        let sentences = split_into_sentences(doc);
        total_sentences += sentences.len();

        for sentence in &sentences {
            let tokens = tokenize_text(sentence);
            if tokens.len() < 2 {
                continue;
            }

            for window in tokens.windows(2) {
                let prev = window[0].clone();
                let next = window[1].clone();

                total_bigram_tokens += 1;
                *raw_counts.entry((prev.clone(), next)).or_insert(0) += 1;
                *context_totals.entry(prev).or_insert(0) += 1;
            }
        }
    }

    let raw_distinct_bigrams = raw_counts.len();
    let mut unpruned_records = Vec::new();

    for ((prev, next), count) in raw_counts {
        let context_count = *context_totals
            .get(&prev)
            .ok_or_else(|| format!("Missing context count for word '{}'", prev))?;

        if count > context_count {
            return Err(format!(
                "Invalid bigram count ({}) exceeds context count ({}) for ({}, {})",
                count, context_count, prev, next
            ));
        }

        let numerator = u128::from(count)
            .checked_mul(u128::from(PROBABILITY_SCALE))
            .and_then(|val| val.checked_add(u128::from(context_count / 2)))
            .ok_or_else(|| format!("Bigram probability overflow for ({}, {})", prev, next))?;

        let prob = numerator / u128::from(context_count);
        let probability_millionths = u32::try_from(prob)
            .map_err(|_| format!("Probability conversion overflow for ({}, {})", prev, next))?;

        if probability_millionths > PROBABILITY_SCALE {
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

    let min_count = config.bigram.minimum_count;
    let max_per_ctx = config.bigram.maximum_predictions_per_context;

    let filtered_records: Vec<_> = unpruned_records
        .into_iter()
        .filter(|rec| rec.count >= min_count)
        .collect();

    let mut grouped: BTreeMap<String, Vec<BigramRecord>> = BTreeMap::new();
    for rec in filtered_records {
        grouped.entry(rec.previous.clone()).or_default().push(rec);
    }

    let mut pruned_records = Vec::new();
    for (_ctx, mut recs) in grouped {
        recs.sort_by(|a, b| {
            b.probability_millionths
                .cmp(&a.probability_millionths)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.next.cmp(&b.next))
        });
        recs.truncate(max_per_ctx);
        pruned_records.extend(recs);
    }

    pruned_records.sort_by(|a, b| {
        a.previous
            .cmp(&b.previous)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.next.cmp(&b.next))
    });

    write_ngram_outputs(
        root,
        &pruned_records,
        &config,
        total_sentences,
        total_bigram_tokens,
        raw_distinct_bigrams,
    )?;

    Ok(BigramBuildStats {
        total_sentences,
        total_bigram_tokens,
        records: pruned_records,
    })
}

/// Extracts, calculates probabilities, prunes, and formats trigram language models.
pub fn build_corpus_trigrams<P: AsRef<Path>>(root_dir: P) -> Result<TrigramBuildStats, String> {
    let root = root_dir.as_ref();
    let config = NgramConfig::load(root)?;

    if config.trigram.minimum_count < 1 {
        return Err("trigram.minimum_count must be >= 1".to_string());
    }
    if config.trigram.maximum_predictions_per_context < 1
        || config.trigram.maximum_predictions_per_context > MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT
    {
        return Err(format!(
            "trigram.maximum_predictions_per_context must be between 1 and {}",
            MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT
        ));
    }

    let registry_path = root.join("data/source-registry/corpora.toml");
    let registry = CorpusRegistry::load_from_file(&registry_path)?;

    let mut registered_corpora = registry.corpora.clone();
    registered_corpora.sort_by(|a, b| a.corpus_id.cmp(&b.corpus_id));

    let mut corpus_texts = Vec::new();
    for corpus_entry in &registered_corpora {
        let imported_corpus_dir = root.join("data/imported").join(&corpus_entry.corpus_id);
        if !imported_corpus_dir.exists() {
            return Err(format!(
                "Imported corpus directory missing for '{}': {:?}",
                corpus_entry.corpus_id, imported_corpus_dir
            ));
        }

        for file_entry in &corpus_entry.files {
            let filename = Path::new(&file_entry.path)
                .file_name()
                .ok_or_else(|| format!("Invalid file path in corpus: {}", file_entry.path))?;
            let imported_file_path = imported_corpus_dir.join(filename);

            if !imported_file_path.exists() {
                return Err(format!(
                    "Imported corpus file missing for '{}': {:?}",
                    corpus_entry.corpus_id, imported_file_path
                ));
            }

            let mut file = File::open(&imported_file_path)
                .map_err(|e| format!("Failed to open {:?}: {}", imported_file_path, e))?;
            let mut hasher = Sha256::new();
            std::io::copy(&mut file, &mut hasher)
                .map_err(|e| format!("Failed to hash {:?}: {}", imported_file_path, e))?;
            let calculated_hash = format!("{:x}", hasher.finalize());

            if calculated_hash != file_entry.sha256 {
                return Err(format!(
                    "Corpus file checksum mismatch for {:?}: expected {}, calculated {}",
                    imported_file_path, file_entry.sha256, calculated_hash
                ));
            }

            let text = fs::read_to_string(&imported_file_path)
                .map_err(|e| format!("Failed to read {:?}: {}", imported_file_path, e))?;
            corpus_texts.push(text);
        }
    }

    let mut total_sentences = 0;
    let mut total_trigram_tokens = 0;
    let mut raw_counts: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    let mut context_totals: BTreeMap<(String, String), u64> = BTreeMap::new();

    for doc in &corpus_texts {
        let sentences = split_into_sentences(doc);
        total_sentences += sentences.len();

        for sentence in &sentences {
            let tokens = tokenize_text(sentence);
            if tokens.len() < 3 {
                continue;
            }

            for window in tokens.windows(3) {
                let prev2 = window[0].clone();
                let prev1 = window[1].clone();
                let next = window[2].clone();

                total_trigram_tokens += 1;
                *raw_counts
                    .entry((prev2.clone(), prev1.clone(), next))
                    .or_insert(0) += 1;
                *context_totals.entry((prev2, prev1)).or_insert(0) += 1;
            }
        }
    }

    let raw_distinct_trigrams = raw_counts.len();
    let mut unpruned_records = Vec::new();

    for ((prev2, prev1, next), count) in raw_counts {
        let context_count = *context_totals
            .get(&(prev2.clone(), prev1.clone()))
            .ok_or_else(|| format!("Missing context count for pair ('{}', '{}')", prev2, prev1))?;

        if context_count == 0 {
            return Err(format!(
                "Invalid zero context count for trigram ({}, {}, {})",
                prev2, prev1, next
            ));
        }
        if count == 0 {
            return Err(format!(
                "Invalid zero count for trigram ({}, {}, {})",
                prev2, prev1, next
            ));
        }
        if count > context_count {
            return Err(format!(
                "Invalid trigram count ({}) exceeds context count ({}) for ({}, {}, {})",
                count, context_count, prev2, prev1, next
            ));
        }

        let numerator = u128::from(count)
            .checked_mul(u128::from(PROBABILITY_SCALE))
            .and_then(|val| val.checked_add(u128::from(context_count / 2)))
            .ok_or_else(|| {
                format!(
                    "Trigram probability overflow for ({}, {}, {})",
                    prev2, prev1, next
                )
            })?;

        let prob = numerator / u128::from(context_count);
        let probability_millionths = u32::try_from(prob).map_err(|_| {
            format!(
                "Probability conversion overflow for ({}, {}, {})",
                prev2, prev1, next
            )
        })?;

        if probability_millionths > PROBABILITY_SCALE {
            return Err(format!(
                "Probability {} exceeds 1,000,000 for ({}, {}, {})",
                probability_millionths, prev2, prev1, next
            ));
        }

        unpruned_records.push(TrigramRecord {
            previous_2: prev2,
            previous_1: prev1,
            next,
            count,
            context_count,
            probability_millionths,
        });
    }

    let min_count = config.trigram.minimum_count;
    let max_per_ctx = config.trigram.maximum_predictions_per_context;

    let filtered_records: Vec<_> = unpruned_records
        .into_iter()
        .filter(|rec| rec.count >= min_count)
        .collect();

    let mut grouped: BTreeMap<(String, String), Vec<TrigramRecord>> = BTreeMap::new();
    for rec in filtered_records {
        grouped
            .entry((rec.previous_2.clone(), rec.previous_1.clone()))
            .or_default()
            .push(rec);
    }

    let mut pruned_records = Vec::new();
    for (_ctx, mut recs) in grouped {
        recs.sort_by(|a, b| {
            b.probability_millionths
                .cmp(&a.probability_millionths)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.next.cmp(&b.next))
        });
        recs.truncate(max_per_ctx);
        pruned_records.extend(recs);
    }

    pruned_records.sort_by(|a, b| {
        a.previous_2
            .cmp(&b.previous_2)
            .then_with(|| a.previous_1.cmp(&b.previous_1))
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.next.cmp(&b.next))
    });

    write_trigram_outputs(
        root,
        &pruned_records,
        &config,
        total_sentences,
        total_trigram_tokens,
        raw_distinct_trigrams,
    )?;

    Ok(TrigramBuildStats {
        total_sentences,
        total_trigram_tokens,
        records: pruned_records,
    })
}

fn write_ngram_outputs<P: AsRef<Path>>(
    root: P,
    records: &[BigramRecord],
    config: &NgramConfig,
    total_sentences: usize,
    total_bigram_tokens: usize,
    raw_distinct_bigrams: usize,
) -> Result<(), String> {
    let build_dir = root.as_ref().join("data/build");
    let reports_dir = root.as_ref().join("data/reports/ngrams");
    fs::create_dir_all(&build_dir)
        .map_err(|e| format!("Failed to create {:?}: {}", build_dir, e))?;
    fs::create_dir_all(&reports_dir)
        .map_err(|e| format!("Failed to create {:?}: {}", reports_dir, e))?;

    let bigrams_jsonl = build_dir.join("bigrams.jsonl");
    let mut file = File::create(&bigrams_jsonl)
        .map_err(|e| format!("Failed to create {:?}: {}", bigrams_jsonl, e))?;

    for rec in records {
        let json_line =
            serde_json::to_string(rec).map_err(|e| format!("Serialize error: {}", e))?;
        writeln!(file, "{}", json_line).map_err(|e| format!("Write error: {}", e))?;
    }

    let lexicon_path = root.as_ref().join("data/reviewed/lexicon.jsonl");
    let mut lexicon_normalized = BTreeSet::new();
    if lexicon_path.exists() {
        let lex_file = File::open(&lexicon_path).map_err(|e| format!("Open error: {}", e))?;
        for line in BufReader::new(lex_file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<SourceLexiconEntry>(&line) {
                lexicon_normalized.insert(entry.normalized);
            }
        }
    }

    let mut contexts = BTreeSet::new();
    let mut pack_eligible = 0;
    let mut excluded_oov_contexts = 0;
    let mut excluded_oov_predictions = 0;
    let mut oov_records = Vec::new();

    for rec in records {
        contexts.insert(rec.previous.clone());
        let prev_in_lex = lexicon_normalized.contains(&rec.previous);
        let next_in_lex = lexicon_normalized.contains(&rec.next);

        if prev_in_lex && next_in_lex {
            pack_eligible += 1;
        } else {
            if !prev_in_lex {
                excluded_oov_contexts += 1;
            }
            if !next_in_lex {
                excluded_oov_predictions += 1;
            }
            oov_records.push(rec.clone());
        }
    }

    let lex_coverage = if !lexicon_normalized.is_empty() {
        (contexts.len() as f64 / lexicon_normalized.len() as f64) * 100.0
    } else {
        0.0
    };

    let summary = BigramSummaryReport {
        total_sentences,
        total_bigram_tokens,
        raw_distinct_bigrams,
        pruned_distinct_bigrams: records.len(),
        distinct_context_count: contexts.len(),
        lexicon_coverage_percent: lex_coverage,
        pack_eligible_bigrams: pack_eligible,
        excluded_oov_contexts,
        excluded_oov_predictions,
    };

    write_json(&reports_dir.join("summary.json"), &summary)?;

    let mut top_bigrams = records.to_vec();
    top_bigrams.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.previous.cmp(&b.previous))
    });
    top_bigrams.truncate(50);
    write_json(&reports_dir.join("top-bigrams.json"), &top_bigrams)?;

    let mut context_counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in records {
        *context_counts.entry(rec.previous.clone()).or_insert(0) += 1;
    }
    write_json(
        &reports_dir.join("context-distribution.json"),
        &context_counts,
    )?;

    write_json(&reports_dir.join("out-of-vocabulary.json"), &oov_records)?;

    let count_pruned_out = raw_distinct_bigrams.saturating_sub(records.len());
    let pruning_summary = serde_json::json!({
        "raw_distinct_bigrams": raw_distinct_bigrams,
        "pruned_distinct_bigrams": records.len(),
        "minimum_count_threshold": config.bigram.minimum_count,
        "maximum_predictions_per_context": config.bigram.maximum_predictions_per_context,
        "total_records_pruned": count_pruned_out,
        "count_pruned_out": count_pruned_out,
        "capacity_pruned_out": 0
    });
    write_json(&reports_dir.join("pruning-summary.json"), &pruning_summary)?;

    let readme = format!(
        "# Kurmancî Bigram Extraction Report\n\n\
        - **Total Sentences Processed**: {}\n\
        - **Total Bigram Tokens**: {}\n\
        - **Raw Distinct Bigrams**: {}\n\
        - **Pruned Retained Bigrams**: {}\n\
        - **Distinct Context Words**: {}\n\
        - **Lexicon Coverage**: {:.2}%\n\
        - **Pack-Eligible Bigrams**: {}\n\
        - **Excluded OOV Contexts**: {}\n\
        - **Excluded OOV Predictions**: {}\n",
        total_sentences,
        total_bigram_tokens,
        raw_distinct_bigrams,
        records.len(),
        contexts.len(),
        lex_coverage,
        pack_eligible,
        excluded_oov_contexts,
        excluded_oov_predictions
    );
    fs::write(reports_dir.join("README.md"), readme)
        .map_err(|e| format!("Write README.md error: {}", e))?;

    generate_manifest(&reports_dir, &bigrams_jsonl)?;

    Ok(())
}

fn write_trigram_outputs<P: AsRef<Path>>(
    root: P,
    records: &[TrigramRecord],
    config: &NgramConfig,
    total_sentences: usize,
    total_trigram_tokens: usize,
    raw_distinct_trigrams: usize,
) -> Result<(), String> {
    let build_dir = root.as_ref().join("data/build");
    let reports_dir = root.as_ref().join("data/reports/trigrams");
    fs::create_dir_all(&build_dir)
        .map_err(|e| format!("Failed to create {:?}: {}", build_dir, e))?;
    fs::create_dir_all(&reports_dir)
        .map_err(|e| format!("Failed to create {:?}: {}", reports_dir, e))?;

    let trigrams_jsonl = build_dir.join("trigrams.jsonl");
    let mut file = File::create(&trigrams_jsonl)
        .map_err(|e| format!("Failed to create {:?}: {}", trigrams_jsonl, e))?;

    for rec in records {
        let json_line =
            serde_json::to_string(rec).map_err(|e| format!("Serialize error: {}", e))?;
        writeln!(file, "{}", json_line).map_err(|e| format!("Write error: {}", e))?;
    }

    let lexicon_path = root.as_ref().join("data/reviewed/lexicon.jsonl");
    let mut lexicon_normalized = BTreeSet::new();
    if lexicon_path.exists() {
        let lex_file = File::open(&lexicon_path).map_err(|e| format!("Open error: {}", e))?;
        for line in BufReader::new(lex_file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<SourceLexiconEntry>(&line) {
                lexicon_normalized.insert(entry.normalized);
            }
        }
    }

    let mut contexts = BTreeSet::new();
    let mut pack_eligible = 0;
    let mut excluded_oov_prev2 = 0;
    let mut excluded_oov_prev1 = 0;
    let mut excluded_oov_next = 0;
    let mut oov_records = Vec::new();

    for rec in records {
        contexts.insert((rec.previous_2.clone(), rec.previous_1.clone()));
        let prev2_in_lex = lexicon_normalized.contains(&rec.previous_2);
        let prev1_in_lex = lexicon_normalized.contains(&rec.previous_1);
        let next_in_lex = lexicon_normalized.contains(&rec.next);

        if prev2_in_lex && prev1_in_lex && next_in_lex {
            pack_eligible += 1;
        } else {
            if !prev2_in_lex {
                excluded_oov_prev2 += 1;
            }
            if !prev1_in_lex {
                excluded_oov_prev1 += 1;
            }
            if !next_in_lex {
                excluded_oov_next += 1;
            }
            oov_records.push(rec.clone());
        }
    }

    let lex_coverage = if !lexicon_normalized.is_empty() {
        (contexts.len() as f64 / lexicon_normalized.len() as f64) * 100.0
    } else {
        0.0
    };

    let summary = TrigramSummaryReport {
        total_sentences,
        total_trigram_tokens,
        raw_distinct_trigrams,
        pruned_distinct_trigrams: records.len(),
        distinct_context_count: contexts.len(),
        lexicon_coverage_percent: lex_coverage,
        pack_eligible_trigrams: pack_eligible,
        excluded_oov_previous_2: excluded_oov_prev2,
        excluded_oov_previous_1: excluded_oov_prev1,
        excluded_oov_next,
    };

    write_json(&reports_dir.join("summary.json"), &summary)?;

    let mut top_trigrams = records.to_vec();
    top_trigrams.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.previous_2.cmp(&b.previous_2))
    });
    top_trigrams.truncate(50);
    write_json(&reports_dir.join("top-trigrams.json"), &top_trigrams)?;

    let mut context_counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in records {
        let key = format!("{} {}", rec.previous_2, rec.previous_1);
        *context_counts.entry(key).or_insert(0) += 1;
    }
    write_json(
        &reports_dir.join("context-distribution.json"),
        &context_counts,
    )?;

    write_json(&reports_dir.join("out-of-vocabulary.json"), &oov_records)?;

    let count_pruned_out = raw_distinct_trigrams.saturating_sub(records.len());
    let pruning_summary = serde_json::json!({
        "raw_distinct_trigrams": raw_distinct_trigrams,
        "pruned_distinct_trigrams": records.len(),
        "minimum_count_threshold": config.trigram.minimum_count,
        "maximum_predictions_per_context": config.trigram.maximum_predictions_per_context,
        "total_records_pruned": count_pruned_out,
        "count_pruned_out": count_pruned_out,
        "capacity_pruned_out": 0
    });
    write_json(&reports_dir.join("pruning-summary.json"), &pruning_summary)?;

    let readme = format!(
        "# Kurmancî Trigram Extraction Report\n\n\
        - **Total Sentences Processed**: {}\n\
        - **Total Trigram Tokens**: {}\n\
        - **Raw Distinct Trigrams**: {}\n\
        - **Pruned Retained Trigrams**: {}\n\
        - **Distinct Context Pairs**: {}\n\
        - **Lexicon Coverage**: {:.2}%\n\
        - **Pack-Eligible Trigrams**: {}\n\
        - **Excluded OOV Previous 2**: {}\n\
        - **Excluded OOV Previous 1**: {}\n\
        - **Excluded OOV Predictions**: {}\n",
        total_sentences,
        total_trigram_tokens,
        raw_distinct_trigrams,
        records.len(),
        contexts.len(),
        lex_coverage,
        pack_eligible,
        excluded_oov_prev2,
        excluded_oov_prev1,
        excluded_oov_next
    );
    fs::write(reports_dir.join("README.md"), readme)
        .map_err(|e| format!("Write README.md error: {}", e))?;

    generate_manifest(&reports_dir, &trigrams_jsonl)?;

    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json_bytes =
        serde_json::to_vec_pretty(value).map_err(|e| format!("JSON serialize error: {}", e))?;
    fs::write(path, json_bytes).map_err(|e| format!("Write error for {:?}: {}", path, e))
}

fn generate_manifest(reports_dir: &Path, primary_artifact_path: &Path) -> Result<(), String> {
    let manifest_path = reports_dir.join("artifacts.sha256");
    let root = reports_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or_else(|| "Failed to find workspace root".to_string())?;

    let mut file_entries = Vec::new();
    let read_dir = fs::read_dir(reports_dir)
        .map_err(|e| format!("Read dir error {:?}: {}", reports_dir, e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
        let path = entry.path();
        if path.is_file() && path.file_name().is_some_and(|n| n != "artifacts.sha256") {
            let rel_path = path
                .strip_prefix(root)
                .map_err(|_| "Failed strip prefix")?
                .to_str()
                .ok_or_else(|| "UTF8 path conversion error".to_string())?
                .to_string();

            let bytes =
                fs::read(&path).map_err(|e| format!("Read file error {:?}: {}", path, e))?;
            let hash = format!("{:x}", Sha256::digest(&bytes));
            file_entries.push((rel_path, hash));
        }
    }

    if primary_artifact_path.exists() {
        let rel_path = primary_artifact_path
            .strip_prefix(root)
            .map_err(|_| "Failed strip prefix")?
            .to_str()
            .ok_or_else(|| "UTF8 path conversion error".to_string())?
            .to_string();
        let bytes = fs::read(primary_artifact_path)
            .map_err(|e| format!("Read file error {:?}: {}", primary_artifact_path, e))?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        file_entries.push((rel_path, hash));
    }

    file_entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut manifest_content = String::new();
    for (rel_path, hash) in file_entries {
        manifest_content.push_str(&format!("{}  {}\n", hash, rel_path));
    }

    fs::write(&manifest_path, manifest_content)
        .map_err(|e| format!("Write manifest error {:?}: {}", manifest_path, e))
}
