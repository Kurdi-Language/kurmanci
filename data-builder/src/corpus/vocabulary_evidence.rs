//! Deterministic OOV Vocabulary Evidence Pipeline and Human Review Queue Generator.
//!
//! Compares per-corpus TRAIN partition frequencies against authoritative lexical packs (seed, reviewed, experimental-full),
//! extracts high-attestation OOV candidates with representative context evidence, validates full provenance,
//! and generates auditable human review queues.

use super::frequency::{FrequencyBuildManifest, FrequencyRecord};
use super::importer::{calculate_file_sha256, verify_canonical_manifest};
use super::partition::{PartitionDocumentRecord, PARTITION_POLICY_VERSION};
use super::quality::classify_technical_noise;
use super::tokenizer::tokenize_text;
use crate::normalize::normalize_text;
use crate::pack::resolve_authoritative_pack_lexicon;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

pub const MAX_REPRESENTATIVE_CONTEXTS: usize = 3;
pub const MAX_OOV_CANDIDATE_CONTEXTS_TARGETS: usize = 1000;

/// Provenance-aware representative context snippet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepresentativeContext {
    pub corpus_id: String,
    pub document_id: String,
    pub snippet: String,
}

/// OOV Candidate Record emitted to the deterministic OOV evidence JSONL and review queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OovCandidateRecord {
    pub schema_version: String,
    pub rank: usize,
    pub token: String,
    pub normalized_token: String,
    pub token_count: u64,
    pub document_count: u64,
    pub normalized_frequency: f64,
    pub zipf_milli: u32,
    pub in_seed: bool,
    pub in_reviewed: bool,
    pub in_experimental_full: bool,
    pub corpus_id: String,
    pub evidence_class: String,
    pub technical_filter_status: String,
    pub technical_filter_reason: String,
    pub representative_contexts: Vec<RepresentativeContext>,
}

/// Provenance metadata embedded in the evidence summary report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VocabularyEvidenceProvenance {
    pub corpus_registry_sha256: String,
    pub canonical_manifest_sha256: String,
    pub partition_manifest_sha256: String,
    pub train_partition_sha256: String,
    pub frequency_artifact_sha256: String,
    pub frequency_build_manifest_sha256: String,
    pub experimental_lexicon_fingerprint: String,
}

/// OOV document attestation distribution.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct OovDocumentDistribution {
    pub gte_2_docs: usize,
    pub gte_5_docs: usize,
    pub gte_10_docs: usize,
    pub gte_25_docs: usize,
    pub gte_50_docs: usize,
    pub gte_100_docs: usize,
}

/// High-level summary report for vocabulary evidence pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocabularyEvidenceSummaryReport {
    pub schema_version: String,
    pub corpus_id: String,
    pub provenance: VocabularyEvidenceProvenance,
    pub total_unique_train_tokens: usize,
    pub total_oov_unique_tokens: usize,
    pub eligible_oov_candidates: usize,
    pub technical_noise_exclusions: usize,
    pub already_known_tokens: usize,
    pub raw_oov_distribution: OovDocumentDistribution,
    pub eligible_oov_distribution: OovDocumentDistribution,
}

/// Special analysis target record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecialTargetRecord {
    pub target: String,
    pub normalized_target: String,
    pub token_count: u64,
    pub document_count: u64,
    pub is_in_seed: bool,
    pub is_in_reviewed: bool,
    pub is_in_experimental_full: bool,
    pub technical_filter_status: String,
    pub technical_filter_reason: String,
    pub representative_contexts: Vec<RepresentativeContext>,
}

