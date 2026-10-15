//! Prediction coverage on held-out corpus contexts: how often a pack's next-word prediction
//! answers at all, from which n-gram level it answers, and how often the word that actually
//! followed is among the returned candidates. The measurement is the baseline the roadmap
//! asks for before any smoothing or backoff change.
//!
//! It is provenance-bound: the pack's manifest names the language model it embeds and the
//! hash of that model's manifest; the model's manifest names its corpus and the hash of the
//! partition manifest its TRAIN partition came from. The evaluated partition must be the
//! `development` or `evaluation` partition of exactly that partitioning (never TRAIN), every
//! record must carry the requested partition name, and only canonical representatives of the
//! model's corpus are walked, tokenized by the same helper the model builder uses. The report
//! carries numbers and hashes only: no token, sentence or document text.
use crate::corpus::ngrams::sentence_token_sequences;
use crate::corpus::partition::PartitionDocumentRecord;
use kurmanci_engine::{Engine, PredictionSource};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::Path;

pub const PREDICTION_COVERAGE_SCHEMA_VERSION: &str = "prediction-coverage-v2";

/// Counts and rates for one kind of context (two preceding words, or one).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ContextCoverage {
    /// Positions of this context kind that were queried.
    pub positions: usize,
    /// Two-word contexts answered from the trigram table.
    pub trigram_hits: usize,
    /// Two-word contexts answered by the deterministic backoff to the bigram table.
    pub bigram_backoffs: usize,
    /// One-word contexts answered from the bigram table.
    pub bigram_hits: usize,
    /// Positions with no prediction at all.
    pub zero_results: usize,
    /// Positions where the word that actually followed was the first candidate.
    pub target_in_top_1: usize,
    /// ... among the first three candidates.
    pub target_in_top_3: usize,
    /// ... among the first five candidates.
    pub target_in_top_5: usize,
    pub trigram_hit_rate: f64,
    pub bigram_backoff_rate: f64,
    pub bigram_hit_rate: f64,
    pub zero_result_rate: f64,
    pub top_1_rate: f64,
    pub top_3_rate: f64,
    pub top_5_rate: f64,
}

impl ContextCoverage {
    fn record_target(&mut self, predictions: &[String], target: &str) {
        if let Some(rank) = predictions.iter().position(|w| w == target) {
            if rank < 1 {
                self.target_in_top_1 += 1;
            }
            if rank < 3 {
                self.target_in_top_3 += 1;
            }
            if rank < 5 {
                self.target_in_top_5 += 1;
            }
        }
    }

    fn finish(&mut self) {
        let n = self.positions as f64;
        let rate = |count: usize| if n > 0.0 { count as f64 / n } else { 0.0 };
        self.trigram_hit_rate = rate(self.trigram_hits);
        self.bigram_backoff_rate = rate(self.bigram_backoffs);
        self.bigram_hit_rate = rate(self.bigram_hits);
        self.zero_result_rate = rate(self.zero_results);
        self.top_1_rate = rate(self.target_in_top_1);
        self.top_3_rate = rate(self.target_in_top_3);
        self.top_5_rate = rate(self.target_in_top_5);
    }
}

/// The provenance chain the measurement was bound to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoverageProvenance {
    pub pack_path: String,
    pub pack_sha256: String,
    pub pack_entry_count: usize,
    pub pack_manifest_sha256: String,
    /// The model the pack manifest names, and the hash of its manifest the pack recorded.
    pub model_id: String,
    pub model_manifest_sha256: String,
    /// The corpus the model was built from; only its canonical representatives are walked.
    pub corpus_id: String,
    /// The partition manifest the model's TRAIN partition came from; the local one must match.
    pub partition_manifest_sha256: String,
    /// The record count that manifest declares for the evaluated partition; the file must have it.
    pub partition_declared_documents: usize,
    /// The evaluated partition (`development` or `evaluation`) and the hash of its file.
    pub partition: String,
    pub partition_sha256: String,
}

