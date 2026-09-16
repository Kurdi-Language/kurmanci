//! Committed language-model artifacts (`language-model-v1`) for controlled packs.
//!
//! A language model is a small, git-tracked, **non-prose** derivative of exactly one
//! registered corpus: vocabulary-restricted unigram frequencies and bigram/trigram statistics
//! computed from the TRAIN-partition canonical representatives of that corpus only, stored as
//! numeric tables over deterministic vocabulary ids. No word sequence from the corpus is
//! written in readable form; only single vocabulary words (which are already published in the
//! packs) appear as text, in `vocabulary.txt`.
//!
//! Layout of `data/language-model/<model_id>/`:
//! - `vocabulary.txt`   one normalized vocabulary word per line, sorted; line index = id
//! - `unigrams.tsv`     `id \t token_count \t document_count \t zipf_milli`
//! - `bigrams.tsv`      `prev_id \t next_id \t count \t context_count \t probability_millionths`
//! - `trigrams.tsv`     `prev2_id \t prev1_id \t next_id \t count \t context_count \t probability_millionths`
//! - `manifest.json`    provenance (corpus, dump, partition, corpus-scoped TRAIN document set,
//!   frequency and n-gram hashes, license/attribution snapshot) and per-file hashes
//! - `artifacts.sha256` verified fail-closed by every consumer
//!
//! Corpus scoping: `build-language-model --corpus-id <id>` reads the global TRAIN partition
//! but keeps only records whose `corpus_id` is the requested corpus **and** that are the
//! canonical representative of their duplicate group (the same rule as every other TRAIN
//! consumer). Other registered corpora never contribute, so the licensing snapshot recorded in
//! the manifest describes every source of the statistics. The selected document set, the
//! derived frequency and n-gram tables and the intermediate build manifest (all written under
//! `data/build/language-model/<corpus_id>/`, git-ignored) are pinned by SHA-256 in the model
//! manifest. DEV/EVAL partitions never contribute.
//!
//! The model vocabulary is the union vocabulary of the three authoritative packs at build
//! time; loading verifies that the committed fingerprint still matches the current union, so a
//! stale model fails closed instead of compiling silently against a changed vocabulary.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::corpus::audit::{DOCUMENT_DEDUP_VERSION, SENTENCE_DEDUP_VERSION};
use crate::corpus::frequency::{frequency_records_from_counts, FrequencyRecord};
use crate::corpus::importer::{verify_canonical_manifest, CANONICAL_SCHEMA_VERSION};
use crate::corpus::ngrams::{split_into_sentences, BigramRecord, NgramConfig, TrigramRecord};
use crate::corpus::partition::{
    PartitionBuildManifest, PartitionDocumentRecord, PARTITION_POLICY_VERSION,
};
use crate::corpus::registry::CorpusRegistry;
use crate::corpus::tokenizer::tokenize_text;
use crate::corpus::train_ngrams::{
    compute_bigram_records, compute_trigram_records, probability_millionths,
};
use crate::pack::builder::resolve_authoritative_pack_lexicon;
use crate::validate::FrequencyMetadata;
use kurmanci_engine::format::{
    MAX_BIGRAM_PREDICTIONS_PER_CONTEXT, MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT, PROBABILITY_SCALE,
};

pub const LANGUAGE_MODEL_SCHEMA_VERSION: &str = "language-model-v1";
pub const LANGUAGE_MODEL_BUILD_SCHEMA_VERSION: &str = "language-model-build-v1";
pub const LANGUAGE_MODEL_DIR: &str = "data/language-model";
/// Git-ignored corpus-scoped intermediates of `build-language-model`.
pub const LANGUAGE_MODEL_BUILD_DIR: &str = "data/build/language-model";
pub const VOCABULARY_FILE: &str = "vocabulary.txt";
pub const UNIGRAMS_FILE: &str = "unigrams.tsv";
pub const BIGRAMS_FILE: &str = "bigrams.tsv";
pub const TRIGRAMS_FILE: &str = "trigrams.tsv";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const ARTIFACTS_FILE: &str = "artifacts.sha256";

/// Minimum n-gram counts a language model is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageModelBuildConfig {
    pub bigram_min_count: u64,
    pub trigram_min_count: u64,
}

/// The one production configuration: the `build-language-model` defaults and
/// `rebuild-production` both use it, so the committed model is reproducible from a single
/// definition.
pub const PRODUCTION_LANGUAGE_MODEL_BUILD: LanguageModelBuildConfig = LanguageModelBuildConfig {
    bigram_min_count: 2,
    trigram_min_count: 3,
};
pub const BUILD_MANIFEST_FILE: &str = "build-manifest.json";
pub const TRAIN_FREQUENCIES_FILE: &str = "train-frequencies.jsonl";
pub const TRAIN_BIGRAMS_FILE: &str = "train-bigrams.jsonl";
pub const TRAIN_TRIGRAMS_FILE: &str = "train-trigrams.jsonl";
/// Redistribution status recorded until a human licensing review decides otherwise.
pub const REDISTRIBUTION_PENDING_REVIEW: &str = "pending-review";

/// One file of the model directory with its SHA-256.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageModelFile {
    pub path: String,
    pub sha256: String,
}

