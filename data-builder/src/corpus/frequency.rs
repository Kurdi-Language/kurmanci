//! Frequency Builder module for compiling token and document frequency statistics.

use super::partition::PartitionDocumentRecord;
use super::registry::CorpusRegistry;
use super::tokenizer::tokenize_text;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

/// A single frequency record in `frequencies.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrequencyRecord {
    pub word: String,
    pub token_count: usize,
    pub document_count: usize,
    pub normalized_frequency: f64,
    pub zipf: f64,
}

/// Statistics collected during frequency building across all corpus files.
#[derive(Debug, Clone)]
pub struct FrequencyBuildStats {
    pub total_documents: usize,
    pub total_tokens: usize,
    pub records: Vec<FrequencyRecord>,
}

/// Builds word frequency statistics across imported text corpora and writes `frequencies.jsonl`.
pub fn build_corpus_frequencies<P: AsRef<Path>>(
    root_dir: P,
) -> Result<FrequencyBuildStats, String> {
    let root = root_dir.as_ref();
    let registry_path = root.join("data/source-registry/corpora.toml");

    if !registry_path.exists() {
        return Err(format!("Corpus registry missing at {:?}", registry_path));
    }

    let registry = CorpusRegistry::load_from_file(&registry_path)?;

    // Sort registered corpora deterministically by corpus_id
    let mut registered_corpora = registry.corpora.clone();
    registered_corpora.sort_by(|a, b| a.corpus_id.cmp(&b.corpus_id));

    let mut total_documents = 0usize;
    let mut total_tokens = 0usize;

    // Track (token_count, document_count) per word
    let mut token_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut doc_counts: BTreeMap<String, usize> = BTreeMap::new();

    for corpus_entry in &registered_corpora {
        let imported_corpus_dir = root.join("data/imported").join(&corpus_entry.corpus_id);
        if !imported_corpus_dir.exists() {
            // Corpus has not been imported yet, skip
            continue;
        }

        // Process ONLY files explicitly declared in corpora.toml for this corpus
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

            // Verify checksum before processing
            let mut f = File::open(&imported_file_path).map_err(|e| {
                format!(
                    "Failed to open imported file {:?}: {}",
                    imported_file_path, e
                )
            })?;
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 8192];
            loop {
                let n = f
                    .read(&mut buffer)
                    .map_err(|e| format!("Error reading {:?}: {}", imported_file_path, e))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            let computed = format!("{:x}", hasher.finalize());
            if computed != file_entry.sha256 {
                return Err(format!(
                    "Checksum verification failed for imported file {:?} in corpus '{}': expected {}, got {}",
                    imported_file_path, corpus_entry.corpus_id, file_entry.sha256, computed
                ));
            }

            // Read line by line (line-delimited document format)
            let file = File::open(&imported_file_path)
                .map_err(|e| format!("Failed to open {:?}: {}", imported_file_path, e))?;
            let reader = BufReader::new(file);

            for line_res in reader.lines() {
                let line = line_res
                    .map_err(|e| format!("Read error in {:?}: {}", imported_file_path, e))?;
                if line.trim().is_empty() {
                    continue;
                }

                total_documents += 1;
                let tokens = tokenize_text(&line);
                total_tokens += tokens.len();

                let mut unique_in_doc = BTreeSet::new();
                for token in tokens {
                    *token_counts.entry(token.clone()).or_insert(0) += 1;
                    unique_in_doc.insert(token);
                }

                for token in unique_in_doc {
                    *doc_counts.entry(token).or_insert(0) += 1;
                }
            }
        }
    }

    if total_tokens == 0 {
        return Err("No tokens were parsed from imported corpora".to_string());
    }

    // Build frequency records
    let mut records: Vec<FrequencyRecord> = token_counts
        .into_iter()
        .map(|(word, count)| {
            let d_count = *doc_counts.get(&word).unwrap_or(&0);
            let norm_freq = count as f64 / total_tokens as f64;
            // Zipf = log10(norm_freq * 1e9) = log10(count_per_billion)
            let raw_zipf = (norm_freq * 1e9).log10();
            let zipf = (raw_zipf * 100.0).round() / 100.0;

            FrequencyRecord {
                word,
                token_count: count,
                document_count: d_count,
                normalized_frequency: norm_freq,
                zipf,
            }
        })
        .collect();

    // Deterministic sorting: descending token_count, ascending word
    records.sort_by(|a, b| {
        b.token_count
            .cmp(&a.token_count)
            .then_with(|| a.word.cmp(&b.word))
    });

    // Write data/build/frequencies.jsonl
    let build_dir = root.join("data/build");
    fs::create_dir_all(&build_dir)
        .map_err(|e| format!("Failed to create build dir {:?}: {}", build_dir, e))?;

    let frequencies_path = build_dir.join("frequencies.jsonl");
    let mut file = File::create(&frequencies_path)
        .map_err(|e| format!("Failed to create {:?}: {}", frequencies_path, e))?;

    for record in &records {
        let line = serde_json::to_string(record)
            .map_err(|e| format!("Failed to serialize frequency record: {}", e))?;
        writeln!(file, "{}", line)
            .map_err(|e| format!("Failed to write to {:?}: {}", frequencies_path, e))?;
    }

    let stats = FrequencyBuildStats {
        total_documents,
        total_tokens,
        records,
    };

    // Write frequency analysis reports
    super::reports::write_all_frequency_reports(root, &stats)?;

    Ok(stats)
}