/// Runs the full deterministic vocabulary evidence pipeline for `corpus_id`.
pub fn build_vocabulary_evidence<P: AsRef<Path>>(
    root_dir: P,
    corpus_id: &str,
) -> Result<VocabularyEvidenceSummaryReport, String> {
    let root = root_dir.as_ref();

    // 1. Full Provenance & Frequency Build Manifest Verification
    let _canonical_manifest = verify_canonical_manifest(root)
        .map_err(|e| format!("Canonical manifest verification failed: {}", e))?;

    let corpora_toml_path = root.join("data/source-registry/corpora.toml");
    let registry_sha256 = calculate_file_sha256(&corpora_toml_path)?;

    let canonical_manifest_path = root.join("data/imported-canonical/manifest.json");
    let canonical_manifest_sha256 = calculate_file_sha256(&canonical_manifest_path)?;

    let partition_dir = root.join("data/build/corpus-partitions");
    let partition_manifest_path = partition_dir.join("manifest.json");
    if !partition_manifest_path.exists() {
        return Err(format!(
            "Partition manifest missing at {:?}. Run partition-corpora first.",
            partition_manifest_path
        ));
    }
    let partition_manifest_sha256 = calculate_file_sha256(&partition_manifest_path)?;

    let train_partition_path = partition_dir.join("train.jsonl");
    if !train_partition_path.exists() {
        return Err(format!(
            "Train partition missing at {:?}. Run partition-corpora first.",
            train_partition_path
        ));
    }
    let train_partition_sha256 = calculate_file_sha256(&train_partition_path)?;

    let freq_path = root.join("data/build/frequencies.jsonl");
    if !freq_path.exists() {
        return Err(format!(
            "Frequencies artifact missing at {:?}. Run build-train-frequencies first.",
            freq_path
        ));
    }
    let frequency_sha256 = calculate_file_sha256(&freq_path)?;

    let freq_manifest_path = root.join("data/build/frequency_manifest.json");
    if !freq_manifest_path.exists() {
        return Err(format!(
            "Frequency build manifest missing at {:?}. Run build-train-frequencies first.",
            freq_manifest_path
        ));
    }
    let freq_manifest_bytes = fs::read(&freq_manifest_path).map_err(|e| {
        format!(
            "Failed to read frequency build manifest at {:?}: {}",
            freq_manifest_path, e
        )
    })?;
    let freq_manifest: FrequencyBuildManifest = serde_json::from_slice(&freq_manifest_bytes)
        .map_err(|e| format!("Failed to parse frequency build manifest: {}", e))?;

    // Strict Frequency Build Manifest Provenance Assertions
    if freq_manifest.schema_version != "frequency-build-v1" {
        return Err(format!(
            "Frequency build manifest schema_version mismatch: recorded '{}', expected 'frequency-build-v1'",
            freq_manifest.schema_version
        ));
    }
    if freq_manifest.partition_policy_version != PARTITION_POLICY_VERSION {
        return Err(format!(
            "Frequency build manifest partition_policy_version mismatch: recorded '{}', expected '{}'",
            freq_manifest.partition_policy_version, PARTITION_POLICY_VERSION
        ));
    }
    if freq_manifest.canonical_manifest_sha256 != canonical_manifest_sha256 {
        return Err(format!(
            "Frequency build manifest canonical_manifest_sha256 mismatch: recorded {}, current {}",
            freq_manifest.canonical_manifest_sha256, canonical_manifest_sha256
        ));
    }
    if freq_manifest.partition_manifest_sha256 != partition_manifest_sha256 {
        return Err(format!(
            "Frequency build manifest partition_manifest_sha256 mismatch: recorded {}, current {}",
            freq_manifest.partition_manifest_sha256, partition_manifest_sha256
        ));
    }
    if freq_manifest.train_partition_sha256 != train_partition_sha256 {
        return Err(format!(
            "Frequency build manifest train_partition_sha256 mismatch: recorded {}, current {}",
            freq_manifest.train_partition_sha256, train_partition_sha256
        ));
    }
    if freq_manifest.frequencies_sha256 != frequency_sha256 {
        return Err(format!(
            "Frequency build manifest frequencies_sha256 mismatch: recorded {}, current {}",
            freq_manifest.frequencies_sha256, frequency_sha256
        ));
    }

    let freq_manifest_sha256 = calculate_file_sha256(&freq_manifest_path)?;

    // 2. Resolve Authoritative Packs using normalized identity
    let seed_entries = resolve_authoritative_pack_lexicon("seed", root)?;
    let reviewed_entries = resolve_authoritative_pack_lexicon("reviewed", root)?;
    let exp_entries = resolve_authoritative_pack_lexicon("experimental-full", root)?;

    let seed_set: BTreeSet<String> = seed_entries
        .iter()
        .map(|e| e.normalized.clone())
        .filter(|w| !w.is_empty())
        .collect();

    let reviewed_set: BTreeSet<String> = reviewed_entries
        .iter()
        .map(|e| e.normalized.clone())
        .filter(|w| !w.is_empty())
        .collect();

    let experimental_set: BTreeSet<String> = exp_entries
        .iter()
        .map(|e| e.normalized.clone())
        .filter(|w| !w.is_empty())
        .collect();

    // Fingerprint actual resolved experimental-full entries
    let mut sorted_exp = exp_entries.clone();
    sorted_exp.sort_by(|a, b| {
        a.normalized
            .cmp(&b.normalized)
            .then_with(|| a.word.cmp(&b.word))
            .then_with(|| a.status.cmp(&b.status))
    });

    let mut fingerprint_hasher = Sha256::new();
    for entry in &sorted_exp {
        let norm_bytes = entry.normalized.as_bytes();
        fingerprint_hasher.update((norm_bytes.len() as u32).to_le_bytes());
        fingerprint_hasher.update(norm_bytes);

        let word_bytes = entry.word.as_bytes();
        fingerprint_hasher.update((word_bytes.len() as u32).to_le_bytes());
        fingerprint_hasher.update(word_bytes);

        let status_bytes = entry.status.as_bytes();
        fingerprint_hasher.update((status_bytes.len() as u32).to_le_bytes());
        fingerprint_hasher.update(status_bytes);

        fingerprint_hasher.update((entry.sources.len() as u32).to_le_bytes());
        for src in &entry.sources {
            let src_bytes = src.as_bytes();
            fingerprint_hasher.update((src_bytes.len() as u32).to_le_bytes());
            fingerprint_hasher.update(src_bytes);
        }
    }
    let lexicon_fingerprint = format!("{:x}", fingerprint_hasher.finalize());

    let provenance = VocabularyEvidenceProvenance {
        corpus_registry_sha256: registry_sha256,
        canonical_manifest_sha256,
        partition_manifest_sha256,
        train_partition_sha256,
        frequency_artifact_sha256: frequency_sha256,
        frequency_build_manifest_sha256: freq_manifest_sha256,
        experimental_lexicon_fingerprint: lexicon_fingerprint,
    };

    // 3. PASS 1: Extract per-corpus TRAIN partition frequencies (low-memory aggregation)
    let train_file = File::open(&train_partition_path).map_err(|e| {
        format!(
            "Failed to open train partition {:?}: {}",
            train_partition_path, e
        )
    })?;
    let train_reader = BufReader::new(train_file);

    let mut per_corpus_token_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_corpus_doc_counts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut total_corpus_tokens = 0usize;

    let special_target_words = [
        "destxweş",
        "taştê",
        "porteqal",
        "xwendin",
        "nivîsîn",
        "pêşkeş",
        "zarok",
        "xwarin",
        "firavîn",
        "xanî",
        "kirin",
        "kenîn",
        "girîn",
        "dil",
        "serî",
        "kategorî",
        "girêdanên",
        "binêre",
        "http",
        "https",
        "www",
        "landkreis",
        "franche",
        "bourgogne",
    ];

    for (l_idx, line_res) in train_reader.lines().enumerate() {
        let line = line_res.map_err(|e| {
            format!(
                "Read error in train partition {:?} at line {}: {}",
                train_partition_path,
                l_idx + 1,
                e
            )
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let doc: PartitionDocumentRecord = serde_json::from_str(&line).map_err(|e| {
            format!(
                "JSON parse error in train partition {:?} at line {}: {}",
                train_partition_path,
                l_idx + 1,
                e
            )
        })?;

        // Strict per-corpus attribution: only read documents belonging to requested corpus_id
        if doc.corpus_id != corpus_id
            || doc.canonical_corpus_id != corpus_id
            || doc.document_id != doc.canonical_document_id
        {
            continue;
        }

        let tokens = tokenize_text(&doc.text);
        if tokens.is_empty() {
            continue;
        }

        for tok in &tokens {
            let norm = normalize_text(tok);
            if norm.is_empty() {
                continue;
            }

            total_corpus_tokens += 1;
            *per_corpus_token_counts.entry(norm.clone()).or_insert(0) += 1;
            per_corpus_doc_counts
                .entry(norm)
                .or_default()
                .insert(doc.document_id.clone());
        }
    }

    // Build per-corpus frequency records
    let mut per_corpus_freqs: Vec<FrequencyRecord> = Vec::new();
    let total_corpus_tokens_f64 = total_corpus_tokens as f64;

    for (word, t_count) in &per_corpus_token_counts {
        let d_count = per_corpus_doc_counts
            .get(word)
            .map(|s| s.len())
            .unwrap_or(0);
        let norm_freq = if total_corpus_tokens > 0 {
            *t_count as f64 / total_corpus_tokens_f64
        } else {
            0.0
        };
        let raw_zipf = if norm_freq > 0.0 {
            log10(norm_freq * 1_000_000.0) + 3.0
        } else {
            0.0
        };
        let zipf = (raw_zipf * 100.0).round() / 100.0;

        per_corpus_freqs.push(FrequencyRecord {
            word: word.clone(),
            token_count: *t_count,
            document_count: d_count,
            normalized_frequency: norm_freq,
            zipf,
        });
    }

    let total_unique_tokens = per_corpus_freqs.len();

    // 4. Classify tokens into OOV candidates, known words, and technical noise
    let mut oov_records: Vec<FrequencyRecord> = Vec::new();
    let mut already_known_count = 0usize;
    let mut noise_exclusions_count = 0usize;

    let mut raw_dist = OovDocumentDistribution::default();
    let mut eligible_dist = OovDocumentDistribution::default();

    for rec in &per_corpus_freqs {
        let norm = normalize_text(&rec.word);
        let in_exp = experimental_set.contains(&norm);

        if in_exp {
            already_known_count += 1;
            continue;
        }

        let noise_reason = classify_technical_noise(&norm);
        let is_eligible = noise_reason == "none";

        if !is_eligible {
            noise_exclusions_count += 1;
        }

        // Update raw OOV distribution
        if rec.document_count >= 2 {
            raw_dist.gte_2_docs += 1;
        }
        if rec.document_count >= 5 {
            raw_dist.gte_5_docs += 1;
        }
        if rec.document_count >= 10 {
            raw_dist.gte_10_docs += 1;
        }
        if rec.document_count >= 25 {
            raw_dist.gte_25_docs += 1;
        }
        if rec.document_count >= 50 {
            raw_dist.gte_50_docs += 1;
        }
        if rec.document_count >= 100 {
            raw_dist.gte_100_docs += 1;
        }

        // Update eligible OOV distribution
        if is_eligible {
            if rec.document_count >= 2 {
                eligible_dist.gte_2_docs += 1;
            }
            if rec.document_count >= 5 {
                eligible_dist.gte_5_docs += 1;
            }
            if rec.document_count >= 10 {
                eligible_dist.gte_10_docs += 1;
            }
            if rec.document_count >= 25 {
                eligible_dist.gte_25_docs += 1;
            }
            if rec.document_count >= 50 {
                eligible_dist.gte_50_docs += 1;
            }
            if rec.document_count >= 100 {
                eligible_dist.gte_100_docs += 1;
            }
        }

        oov_records.push(rec.clone());
    }

    // Sort OOV candidates deterministically:
    // document_count desc -> token_count desc -> word lexical asc
    oov_records.sort_by(|a, b| {
        b.document_count
            .cmp(&a.document_count)
            .then_with(|| b.token_count.cmp(&a.token_count))
            .then_with(|| a.word.cmp(&b.word))
    });

    let total_oov_tokens = oov_records.len();
    let eligible_oov = total_oov_tokens.saturating_sub(noise_exclusions_count);

    // Collect target tokens for PASS 2 context extraction (top OOV review targets + special targets)
    let mut target_context_tokens: BTreeSet<String> = BTreeSet::new();

    let eligible_oov_candidates_iter = oov_records
        .iter()
        .filter(|r| classify_technical_noise(&normalize_text(&r.word)) == "none")
        .take(MAX_OOV_CANDIDATE_CONTEXTS_TARGETS);

    for rec in eligible_oov_candidates_iter {
        target_context_tokens.insert(normalize_text(&rec.word));
    }

    for target in &special_target_words {
        target_context_tokens.insert(normalize_text(target));
    }

    // 5. PASS 2: Context Extraction for target tokens only
    let mut per_corpus_contexts: BTreeMap<String, Vec<RepresentativeContext>> = BTreeMap::new();
    if !target_context_tokens.is_empty() {
        let train_file_p2 = File::open(&train_partition_path).map_err(|e| {
            format!(
                "Failed to open train partition for pass 2 {:?}: {}",
                train_partition_path, e
            )
        })?;
        let train_reader_p2 = BufReader::new(train_file_p2);

        for (l_idx, line_res) in train_reader_p2.lines().enumerate() {
            let line = line_res.map_err(|e| {
                format!(
                    "Read error in train partition pass 2 {:?} at line {}: {}",
                    train_partition_path,
                    l_idx + 1,
                    e
                )
            })?;

            if line.trim().is_empty() {
                continue;
            }

            let doc: PartitionDocumentRecord = serde_json::from_str(&line).map_err(|e| {
                format!(
                    "JSON parse error in train partition pass 2 {:?} at line {}: {}",
                    train_partition_path,
                    l_idx + 1,
                    e
                )
            })?;

            if doc.corpus_id != corpus_id
                || doc.canonical_corpus_id != corpus_id
                || doc.document_id != doc.canonical_document_id
            {
                continue;
            }

            let tokens = tokenize_text(&doc.text);
            if tokens.is_empty() {
                continue;
            }

            for (idx, tok) in tokens.iter().enumerate() {
                let norm = normalize_text(tok);
                if !target_context_tokens.contains(&norm) {
                    continue;
                }

                let contexts_list = per_corpus_contexts.entry(norm.clone()).or_default();
                if contexts_list.len() < MAX_REPRESENTATIVE_CONTEXTS
                    && !contexts_list
                        .iter()
                        .any(|c| c.document_id == doc.document_id)
                {
                    let start = idx.saturating_sub(4);
                    let end = (idx + 5).min(tokens.len());
                    let snippet = tokens[start..end].join(" ");

                    contexts_list.push(RepresentativeContext {
                        corpus_id: corpus_id.to_string(),
                        document_id: doc.document_id.clone(),
                        snippet,
                    });
                }
            }
        }
    }

    // 6. Build Candidate Queue Records & Evidence Records
    let mut candidate_evidence_records: Vec<OovCandidateRecord> = Vec::new();
    let mut eligible_review_queue_records: Vec<OovCandidateRecord> = Vec::new();

    let mut eligible_rank = 1usize;

    for (rank, rec) in (1usize..).zip(oov_records.iter()) {
        let norm = normalize_text(&rec.word);
        let in_seed = seed_set.contains(&norm);
        let in_rev = reviewed_set.contains(&norm);
        let in_exp = experimental_set.contains(&norm);

        let noise_reason = classify_technical_noise(&norm);
        let (status, evidence_class) = if noise_reason != "none" {
            ("technical_noise".to_string(), "technical_noise".to_string())
        } else {
            (
                "eligible_for_review".to_string(),
                "oov_candidate".to_string(),
            )
        };

        let contexts = per_corpus_contexts.get(&norm).cloned().unwrap_or_default();

        let candidate = OovCandidateRecord {
            schema_version: "oov-candidate-v1".to_string(),
            rank,
            token: rec.word.clone(),
            normalized_token: norm,
            token_count: rec.token_count as u64,
            document_count: rec.document_count as u64,
            normalized_frequency: rec.normalized_frequency,
            zipf_milli: (rec.zipf * 1000.0).round() as u32,
            in_seed,
            in_reviewed: in_rev,
            in_experimental_full: in_exp,
            corpus_id: corpus_id.to_string(),
            evidence_class,
            technical_filter_status: status.clone(),
            technical_filter_reason: noise_reason,
            representative_contexts: contexts,
        };

        candidate_evidence_records.push(candidate.clone());

        if status == "eligible_for_review" {
            let mut queue_rec = candidate;
            queue_rec.rank = eligible_rank;
            eligible_review_queue_records.push(queue_rec);
            eligible_rank += 1;
        }
    }

    // 7. Generate Special Analysis Target Report
    let mut special_reports = Vec::new();
    for target in &special_target_words {
        let norm = normalize_text(target);
        let freq_rec = per_corpus_freqs
            .iter()
            .find(|r| normalize_text(&r.word) == norm);
        let in_seed = seed_set.contains(&norm);
        let in_rev = reviewed_set.contains(&norm);
        let in_exp = experimental_set.contains(&norm);
        let noise_reason = classify_technical_noise(&norm);
        let status = if noise_reason != "none" {
            "technical_noise".to_string()
        } else if in_exp {
            "already_known".to_string()
        } else {
            "eligible_for_review".to_string()
        };

        let contexts = per_corpus_contexts.get(&norm).cloned().unwrap_or_default();

        let spec = SpecialTargetRecord {
            target: target.to_string(),
            normalized_target: norm,
            token_count: freq_rec.map(|r| r.token_count as u64).unwrap_or(0),
            document_count: freq_rec.map(|r| r.document_count as u64).unwrap_or(0),
            is_in_seed: in_seed,
            is_in_reviewed: in_rev,
            is_in_experimental_full: in_exp,
            technical_filter_status: status,
            technical_filter_reason: noise_reason,
            representative_contexts: contexts,
        };
        special_reports.push(spec);
    }

    // 8. Save Reports & Queue Artifacts to data/reports/vocabulary-evidence/{corpus_id}/
    let report_dir = root.join(format!("data/reports/vocabulary-evidence/{}", corpus_id));
    fs::create_dir_all(&report_dir).map_err(|e| {
        format!(
            "Failed to create report directory at {:?}: {}",
            report_dir, e
        )
    })?;

    let summary = VocabularyEvidenceSummaryReport {
        schema_version: "vocabulary-evidence-v1".to_string(),
        corpus_id: corpus_id.to_string(),
        provenance,
        total_unique_train_tokens: total_unique_tokens,
        total_oov_unique_tokens: total_oov_tokens,
        eligible_oov_candidates: eligible_oov,
        technical_noise_exclusions: noise_exclusions_count,
        already_known_tokens: already_known_count,
        raw_oov_distribution: raw_dist,
        eligible_oov_distribution: eligible_dist,
    };

    let summary_path = report_dir.join("summary.json");
    let summary_bytes = serde_json::to_vec_pretty(&summary)
        .map_err(|e| format!("Failed to serialize summary: {}", e))?;
    fs::write(&summary_path, &summary_bytes)
        .map_err(|e| format!("Failed to write summary: {}", e))?;

    let evidence_path = report_dir.join("oov-evidence.jsonl");
    let mut evidence_file = File::create(&evidence_path)
        .map_err(|e| format!("Failed to create evidence file {:?}: {}", evidence_path, e))?;
    for cand in &candidate_evidence_records {
        let line = serde_json::to_string(cand).unwrap();
        writeln!(evidence_file, "{}", line).unwrap();
    }

    let queue_path = report_dir.join("oov-review-queue.jsonl");
    let mut queue_file = File::create(&queue_path)
        .map_err(|e| format!("Failed to create queue file {:?}: {}", queue_path, e))?;

    for cand in &eligible_review_queue_records {
        let line = serde_json::to_string(cand).unwrap();
        writeln!(queue_file, "{}", line).unwrap();
    }

    let special_path = report_dir.join("special-targets-report.json");
    let special_bytes = serde_json::to_vec_pretty(&special_reports)
        .map_err(|e| format!("Failed to serialize special targets: {}", e))?;
    fs::write(&special_path, &special_bytes)
        .map_err(|e| format!("Failed to write special targets: {}", e))?;

    // Generate SHA-256 checksum manifest artifacts.sha256
    let artifact_files = [
        "summary.json",
        "oov-evidence.jsonl",
        "oov-review-queue.jsonl",
        "special-targets-report.json",
    ];

    let mut sha_lines = Vec::new();
    for file_name in &artifact_files {
        let p = report_dir.join(file_name);
        if p.exists() {
            let data = fs::read(&p).unwrap();
            let hash = format!("{:x}", Sha256::digest(&data));
            sha_lines.push(format!("{}  {}", hash, file_name));
        }
    }
    sha_lines.sort();

    let manifest_content = sha_lines.join("\n") + "\n";
    fs::write(report_dir.join("artifacts.sha256"), manifest_content)
        .map_err(|e| format!("Failed to write artifacts.sha256: {}", e))?;

    Ok(summary)
}

fn log10(val: f64) -> f64 {
    val.ln() / std::f64::consts::LN_10
}