/// Snapshot of the corpus licensing metadata the model derives from. Recorded, not judged:
/// whether statistical derivatives inherit the corpus licence obligations is an open
/// determination (`redistribution_determination`) until reviewed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageModelLicensing {
    pub corpus_name: String,
    /// Human-readable licence name from the corpus registry.
    pub license: String,
    /// Canonical SPDX identifier from the corpus registry (what packs record as `spdx`).
    pub license_spdx: String,
    pub license_url: String,
    pub attribution: String,
    pub source_url: String,
    pub redistribution_determination: String,
}

/// Corpus-scoped intermediate provenance written to
/// `data/build/language-model/<corpus_id>/build-manifest.json` by `build_language_model`.
/// Everything a model manifest pins about its statistics is recorded here first.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageModelBuildManifest {
    pub schema_version: String,
    pub corpus_id: String,
    pub corpus_version: String,
    pub corpus_registry_sha256: String,
    pub canonical_manifest_sha256: String,
    pub partition_manifest_sha256: String,
    pub train_partition_sha256: String,
    /// Number of records in the global TRAIN partition (all corpora), as read.
    pub partition_train_documents: usize,
    /// Canonical TRAIN representatives of this corpus only.
    pub train_document_count: usize,
    /// SHA-256 over the exact selected TRAIN records (one JSON line each, in partition order).
    pub train_document_set_sha256: String,
    pub train_token_count: u64,
    pub train_sentence_count: usize,
    pub ngram_config_sha256: String,
    pub bigram_min_count: u64,
    pub bigram_max_per_context: usize,
    pub trigram_min_count: u64,
    pub trigram_max_per_context: usize,
    pub frequency_records: usize,
    pub bigram_records: usize,
    pub trigram_records: usize,
    pub train_frequencies_sha256: String,
    pub train_bigrams_sha256: String,
    pub train_trigrams_sha256: String,
}

/// Provenance and content manifest of a committed language model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageModelManifest {
    pub schema_version: String,
    pub model_id: String,
    pub corpus_id: String,
    pub corpus_version: String,
    /// Every corpus whose text contributed to the statistics (exactly `[corpus_id]`).
    pub contributing_corpora: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus_source_artifact_sha256: Option<String>,
    pub corpus_documents_sha256: String,
    pub corpus_registry_sha256: String,
    pub canonical_manifest_sha256: String,
    pub partition_manifest_sha256: String,
    pub train_partition_sha256: String,
    pub train_document_count: usize,
    pub train_document_set_sha256: String,
    pub train_frequencies_sha256: String,
    pub train_bigrams_sha256: String,
    pub train_trigrams_sha256: String,
    pub build_manifest_sha256: String,
    pub ngram_config_sha256: String,
    pub licensing: LanguageModelLicensing,
    pub vocabulary_fingerprint: String,
    pub vocabulary_size: usize,
    pub unigram_count: usize,
    pub bigram_count: usize,
    pub trigram_count: usize,
    pub bigram_min_count: u64,
    pub trigram_min_count: u64,
    pub files: Vec<LanguageModelFile>,
}

/// A loaded, hash-verified language model ready for pack compilation.
#[derive(Debug, Clone)]
pub struct LanguageModel {
    pub manifest: LanguageModelManifest,
    pub manifest_sha256: String,
    pub unigrams: BTreeMap<String, FrequencyMetadata>,
    pub bigrams: Vec<BigramRecord>,
    pub trigrams: Vec<TrigramRecord>,
}

/// In-memory content handed to the writer (used by the builder and by tests).
#[derive(Debug, Clone, Default)]
pub struct LanguageModelContent {
    /// Sorted, unique, normalized vocabulary words; the index is the id.
    pub vocabulary: Vec<String>,
    /// `(id, metadata)` sorted by id.
    pub unigrams: Vec<(u32, FrequencyMetadata)>,
    /// `(prev_id, next_id, count, context_count, probability_millionths)` sorted.
    pub bigrams: Vec<(u32, u32, u64, u64, u32)>,
    /// `(prev2_id, prev1_id, next_id, count, context_count, probability_millionths)` sorted.
    pub trigrams: Vec<(u32, u32, u32, u64, u64, u32)>,
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn model_dir(root: &Path, model_id: &str) -> PathBuf {
    root.join(LANGUAGE_MODEL_DIR).join(model_id)
}

fn validate_model_id(model_id: &str) -> Result<(), String> {
    let ok = !model_id.is_empty()
        && model_id.len() <= 128
        && !model_id.starts_with('.')
        && model_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Invalid language model id '{}' (ASCII letters, digits, '-', '_', '.' only)",
            model_id
        ))
    }
}