/// The report of one measurement. Numbers and hashes only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PredictionCoverageReport {
    pub schema_version: String,
    pub provenance: CoverageProvenance,
    /// Candidates requested per query; at least 5 so that the top-5 rate is meaningful.
    pub limit: usize,
    pub documents_read: usize,
    /// Records of a corpus other than the model's, skipped as the model builder skips them.
    pub other_corpus_documents_skipped: usize,
    /// Records whose canonical identity differs from their own (near-duplicates), skipped
    /// exactly as the model builder skips them.
    pub duplicate_documents_skipped: usize,
    pub documents_evaluated: usize,
    pub sentences: usize,
    pub tokens: usize,
    pub two_word_contexts: ContextCoverage,
    pub one_word_contexts: ContextCoverage,
}

/// Measures prediction coverage of the built pack at `pack_path` (its `manifest.json` must
/// sit next to it) on the `partition` (`development` or `evaluation`) of the repository at
/// `root`, bound to the pack's model and the model's partitioning. `limit` is the number of
/// candidates requested per query and must be at least 5.
pub fn evaluate_prediction_coverage(
    root: &Path,
    pack_path: &Path,
    partition: &str,
    limit: usize,
) -> Result<PredictionCoverageReport, String> {
    if limit < 5 {
        return Err(format!(
            "limit must be at least 5 so that the top-5 rate is meaningful (got {limit})"
        ));
    }
    // The whole provenance chain lives in eval_support, shared with the pack-quality
    // benchmark: a second copy here would be a copy that can rot.
    let verified =
        crate::eval_support::verify_pack_on_held_out_partition(root, pack_path, partition)?;
    let pack_bytes = verified.pack_bytes;
    let pack_sha256 = verified.pack_sha256;
    let model_id = verified.model_id;
    let model_manifest_sha256 = verified.model_manifest_sha256;
    let local_partition_manifest_sha256 = verified.partition_manifest_sha256;
    let expected_records = verified.partition_declared_documents;
    let partition_bytes = verified.partition_bytes;
    let partition_sha256 = verified.partition_sha256;
    let corpus_id = verified.corpus_id;
    let pack_manifest_sha256 = verified.pack_manifest_sha256;

    // 4. The engine.
    let mut engine = Engine::new();
    let pack_entry_count = engine
        .load_binary_pack(&pack_bytes)
        .map_err(|e| format!("Failed to load pack {:?}: {}", pack_path, e))?;

    let mut report = PredictionCoverageReport {
        schema_version: PREDICTION_COVERAGE_SCHEMA_VERSION.to_string(),
        provenance: CoverageProvenance {
            pack_path: pack_path.display().to_string(),
            pack_sha256,
            pack_entry_count,
            pack_manifest_sha256,
            model_id,
            model_manifest_sha256,
            corpus_id,
            partition_manifest_sha256: local_partition_manifest_sha256,
            partition_declared_documents: expected_records,
            partition: partition.to_string(),
            partition_sha256,
        },
        limit,
        documents_read: 0,
        other_corpus_documents_skipped: 0,
        duplicate_documents_skipped: 0,
        documents_evaluated: 0,
        sentences: 0,
        tokens: 0,
        two_word_contexts: ContextCoverage::default(),
        one_word_contexts: ContextCoverage::default(),
    };

    // 5. Walk the partition exactly as the model builder walks TRAIN.
    for (line_idx, line) in BufReader::new(partition_bytes.as_slice())
        .lines()
        .enumerate()
    {
        let line = line.map_err(|e| format!("Read error on line {}: {}", line_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: PartitionDocumentRecord = serde_json::from_str(&line)
            .map_err(|e| format!("Invalid partition record on line {}: {}", line_idx + 1, e))?;
        if record.partition != partition {
            return Err(format!(
                "partition record on line {} says '{}', expected '{}'",
                line_idx + 1,
                record.partition,
                partition
            ));
        }
        report.documents_read += 1;
        if record.corpus_id != report.provenance.corpus_id {
            report.other_corpus_documents_skipped += 1;
            continue;
        }
        if record.corpus_id != record.canonical_corpus_id
            || record.document_id != record.canonical_document_id
        {
            report.duplicate_documents_skipped += 1;
            continue;
        }
        report.documents_evaluated += 1;
        for tokens in sentence_token_sequences(&record.text) {
            if tokens.is_empty() {
                continue;
            }
            report.sentences += 1;
            report.tokens += tokens.len();
            for i in 1..tokens.len() {
                let target = &tokens[i];
                if i >= 2 {
                    let cov = &mut report.two_word_contexts;
                    cov.positions += 1;
                    let result =
                        engine.predict_next_with_context(&tokens[i - 2], &tokens[i - 1], limit);
                    let answered = !result.predictions.is_empty();
                    match (result.source, answered) {
                        (Some(PredictionSource::Trigram), true) => cov.trigram_hits += 1,
                        (Some(PredictionSource::BigramBackoff), true)
                        | (Some(PredictionSource::Bigram), true) => cov.bigram_backoffs += 1,
                        _ => cov.zero_results += 1,
                    }
                    let words: Vec<String> =
                        result.predictions.iter().map(|p| p.word.clone()).collect();
                    cov.record_target(&words, target);
                } else {
                    let cov = &mut report.one_word_contexts;
                    cov.positions += 1;
                    let predictions = engine.predict_next(&tokens[i - 1], limit);
                    if predictions.is_empty() {
                        cov.zero_results += 1;
                    } else {
                        cov.bigram_hits += 1;
                    }
                    let words: Vec<String> = predictions.iter().map(|p| p.word.clone()).collect();
                    cov.record_target(&words, target);
                }
            }
        }
    }
    // The partition file must be exactly the one the manifest describes: neither truncated
    // nor augmented (the same rule the model builder applies to TRAIN).
    if report.documents_read != expected_records {
        return Err(format!(
            "{} partition has {} records but the partition manifest the model was built from declares {}; the partition file is truncated or augmented",
            partition, report.documents_read, expected_records
        ));
    }
    report.two_word_contexts.finish();
    report.one_word_contexts.finish();
    Ok(report)
}

/// The human-readable table the CLI prints (numbers and hashes only).
pub fn format_report(report: &PredictionCoverageReport) -> String {
    let p = &report.provenance;
    let mut out = String::new();
    out.push_str(&format!(
        "pack {} (sha256 {}…, {} entries), model {} (manifest {}…), corpus {}, partitioning {}…\n",
        p.pack_path,
        &p.pack_sha256[..12],
        p.pack_entry_count,
        p.model_id,
        &p.model_manifest_sha256[..12],
        p.corpus_id,
        &p.partition_manifest_sha256[..12]
    ));
    out.push_str(&format!(
        "partition {} (sha256 {}…); limit {}; documents read {}, other corpus {}, duplicates {}, evaluated {}; sentences {}; tokens {}\n",
        p.partition,
        &p.partition_sha256[..12],
        report.limit,
        report.documents_read,
        report.other_corpus_documents_skipped,
        report.duplicate_documents_skipped,
        report.documents_evaluated,
        report.sentences,
        report.tokens
    ));
    out.push_str("context     positions  trigram  backoff  bigram   zero   top1    top3    top5\n");
    for (name, c) in [
        ("two-word", &report.two_word_contexts),
        ("one-word", &report.one_word_contexts),
    ] {
        out.push_str(&format!(
            "{:<10} {:>10} {:>8} {:>8} {:>7} {:>6}  {:>5.1}%  {:>5.1}%  {:>5.1}%\n",
            name,
            c.positions,
            c.trigram_hits,
            c.bigram_backoffs,
            c.bigram_hits,
            c.zero_results,
            c.top_1_rate * 100.0,
            c.top_3_rate * 100.0,
            c.top_5_rate * 100.0
        ));
    }
    out
}