/// Builds word frequency statistics strictly from the TRAIN partition (`train.jsonl`),
/// including ONLY canonical duplicate representatives (`corpus_id == canonical_corpus_id && document_id == canonical_document_id`).
pub fn build_corpus_train_frequencies<P: AsRef<Path>>(
    root_dir: P,
) -> Result<FrequencyBuildStats, String> {
    let root = root_dir.as_ref();
    let partition_dir = root.join("data/build/corpus-partitions");
    let manifest_path = partition_dir.join("manifest.json");

    use super::audit::{DOCUMENT_DEDUP_VERSION, SENTENCE_DEDUP_VERSION};
    use super::importer::{verify_canonical_manifest, CANONICAL_SCHEMA_VERSION};
    use super::partition::{PartitionBuildManifest, PARTITION_POLICY_VERSION};

    // 1. Verify canonical manifest provenance against source registry and canonical documents
    let _canonical_manifest = verify_canonical_manifest(root)
        .map_err(|e| format!("Canonical manifest verification failed: {}", e))?;

    let manifest_bytes = fs::read(&manifest_path).map_err(|e| {
        format!(
            "Failed to read partition manifest at {:?}: {}",
            manifest_path, e
        )
    })?;
    let manifest: PartitionBuildManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| {
            format!(
                "Failed to parse partition manifest at {:?}: {}",
                manifest_path, e
            )
        })?;

    if manifest.partition_policy_version != PARTITION_POLICY_VERSION {
        return Err(format!(
            "Partition policy version mismatch: expected {}, found {}",
            PARTITION_POLICY_VERSION, manifest.partition_policy_version
        ));
    }
    if manifest.canonical_schema_version != CANONICAL_SCHEMA_VERSION {
        return Err(format!(
            "Canonical schema version mismatch: expected {}, found {}",
            CANONICAL_SCHEMA_VERSION, manifest.canonical_schema_version
        ));
    }
    if manifest.document_normalization_version != DOCUMENT_DEDUP_VERSION {
        return Err(format!(
            "Document normalization version mismatch: expected {}, found {}",
            DOCUMENT_DEDUP_VERSION, manifest.document_normalization_version
        ));
    }
    if manifest.sentence_normalization_version != SENTENCE_DEDUP_VERSION {
        return Err(format!(
            "Sentence normalization version mismatch: expected {}, found {}",
            SENTENCE_DEDUP_VERSION, manifest.sentence_normalization_version
        ));
    }

    let canonical_manifest_path = root.join("data/imported-canonical/manifest.json");
    if !canonical_manifest_path.exists() {
        return Err(format!(
            "Canonical input manifest missing at {:?}. Cannot verify partition provenance.",
            canonical_manifest_path
        ));
    }
    let canonical_manifest_bytes = fs::read(&canonical_manifest_path).map_err(|e| {
        format!(
            "Failed to read canonical input manifest {:?}: {}",
            canonical_manifest_path, e
        )
    })?;
    let actual_canonical_sha256 = format!("{:x}", Sha256::digest(&canonical_manifest_bytes));
    if actual_canonical_sha256 != manifest.canonical_input_manifest_sha256 {
        return Err(format!(
            "Canonical input manifest SHA-256 mismatch in partition manifest: expected {}, found {}",
            manifest.canonical_input_manifest_sha256, actual_canonical_sha256
        ));
    }

    let train_path = partition_dir.join("train.jsonl");
    if !train_path.exists() {
        return Err(format!("Train partition file missing at {:?}", train_path));
    }

    let file = File::open(&train_path)
        .map_err(|e| format!("Failed to open train partition {:?}: {}", train_path, e))?;
    let reader = BufReader::new(file);

    let mut train_records_read = 0usize;
    let mut total_documents = 0usize;
    let mut total_tokens = 0usize;

    let mut token_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut doc_counts: BTreeMap<String, usize> = BTreeMap::new();

    for (l_idx, line_res) in reader.lines().enumerate() {
        let line = line_res
            .map_err(|e| format!("Read error in {:?} line {}: {}", train_path, l_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }

        let record: PartitionDocumentRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON error in {:?} line {}: {}", train_path, l_idx + 1, e))?;

        if record.partition != "train" {
            return Err(format!(
                "Train partition record invalid: expected partition 'train', found '{}'",
                record.partition
            ));
        }
        train_records_read += 1;

        // INVARIANT: Include ONLY canonical duplicate representatives
        if record.corpus_id != record.canonical_corpus_id
            || record.document_id != record.canonical_document_id
        {
            continue;
        }

        total_documents += 1;
        let tokens = tokenize_text(&record.text);
        total_tokens += tokens.len();

        let mut unique_in_doc = BTreeSet::new();
        for token in tokens {
            *token_counts.entry(token.clone()).or_insert(0) += 1;
            unique_in_doc.insert(token);
        }

        for token in unique_in_doc {
            *doc_counts.entry(token).or_insert(0) += 1;
        }
    }

    if train_records_read != manifest.train_documents {
        return Err(format!(
            "Train records count mismatch: manifest expected {}, read {} records",
            manifest.train_documents, train_records_read
        ));
    }

    if total_tokens == 0 {
        return Err("No tokens were parsed from train partition".to_string());
    }

    let mut records: Vec<FrequencyRecord> = token_counts
        .into_iter()
        .map(|(word, count)| {
            let d_count = *doc_counts.get(&word).unwrap_or(&0);
            let norm_freq = count as f64 / total_tokens as f64;
            let raw_zipf = (norm_freq * 1e9).log10();
            let zipf = (raw_zipf * 100.0).round() / 100.0;

            FrequencyRecord {
                word,
                token_count: count,
                document_count: d_count,
                normalized_frequency: norm_freq,
                zipf,
            }
        })
        .collect();

    records.sort_by(|a, b| {
        b.token_count
            .cmp(&a.token_count)
            .then_with(|| a.word.cmp(&b.word))
    });

    let build_dir = root.join("data/build");
    fs::create_dir_all(&build_dir)
        .map_err(|e| format!("Failed to create build dir {:?}: {}", build_dir, e))?;

    let frequencies_path = build_dir.join("frequencies.jsonl");
    let mut file = File::create(&frequencies_path)
        .map_err(|e| format!("Failed to create {:?}: {}", frequencies_path, e))?;

    for record in &records {
        let line = serde_json::to_string(record)
            .map_err(|e| format!("Failed to serialize frequency record: {}", e))?;
        writeln!(file, "{}", line)
            .map_err(|e| format!("Failed to write to {:?}: {}", frequencies_path, e))?;
    }

    let stats = FrequencyBuildStats {
        total_documents,
        total_tokens,
        records,
    };

    super::reports::write_all_frequency_reports(root, &stats)?;

    Ok(stats)
}