/// SHA-256 over the vocabulary, one word per line.
pub fn vocabulary_fingerprint(vocabulary: &[String]) -> String {
    let mut hasher = Sha256::new();
    for w in vocabulary {
        hasher.update(w.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// A vocabulary entry must be one non-empty token: whitespace or control characters could
/// never match tokenizer output and would smuggle readable phrases into the artifact.
fn is_single_token(word: &str) -> bool {
    !word.is_empty() && !word.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Union of normalized forms across the three authoritative packs, sorted, single tokens only.
/// This is the vocabulary a model is restricted to, and the vocabulary every consumer checks
/// the committed model against.
pub fn authoritative_vocabulary(root: &Path) -> Result<Vec<String>, String> {
    let mut vocab: BTreeSet<String> = BTreeSet::new();
    for pack_id in ["seed", "reviewed", "experimental-full"] {
        for entry in resolve_authoritative_pack_lexicon(pack_id, root)? {
            if is_single_token(&entry.normalized) {
                vocab.insert(entry.normalized);
            }
        }
    }
    Ok(vocab.into_iter().collect())
}

/// Fingerprint of the current authoritative vocabulary (see `authoritative_vocabulary`).
pub fn authoritative_vocabulary_fingerprint(root: &Path) -> Result<String, String> {
    Ok(vocabulary_fingerprint(&authoritative_vocabulary(root)?))
}

fn frequency_metadata(record: &FrequencyRecord) -> Result<FrequencyMetadata, String> {
    if !record.zipf.is_finite() || record.zipf < 0.0 {
        return Err(format!(
            "Frequency record '{}': zipf {} is not usable",
            record.word, record.zipf
        ));
    }
    let milli = (record.zipf * 1000.0).round();
    if milli > u32::MAX as f64 {
        return Err(format!(
            "Frequency record '{}': zipf_milli overflow",
            record.word
        ));
    }
    Ok(FrequencyMetadata {
        token_count: record.token_count as u64,
        document_count: record.document_count as u64,
        zipf_milli: milli as u32,
    })
}

/// Semantic invariants of model content, enforced identically when writing and when loading:
/// single-token sorted unique vocabulary, ids inside the vocabulary, sorted unique record
/// keys, positive counts bounded by their context, and probabilities equal to the canonical
/// `count / context_count` rounding.
pub fn validate_language_model_content(content: &LanguageModelContent) -> Result<(), String> {
    if content.vocabulary.is_empty() {
        return Err("Language model vocabulary is empty".to_string());
    }
    if let Some(bad) = content.vocabulary.iter().find(|w| !is_single_token(w)) {
        return Err(format!(
            "Language model vocabulary entries must be single non-empty tokens without whitespace (found {:?})",
            bad
        ));
    }
    for w in content.vocabulary.windows(2) {
        if w[0] >= w[1] {
            return Err("Language model vocabulary must be sorted and unique".to_string());
        }
    }
    let n = content.vocabulary.len() as u64;
    let in_range = |id: u32| u64::from(id) < n;

    for w in content.unigrams.windows(2) {
        if w[0].0 >= w[1].0 {
            return Err("Language model unigrams must be sorted by unique id".to_string());
        }
    }
    for (id, m) in &content.unigrams {
        if !in_range(*id) {
            return Err(format!("Unigram id {} is outside the vocabulary", id));
        }
        if m.token_count == 0 || m.document_count == 0 || m.document_count > m.token_count {
            return Err(format!(
                "Unigram id {} has impossible counts (tokens {}, documents {})",
                id, m.token_count, m.document_count
            ));
        }
    }

    for w in content.bigrams.windows(2) {
        if (w[0].0, w[0].1) >= (w[1].0, w[1].1) {
            return Err("Language model bigrams must be sorted by unique id pair".to_string());
        }
    }
    for (a, b, count, context_count, prob) in &content.bigrams {
        if !in_range(*a) || !in_range(*b) {
            return Err(format!(
                "Bigram ({}, {}) references an id outside the vocabulary",
                a, b
            ));
        }
        let expected = probability_millionths(*count, *context_count, "bigram")?;
        if *prob != expected || *prob > PROBABILITY_SCALE {
            return Err(format!(
                "Bigram ({}, {}) probability {} does not match count {} / context {}",
                a, b, prob, count, context_count
            ));
        }
    }

    for w in content.trigrams.windows(2) {
        if (w[0].0, w[0].1, w[0].2) >= (w[1].0, w[1].1, w[1].2) {
            return Err("Language model trigrams must be sorted by unique id triple".to_string());
        }
    }
    for (a, b, c, count, context_count, prob) in &content.trigrams {
        if !in_range(*a) || !in_range(*b) || !in_range(*c) {
            return Err(format!(
                "Trigram ({}, {}, {}) references an id outside the vocabulary",
                a, b, c
            ));
        }
        let expected = probability_millionths(*count, *context_count, "trigram")?;
        if *prob != expected || *prob > PROBABILITY_SCALE {
            return Err(format!(
                "Trigram ({}, {}, {}) probability {} does not match count {} / context {}",
                a, b, c, prob, count, context_count
            ));
        }
    }
    Ok(())
}

/// Writes `data/language-model/<model_id>/` atomically (stage, swap, backup) from validated
/// numeric content, filling the vocabulary fingerprint, counts and per-file hashes of
/// `manifest`. The n-gram tables never contain a readable word.
pub fn write_language_model(
    root: &Path,
    manifest: LanguageModelManifest,
    content: &LanguageModelContent,
) -> Result<LanguageModelManifest, String> {
    validate_model_id(&manifest.model_id)?;
    validate_language_model_content(content)?;

    let target = model_dir(root, &manifest.model_id);
    let stage = root
        .join(LANGUAGE_MODEL_DIR)
        .join(format!(".{}.tmp-stage", manifest.model_id));
    let backup = root
        .join(LANGUAGE_MODEL_DIR)
        .join(format!(".{}.tmp-backup", manifest.model_id));
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| format!("Failed to clean {:?}: {}", stage, e))?;
    }
    fs::create_dir_all(&stage).map_err(|e| format!("Failed to create {:?}: {}", stage, e))?;

    let write_lines = |name: &str, lines: Vec<String>| -> Result<LanguageModelFile, String> {
        let path = stage.join(name);
        let mut f =
            File::create(&path).map_err(|e| format!("Failed to create {:?}: {}", path, e))?;
        for l in lines {
            f.write_all(l.as_bytes())
                .and_then(|_| f.write_all(b"\n"))
                .map_err(|e| format!("Write error {:?}: {}", path, e))?;
        }
        f.flush().map_err(|e| format!("Flush error: {}", e))?;
        Ok(LanguageModelFile {
            path: name.to_string(),
            sha256: sha256_file(&path)?,
        })
    };
    let mut files = vec![
        write_lines(VOCABULARY_FILE, content.vocabulary.clone())?,
        write_lines(
            UNIGRAMS_FILE,
            content
                .unigrams
                .iter()
                .map(|(id, m)| {
                    format!(
                        "{}\t{}\t{}\t{}",
                        id, m.token_count, m.document_count, m.zipf_milli
                    )
                })
                .collect(),
        )?,
        write_lines(
            BIGRAMS_FILE,
            content
                .bigrams
                .iter()
                .map(|(a, b, c, cc, p)| format!("{}\t{}\t{}\t{}\t{}", a, b, c, cc, p))
                .collect(),
        )?,
        write_lines(
            TRIGRAMS_FILE,
            content
                .trigrams
                .iter()
                .map(|(a, b, c, n, cc, p)| format!("{}\t{}\t{}\t{}\t{}\t{}", a, b, c, n, cc, p))
                .collect(),
        )?,
    ];
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let manifest = LanguageModelManifest {
        vocabulary_fingerprint: vocabulary_fingerprint(&content.vocabulary),
        vocabulary_size: content.vocabulary.len(),
        unigram_count: content.unigrams.len(),
        bigram_count: content.bigrams.len(),
        trigram_count: content.trigrams.len(),
        files,
        ..manifest
    };
    let manifest_path = stage.join(MANIFEST_FILE);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| format!("Failed to write {:?}: {}", manifest_path, e))?;
    let mut artifact_lines: Vec<String> = manifest
        .files
        .iter()
        .map(|f| format!("{}  {}", f.sha256, f.path))
        .collect();
    artifact_lines.push(format!(
        "{}  {}",
        sha256_file(&manifest_path)?,
        MANIFEST_FILE
    ));
    artifact_lines.sort();
    fs::write(stage.join(ARTIFACTS_FILE), artifact_lines.join("\n") + "\n")
        .map_err(|e| format!("Failed to write artifacts.sha256: {}", e))?;

    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|e| format!("Failed to clean {:?}: {}", backup, e))?;
    }
    if target.exists() {
        fs::rename(&target, &backup)
            .map_err(|e| format!("Failed to back up {:?}: {}", target, e))?;
    }
    if let Err(e) = fs::rename(&stage, &target) {
        if backup.exists() {
            let _ = fs::rename(&backup, &target);
        }
        return Err(format!("Failed to install {:?}: {}", target, e));
    }
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(manifest)
}

/// Loads the partition manifest and checks it against the current policy versions and the
/// canonical import it was built from.
fn verify_partition_manifest(
    root: &Path,
    canonical_manifest_sha256: &str,
) -> Result<PartitionBuildManifest, String> {
    let path = root.join("data/build/corpus-partitions/manifest.json");
    if !path.exists() {
        return Err("Partition outputs missing; run partition-corpora first".to_string());
    }
    let manifest: PartitionBuildManifest = serde_json::from_slice(
        &fs::read(&path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?,
    )
    .map_err(|e| format!("Failed to parse partition manifest {:?}: {}", path, e))?;
    if manifest.partition_policy_version != PARTITION_POLICY_VERSION
        || manifest.canonical_schema_version != CANONICAL_SCHEMA_VERSION
        || manifest.document_normalization_version != DOCUMENT_DEDUP_VERSION
        || manifest.sentence_normalization_version != SENTENCE_DEDUP_VERSION
    {
        return Err(
            "Partition manifest was produced by different partition/canonical/normalization policy versions; rerun partition-corpora"
                .to_string(),
        );
    }
    if manifest.canonical_input_manifest_sha256 != canonical_manifest_sha256 {
        return Err(format!(
            "Partition manifest canonical_input_manifest_sha256 {} does not match the canonical import on disk ({}); rerun partition-corpora",
            manifest.canonical_input_manifest_sha256, canonical_manifest_sha256
        ));
    }
    Ok(manifest)
}

fn write_jsonl<T: Serialize>(path: &Path, records: &[T]) -> Result<String, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create {:?}: {}", parent, e))?;
    }
    let mut f = File::create(path).map_err(|e| format!("Failed to create {:?}: {}", path, e))?;
    for r in records {
        let line = serde_json::to_string(r).map_err(|e| e.to_string())?;
        writeln!(f, "{}", line).map_err(|e| format!("Write error {:?}: {}", path, e))?;
    }
    f.flush().map_err(|e| format!("Flush error: {}", e))?;
    sha256_file(path)
}

/// Builds `data/language-model/<corpus_id>-<corpus_version>/` deterministically from the
/// canonical TRAIN representatives of `corpus_id` alone. Requires the corpus to be
/// registered, acquired, imported and partitioned. Writes the corpus-scoped intermediates
/// (`train-frequencies.jsonl`, `train-bigrams.jsonl`, `train-trigrams.jsonl`,
/// `build-manifest.json`) under `data/build/language-model/<corpus_id>/` and pins their
/// hashes in the model manifest. Fails closed on any provenance inconsistency.
pub fn build_language_model<P: AsRef<Path>>(
    root_dir: P,
    corpus_id: &str,
    bigram_min_count: u64,
    trigram_min_count: u64,
) -> Result<LanguageModelManifest, String> {
    let root = root_dir.as_ref();
    if bigram_min_count == 0 || trigram_min_count == 0 {
        return Err("Minimum n-gram counts must be at least 1".to_string());
    }

    // 1. Corpus registration and canonical import provenance.
    let registry_path = root.join("data/source-registry/corpora.toml");
    let registry = CorpusRegistry::load_from_file(&registry_path)?;
    let corpus = registry
        .find_corpus(corpus_id)
        .ok_or_else(|| format!("Corpus '{}' is not registered in corpora.toml", corpus_id))?;
    let corpus_documents_sha256 = corpus
        .files
        .first()
        .map(|f| f.sha256.clone())
        .ok_or_else(|| format!("Corpus '{}' registers no files", corpus_id))?;
    let corpus_registry_sha256 = sha256_file(&registry_path)?;
    let canonical = verify_canonical_manifest(root)?;
    if canonical
        .skipped_external_corpora
        .contains(&corpus_id.to_string())
        || !canonical.corpora.iter().any(|c| c.corpus_id == corpus_id)
    {
        return Err(format!(
            "Corpus '{}' is not part of the current canonical import; run `acquire-corpus {}` and `import-all-corpora` first",
            corpus_id, corpus_id
        ));
    }
    let canonical_manifest_path = root.join("data/imported-canonical/manifest.json");
    let canonical_manifest_sha256 = sha256_file(&canonical_manifest_path)?;

    // 2. Partition provenance and n-gram pruning configuration.
    let partition_manifest_path = root.join("data/build/corpus-partitions/manifest.json");
    let train_path = root.join("data/build/corpus-partitions/train.jsonl");
    let partition_manifest = verify_partition_manifest(root, &canonical_manifest_sha256)?;
    if !train_path.exists() {
        return Err(format!("Train partition file missing at {:?}", train_path));
    }
    let partition_manifest_sha256 = sha256_file(&partition_manifest_path)?;
    let train_partition_sha256 = sha256_file(&train_path)?;
    let config = NgramConfig::load(root)?;
    if config.bigram.maximum_predictions_per_context < 1
        || config.bigram.maximum_predictions_per_context > MAX_BIGRAM_PREDICTIONS_PER_CONTEXT
        || config.trigram.maximum_predictions_per_context < 1
        || config.trigram.maximum_predictions_per_context > MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT
    {
        return Err("n-gram maximum_predictions_per_context out of range".to_string());
    }
    let ngram_config_sha256 = sha256_file(&root.join("data-builder/config/ngrams.toml"))?;

    // 3. Corpus-scoped TRAIN document set: this corpus's canonical representatives only.
    let file = File::open(&train_path)
        .map_err(|e| format!("Failed to open train partition {:?}: {}", train_path, e))?;
    let mut train_records_read = 0usize;
    let mut train_document_count = 0usize;
    let mut document_set_hasher = Sha256::new();
    let mut token_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut doc_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_tokens = 0usize;
    let mut sentences: Vec<Vec<String>> = Vec::new();
    for (idx, line_res) in BufReader::new(file).lines().enumerate() {
        let line = line_res.map_err(|e| format!("Read error line {}: {}", idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: PartitionDocumentRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON error in train partition line {}: {}", idx + 1, e))?;
        if record.partition != "train" {
            return Err(format!(
                "Train partition record invalid: expected 'train', found '{}'",
                record.partition
            ));
        }
        train_records_read += 1;
        if record.corpus_id != corpus_id {
            continue;
        }
        // INVARIANT: only canonical duplicate representatives contribute (same rule as every
        // other TRAIN consumer); a document whose representative lives in another corpus is
        // excluded here exactly as it is everywhere else.
        if record.corpus_id != record.canonical_corpus_id
            || record.document_id != record.canonical_document_id
        {
            continue;
        }
        train_document_count += 1;
        document_set_hasher.update(line.as_bytes());
        document_set_hasher.update(b"\n");

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
        for sentence in split_into_sentences(&record.text) {
            sentences.push(tokenize_text(&sentence));
        }
    }
    if train_records_read != partition_manifest.train_documents {
        return Err(format!(
            "Train records count mismatch: partition manifest expected {}, read {}",
            partition_manifest.train_documents, train_records_read
        ));
    }
    if train_document_count == 0 || total_tokens == 0 {
        return Err(format!(
            "Corpus '{}' has no canonical TRAIN documents with tokens; nothing to model",
            corpus_id
        ));
    }
    let train_document_set_sha256 = format!("{:x}", document_set_hasher.finalize());

    // 4. Corpus-scoped statistics (same formulas as the corpus-wide builders).
    let frequency_records = frequency_records_from_counts(token_counts, &doc_counts, total_tokens);
    let bigram_records = compute_bigram_records(
        &sentences,
        bigram_min_count,
        config.bigram.maximum_predictions_per_context,
    )?;
    let trigram_records = compute_trigram_records(
        &sentences,
        trigram_min_count,
        config.trigram.maximum_predictions_per_context,
    )?;

    // 5. Git-ignored intermediates and their build manifest (pinned by the model manifest).
    let build_dir = root.join(LANGUAGE_MODEL_BUILD_DIR).join(corpus_id);
    let train_frequencies_sha256 =
        write_jsonl(&build_dir.join(TRAIN_FREQUENCIES_FILE), &frequency_records)?;
    let train_bigrams_sha256 = write_jsonl(&build_dir.join(TRAIN_BIGRAMS_FILE), &bigram_records)?;
    let train_trigrams_sha256 =
        write_jsonl(&build_dir.join(TRAIN_TRIGRAMS_FILE), &trigram_records)?;
    let build_manifest = LanguageModelBuildManifest {
        schema_version: LANGUAGE_MODEL_BUILD_SCHEMA_VERSION.to_string(),
        corpus_id: corpus_id.to_string(),
        corpus_version: corpus.version.clone(),
        corpus_registry_sha256: corpus_registry_sha256.clone(),
        canonical_manifest_sha256: canonical_manifest_sha256.clone(),
        partition_manifest_sha256: partition_manifest_sha256.clone(),
        train_partition_sha256: train_partition_sha256.clone(),
        partition_train_documents: train_records_read,
        train_document_count,
        train_document_set_sha256: train_document_set_sha256.clone(),
        train_token_count: total_tokens as u64,
        train_sentence_count: sentences.len(),
        ngram_config_sha256: ngram_config_sha256.clone(),
        bigram_min_count,
        bigram_max_per_context: config.bigram.maximum_predictions_per_context,
        trigram_min_count,
        trigram_max_per_context: config.trigram.maximum_predictions_per_context,
        frequency_records: frequency_records.len(),
        bigram_records: bigram_records.len(),
        trigram_records: trigram_records.len(),
        train_frequencies_sha256: train_frequencies_sha256.clone(),
        train_bigrams_sha256: train_bigrams_sha256.clone(),
        train_trigrams_sha256: train_trigrams_sha256.clone(),
    };
    let build_manifest_path = build_dir.join(BUILD_MANIFEST_FILE);
    fs::write(
        &build_manifest_path,
        serde_json::to_string_pretty(&build_manifest).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| format!("Failed to write {:?}: {}", build_manifest_path, e))?;
    let build_manifest_sha256 = sha256_file(&build_manifest_path)?;

    // 6. Vocabulary and id assignment.
    let vocabulary = authoritative_vocabulary(root)?;
    if vocabulary.is_empty() {
        return Err("Authoritative vocabulary is empty".to_string());
    }
    let id_of: BTreeMap<&str, u32> = vocabulary
        .iter()
        .enumerate()
        .map(|(i, w)| (w.as_str(), i as u32))
        .collect();

    // 7. Numeric content.
    let mut unigrams: BTreeMap<u32, FrequencyMetadata> = BTreeMap::new();
    for rec in &frequency_records {
        if let Some(&id) = id_of.get(rec.word.as_str()) {
            if unigrams.insert(id, frequency_metadata(rec)?).is_some() {
                return Err(format!("Duplicate frequency record for '{}'", rec.word));
            }
        }
    }
    let mut bigrams: BTreeMap<(u32, u32), (u64, u64, u32)> = BTreeMap::new();
    for rec in &bigram_records {
        if let (Some(&a), Some(&b)) = (
            id_of.get(rec.previous.as_str()),
            id_of.get(rec.next.as_str()),
        ) {
            if bigrams
                .insert(
                    (a, b),
                    (rec.count, rec.context_count, rec.probability_millionths),
                )
                .is_some()
            {
                return Err(format!(
                    "Duplicate bigram record ({}, {})",
                    rec.previous, rec.next
                ));
            }
        }
    }
    let mut trigrams: BTreeMap<(u32, u32, u32), (u64, u64, u32)> = BTreeMap::new();
    for rec in &trigram_records {
        if let (Some(&a), Some(&b), Some(&c)) = (
            id_of.get(rec.previous_2.as_str()),
            id_of.get(rec.previous_1.as_str()),
            id_of.get(rec.next.as_str()),
        ) {
            if trigrams
                .insert(
                    (a, b, c),
                    (rec.count, rec.context_count, rec.probability_millionths),
                )
                .is_some()
            {
                return Err(format!(
                    "Duplicate trigram record ({}, {}, {})",
                    rec.previous_2, rec.previous_1, rec.next
                ));
            }
        }
    }
    let content = LanguageModelContent {
        vocabulary,
        unigrams: unigrams.into_iter().collect(),
        bigrams: bigrams
            .into_iter()
            .map(|((a, b), (c, cc, p))| (a, b, c, cc, p))
            .collect(),
        trigrams: trigrams
            .into_iter()
            .map(|((a, b, c), (n, cc, p))| (a, b, c, n, cc, p))
            .collect(),
    };

    let model_id = format!("{}-{}", corpus_id, corpus.version);
    let manifest = LanguageModelManifest {
        schema_version: LANGUAGE_MODEL_SCHEMA_VERSION.to_string(),
        model_id,
        corpus_id: corpus_id.to_string(),
        corpus_version: corpus.version.clone(),
        contributing_corpora: vec![corpus_id.to_string()],
        corpus_source_artifact_sha256: corpus.source_artifact.as_ref().map(|a| a.sha256.clone()),
        corpus_documents_sha256,
        corpus_registry_sha256,
        canonical_manifest_sha256,
        partition_manifest_sha256,
        train_partition_sha256,
        train_document_count,
        train_document_set_sha256,
        train_frequencies_sha256,
        train_bigrams_sha256,
        train_trigrams_sha256,
        build_manifest_sha256,
        ngram_config_sha256,
        licensing: LanguageModelLicensing {
            corpus_name: corpus.corpus_name.clone(),
            license: corpus.license.clone(),
            license_spdx: corpus.license_spdx.clone(),
            license_url: corpus.license_url.clone(),
            attribution: corpus.attribution.clone(),
            source_url: corpus.url.clone(),
            redistribution_determination: REDISTRIBUTION_PENDING_REVIEW.to_string(),
        },
        vocabulary_fingerprint: String::new(),
        vocabulary_size: 0,
        unigram_count: 0,
        bigram_count: 0,
        trigram_count: 0,
        bigram_min_count,
        trigram_min_count,
        files: Vec::new(),
    };
    write_language_model(root, manifest, &content)
}

/// SHA-256 of the committed model manifest (what pack manifests pin).
pub fn language_model_manifest_sha256<P: AsRef<Path>>(
    root_dir: P,
    model_id: &str,
) -> Result<String, String> {
    validate_model_id(model_id)?;
    let path = model_dir(root_dir.as_ref(), model_id).join(MANIFEST_FILE);
    if !path.exists() {
        return Err(format!(
            "Language model '{}' manifest missing at {:?}",
            model_id, path
        ));
    }
    sha256_file(&path)
}

fn parse_u64(s: &str, what: &str, line: usize) -> Result<u64, String> {
    s.parse::<u64>()
        .map_err(|_| format!("{} line {}: invalid number '{}'", what, line, s))
}

fn parse_u32(s: &str, what: &str, line: usize) -> Result<u32, String> {
    s.parse::<u32>()
        .map_err(|_| format!("{} line {}: invalid number '{}'", what, line, s))
}

/// Loads and hash-verifies the committed language model `model_id`, enforces the content
/// invariants of `validate_language_model_content`, requires the vocabulary fingerprint to
/// match both the manifest and the **current** authoritative pack vocabulary under `root`,
/// and expands numeric ids back to words for the compiler. Fails closed on any mismatch.
pub fn load_language_model<P: AsRef<Path>>(
    root_dir: P,
    model_id: &str,
) -> Result<LanguageModel, String> {
    let root = root_dir.as_ref();
    validate_model_id(model_id)?;
    let dir = model_dir(root, model_id);
    let manifest_path = dir.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(format!(
            "Language model '{}' not found at {:?}",
            model_id, dir
        ));
    }
    let manifest: LanguageModelManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|e| format!("Failed to read {:?}: {}", manifest_path, e))?,
    )
    .map_err(|e| format!("Failed to parse {:?}: {}", manifest_path, e))?;
    if manifest.schema_version != LANGUAGE_MODEL_SCHEMA_VERSION {
        return Err(format!(
            "Language model '{}' schema_version '{}' unsupported (expected '{}')",
            model_id, manifest.schema_version, LANGUAGE_MODEL_SCHEMA_VERSION
        ));
    }
    if manifest.model_id != model_id {
        return Err(format!(
            "Language model manifest id '{}' does not match directory '{}'",
            manifest.model_id, model_id
        ));
    }
    if manifest.contributing_corpora != vec![manifest.corpus_id.clone()] {
        return Err(format!(
            "Language model '{}' declares contributing corpora {:?}; exactly its own corpus '{}' is allowed",
            model_id, manifest.contributing_corpora, manifest.corpus_id
        ));
    }
    if !is_single_token(&manifest.licensing.license_spdx) {
        return Err(format!(
            "Language model '{}' licensing.license_spdx '{}' is not a single SPDX identifier",
            model_id, manifest.licensing.license_spdx
        ));
    }

    // artifacts.sha256 must list exactly the manifest plus the manifest's files, all matching.
    let art_path = dir.join(ARTIFACTS_FILE);
    let art = fs::read_to_string(&art_path)
        .map_err(|e| format!("Failed to read {:?}: {}", art_path, e))?;
    let mut listed: BTreeMap<String, String> = BTreeMap::new();
    for line in art.lines().filter(|l| !l.trim().is_empty()) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(format!("Malformed line in {:?}: '{}'", art_path, line));
        }
        if listed
            .insert(parts[1].to_string(), parts[0].to_string())
            .is_some()
        {
            return Err(format!("Duplicate entry '{}' in {:?}", parts[1], art_path));
        }
    }
    let mut expected: BTreeSet<String> = manifest.files.iter().map(|f| f.path.clone()).collect();
    expected.insert(MANIFEST_FILE.to_string());
    let required: BTreeSet<String> = [
        VOCABULARY_FILE,
        UNIGRAMS_FILE,
        BIGRAMS_FILE,
        TRIGRAMS_FILE,
        MANIFEST_FILE,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if expected != required {
        return Err(format!(
            "Language model '{}' manifest lists {:?}, expected {:?}",
            model_id, expected, required
        ));
    }
    let listed_names: BTreeSet<String> = listed.keys().cloned().collect();
    if listed_names != expected {
        return Err(format!(
            "Language model '{}' artifacts.sha256 lists {:?}, expected {:?}",
            model_id, listed_names, expected
        ));
    }
    for (name, expected_sha) in &listed {
        let actual = sha256_file(&dir.join(name))?;
        if &actual != expected_sha {
            return Err(format!(
                "Language model '{}' file '{}' hash mismatch: artifacts.sha256 {}, actual {}",
                model_id, name, expected_sha, actual
            ));
        }
        if let Some(f) = manifest.files.iter().find(|f| &f.path == name) {
            if &f.sha256 != expected_sha {
                return Err(format!(
                    "Language model '{}' file '{}' hash mismatch between manifest and artifacts.sha256",
                    model_id, name
                ));
            }
        }
    }
    let manifest_sha256 = sha256_file(&manifest_path)?;

    // Numeric content, validated with the same invariants the writer enforces.
    let vocab_text = fs::read_to_string(dir.join(VOCABULARY_FILE))
        .map_err(|e| format!("Failed to read vocabulary: {}", e))?;
    let vocabulary: Vec<String> = vocab_text.lines().map(|l| l.to_string()).collect();
    let read_tsv = |name: &str, cols: usize| -> Result<Vec<Vec<String>>, String> {
        let text = fs::read_to_string(dir.join(name))
            .map_err(|e| format!("Failed to read {}: {}", name, e))?;
        let mut rows = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let parts: Vec<String> = line.split('\t').map(|s| s.to_string()).collect();
            if parts.len() != cols {
                return Err(format!(
                    "{} line {}: expected {} columns",
                    name,
                    idx + 1,
                    cols
                ));
            }
            rows.push(parts);
        }
        Ok(rows)
    };
    let mut content = LanguageModelContent {
        vocabulary,
        ..Default::default()
    };
    for (i, r) in read_tsv(UNIGRAMS_FILE, 4)?.into_iter().enumerate() {
        let line = i + 1;
        content.unigrams.push((
            parse_u32(&r[0], UNIGRAMS_FILE, line)?,
            FrequencyMetadata {
                token_count: parse_u64(&r[1], UNIGRAMS_FILE, line)?,
                document_count: parse_u64(&r[2], UNIGRAMS_FILE, line)?,
                zipf_milli: parse_u32(&r[3], UNIGRAMS_FILE, line)?,
            },
        ));
    }
    for (i, r) in read_tsv(BIGRAMS_FILE, 5)?.into_iter().enumerate() {
        let line = i + 1;
        content.bigrams.push((
            parse_u32(&r[0], BIGRAMS_FILE, line)?,
            parse_u32(&r[1], BIGRAMS_FILE, line)?,
            parse_u64(&r[2], BIGRAMS_FILE, line)?,
            parse_u64(&r[3], BIGRAMS_FILE, line)?,
            parse_u32(&r[4], BIGRAMS_FILE, line)?,
        ));
    }
    for (i, r) in read_tsv(TRIGRAMS_FILE, 6)?.into_iter().enumerate() {
        let line = i + 1;
        content.trigrams.push((
            parse_u32(&r[0], TRIGRAMS_FILE, line)?,
            parse_u32(&r[1], TRIGRAMS_FILE, line)?,
            parse_u32(&r[2], TRIGRAMS_FILE, line)?,
            parse_u64(&r[3], TRIGRAMS_FILE, line)?,
            parse_u64(&r[4], TRIGRAMS_FILE, line)?,
            parse_u32(&r[5], TRIGRAMS_FILE, line)?,
        ));
    }
    validate_language_model_content(&content)
        .map_err(|e| format!("Language model '{}': {}", model_id, e))?;

    if content.vocabulary.len() != manifest.vocabulary_size
        || vocabulary_fingerprint(&content.vocabulary) != manifest.vocabulary_fingerprint
    {
        return Err(format!(
            "Language model '{}' vocabulary does not match its manifest (size/fingerprint)",
            model_id
        ));
    }
    if content.unigrams.len() != manifest.unigram_count
        || content.bigrams.len() != manifest.bigram_count
        || content.trigrams.len() != manifest.trigram_count
    {
        return Err(format!(
            "Language model '{}' record counts ({}, {}, {}) do not match its manifest ({}, {}, {})",
            model_id,
            content.unigrams.len(),
            content.bigrams.len(),
            content.trigrams.len(),
            manifest.unigram_count,
            manifest.bigram_count,
            manifest.trigram_count
        ));
    }

    // The model must have been built against the vocabulary the packs publish right now.
    let current_fingerprint = authoritative_vocabulary_fingerprint(root)?;
    if current_fingerprint != manifest.vocabulary_fingerprint {
        return Err(format!(
            "Language model '{}' was built for vocabulary fingerprint {} but the current authoritative pack vocabulary has fingerprint {}; regenerate it with `build-language-model --corpus-id {}`",
            model_id, manifest.vocabulary_fingerprint, current_fingerprint, manifest.corpus_id
        ));
    }

    // Expand ids to words for the compiler.
    let word = |id: u32| content.vocabulary[id as usize].clone();
    let unigrams = content
        .unigrams
        .iter()
        .map(|(id, m)| (word(*id), m.clone()))
        .collect();
    let bigrams = content
        .bigrams
        .iter()
        .map(|(a, b, count, context_count, p)| BigramRecord {
            previous: word(*a),
            next: word(*b),
            count: *count,
            context_count: *context_count,
            probability_millionths: *p,
        })
        .collect();
    let trigrams = content
        .trigrams
        .iter()
        .map(|(a, b, c, count, context_count, p)| TrigramRecord {
            previous_2: word(*a),
            previous_1: word(*b),
            next: word(*c),
            count: *count,
            context_count: *context_count,
            probability_millionths: *p,
        })
        .collect();

    Ok(LanguageModel {
        manifest,
        manifest_sha256,
        unigrams,
        bigrams,
        trigrams,
    })
}
