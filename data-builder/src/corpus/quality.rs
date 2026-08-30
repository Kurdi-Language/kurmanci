//! Deterministic Corpus Quality Analysis Module for Kurmancî Corpora.
//!
//! Analyzes raw document-level anomalies, script distribution, technical noise classification,
//! and lexicon matching against seed, reviewed, and experimental reservoirs.

use super::importer::{verify_canonical_manifest, CanonicalDocumentRecord};
use super::tokenizer::{is_letter, tokenize_text};
use crate::normalize::normalize_text;
use crate::pack::resolve_authoritative_pack_lexicon;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use unicode_script::{Script, UnicodeScript};

pub const LOW_CONTENT_TOKEN_THRESHOLD: usize = 5;
pub const MARKUP_DOMINATED_PERCENT_THRESHOLD: usize = 50;
pub const MAX_REVIEW_TOKEN_LENGTH: usize = 45;

/// Script distribution measurements for emitted lexical tokens.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ScriptDistribution {
    pub latin_tokens: usize,
    pub arabic_tokens: usize,
    pub cyrillic_tokens: usize,
    pub mixed_script_tokens: usize,
    pub other_script_tokens: usize,
}

/// Lexicon matching and coverage metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LexiconMatchingMetrics {
    pub seed_lexicon_tokens: usize,
    pub seed_lexicon_unique_types: usize,
    pub reviewed_lexicon_tokens: usize,
    pub reviewed_lexicon_unique_types: usize,
    pub experimental_lexicon_tokens: usize,
    pub experimental_lexicon_unique_types: usize,
    pub seed_lexicon_token_coverage_percent: f64,
    pub reviewed_lexicon_token_coverage_percent: f64,
    pub experimental_lexicon_token_coverage_percent: f64,
    pub experimental_lexicon_type_coverage_percent: f64,
}

/// Document-level anomaly counts measured from raw document text before tokenization.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DocumentAnomalyMetrics {
    pub empty_documents: usize,
    pub low_content_documents: usize,
    pub technical_markup_dominated_documents: usize,
}

/// Technical contamination metrics measured directly from raw source document prose before tokenization.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SourceDocumentQualityMetrics {
    pub raw_numeric_token_occurrences: usize,
    pub control_character_contamination: usize,
    pub url_email_source_remnants: usize,
    pub mediawiki_structural_remnants: usize,
}

/// Technical noise categorization measured over emitted post-tokenization lexical tokens.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LexicalTokenQualityMetrics {
    pub pathological_length_tokens: usize,
    pub protocol_marker_remnants: usize,
}

/// Overall statistical corpus quality metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CorpusQualityMetrics {
    pub schema_version: String,
    pub corpus_id: String,
    pub total_documents: usize,
    pub canonical_documents: usize,
    pub total_lexical_tokens: usize,
    pub unique_lexical_tokens: usize,
    pub script_distribution: ScriptDistribution,
    pub lexicon_matching: LexiconMatchingMetrics,
    pub document_anomalies: DocumentAnomalyMetrics,
    pub source_quality: SourceDocumentQualityMetrics,
    pub lexical_quality: LexicalTokenQualityMetrics,
}

/// Top token record emitted to JSONL reports.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopTokenRecord {
    pub rank: usize,
    pub token: String,
    pub normalized_token: String,
    pub count: usize,
    pub is_in_seed_lexicon: bool,
    pub is_in_reviewed_lexicon: bool,
    pub is_in_experimental_lexicon: bool,
    pub is_technical_noise: bool,
    pub noise_reason: String,
}

/// Analyzes corpus quality deterministically for a given `corpus_id`.
pub fn analyze_corpus_quality<P: AsRef<Path>>(
    root_dir: P,
    corpus_id: &str,
) -> Result<CorpusQualityMetrics, String> {
    let root = root_dir.as_ref();

    // 1. Verify canonical manifest provenance before reading document files
    let canonical_manifest = verify_canonical_manifest(root).map_err(|e| {
        format!(
            "Canonical manifest verification failed in quality analysis: {}",
            e
        )
    })?;

    let corpus_entry = canonical_manifest
        .corpora
        .iter()
        .find(|c| c.corpus_id == corpus_id)
        .ok_or_else(|| {
            format!(
                "Corpus ID '{}' not found in canonical import manifest.",
                corpus_id
            )
        })?;

    let canonical_docs_path = root
        .join("data/imported-canonical")
        .join(&corpus_entry.documents_file);
    if !canonical_docs_path.exists() {
        return Err(format!(
            "Canonical documents file missing at {:?}",
            canonical_docs_path
        ));
    }

    // 2. Resolve authoritative pack lexicon entries using normalized identity
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

    // Verify hard membership invariants
    for word in &seed_set {
        if !reviewed_set.contains(word) {
            return Err(format!(
                "Invariant violation: Seed word '{}' not present in reviewed pack lexicon",
                word
            ));
        }
    }
    for word in &reviewed_set {
        if !experimental_set.contains(word) {
            return Err(format!(
                "Invariant violation: Reviewed word '{}' not present in experimental-full pack lexicon",
                word
            ));
        }
    }

    // 3. Stream canonical documents and measure raw source + post-tokenization quality
    let docs_file = File::open(&canonical_docs_path).map_err(|e| {
        format!(
            "Failed to open canonical documents {:?}: {}",
            canonical_docs_path, e
        )
    })?;
    let docs_reader = BufReader::new(docs_file);

    let mut total_docs = 0usize;
    let mut empty_docs = 0usize;
    let mut low_content_docs = 0usize;
    let mut markup_dominated_docs = 0usize;

    let mut source_quality = SourceDocumentQualityMetrics::default();
    let mut lexical_quality = LexicalTokenQualityMetrics::default();
    let mut lexical_token_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut script_dist = ScriptDistribution::default();

    for (l_idx, line_res) in docs_reader.lines().enumerate() {
        let line = line_res.map_err(|e| {
            format!(
                "Read error in canonical documents {:?} at line {}: {}",
                canonical_docs_path,
                l_idx + 1,
                e
            )
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let doc: CanonicalDocumentRecord = serde_json::from_str(&line).map_err(|e| {
            format!(
                "JSON parse error in canonical documents {:?} at line {}: {}",
                canonical_docs_path,
                l_idx + 1,
                e
            )
        })?;

        total_docs += 1;
        let text = &doc.text;

        if text.trim().is_empty() {
            empty_docs += 1;
            continue;
        }

        // Measure raw source quality before tokenization
        let mut raw_token_count = 0usize;
        let mut raw_source_noise_count = 0usize;

        for raw_tok in text.split_whitespace() {
            raw_token_count += 1;
            let lower_raw = raw_tok.to_lowercase();
            let mut is_noise = false;

            if raw_tok.chars().all(|c| c.is_numeric()) {
                source_quality.raw_numeric_token_occurrences += 1;
                is_noise = true;
            }
            if raw_tok
                .chars()
                .any(|c| c.is_control() || c == '\u{FFFD}' || c == '\u{200B}')
            {
                source_quality.control_character_contamination += 1;
                is_noise = true;
            }
            if lower_raw.starts_with("http://")
                || lower_raw.starts_with("https://")
                || lower_raw.starts_with("www.")
                || lower_raw.starts_with("ftp://")
                || lower_raw.contains("://")
                || (lower_raw.contains("@") && lower_raw.contains("."))
            {
                source_quality.url_email_source_remnants += 1;
                is_noise = true;
            }
            if lower_raw.starts_with("<ref")
                || lower_raw.starts_with("</ref")
                || lower_raw.starts_with("<references")
                || lower_raw.starts_with("şablon:")
                || lower_raw.starts_with("wêne:")
                || lower_raw.starts_with("dosye:")
                || lower_raw.starts_with("category:")
                || lower_raw.starts_with("file:")
            {
                source_quality.mediawiki_structural_remnants += 1;
                is_noise = true;
            }

            if is_noise {
                raw_source_noise_count += 1;
            }
        }

        if raw_token_count > 0
            && (raw_source_noise_count * 100 / raw_token_count)
                >= MARKUP_DOMINATED_PERCENT_THRESHOLD
        {
            markup_dominated_docs += 1;
        }

        // Lexical tokenization and token quality analysis
        let tokens = tokenize_text(text);
        let mut clean_token_count = 0usize;

        for tok in &tokens {
            let norm_tok = normalize_text(tok);
            if norm_tok.is_empty() {
                continue;
            }

            *lexical_token_counts.entry(norm_tok.clone()).or_insert(0) += 1;

            // Script analysis
            classify_script(&norm_tok, &mut script_dist);

            // Technical noise analysis on lexical token
            let noise_reason = classify_technical_noise(&norm_tok);
            if noise_reason != "none" {
                match noise_reason.as_str() {
                    "url_email_fragment" => lexical_quality.protocol_marker_remnants += 1,
                    "pathological_length" => lexical_quality.pathological_length_tokens += 1,
                    _ => {}
                }
            } else {
                clean_token_count += 1;
            }
        }

        if clean_token_count < LOW_CONTENT_TOKEN_THRESHOLD {
            low_content_docs += 1;
        }
    }

    // 4. Aggregate totals and lexicon matching
    let total_tokens: usize = lexical_token_counts.values().sum();
    let unique_tokens = lexical_token_counts.len();

    let mut seed_matched_tokens = 0usize;
    let mut seed_matched_types = 0usize;
    let mut reviewed_matched_tokens = 0usize;
    let mut reviewed_matched_types = 0usize;
    let mut exp_matched_tokens = 0usize;
    let mut exp_matched_types = 0usize;

    for (tok, count) in &lexical_token_counts {
        if seed_set.contains(tok) {
            seed_matched_tokens += count;
            seed_matched_types += 1;
        }
        if reviewed_set.contains(tok) {
            reviewed_matched_tokens += count;
            reviewed_matched_types += 1;
        }
        if experimental_set.contains(tok) {
            exp_matched_tokens += count;
            exp_matched_types += 1;
        }
    }

    let seed_token_coverage = if total_tokens > 0 {
        (seed_matched_tokens as f64 * 100.0) / total_tokens as f64
    } else {
        0.0
    };
    let reviewed_token_coverage = if total_tokens > 0 {
        (reviewed_matched_tokens as f64 * 100.0) / total_tokens as f64
    } else {
        0.0
    };
    let exp_token_coverage = if total_tokens > 0 {
        (exp_matched_tokens as f64 * 100.0) / total_tokens as f64
    } else {
        0.0
    };
    let exp_type_coverage = if unique_tokens > 0 {
        (exp_matched_types as f64 * 100.0) / unique_tokens as f64
    } else {
        0.0
    };

    let metrics = CorpusQualityMetrics {
        schema_version: "corpus-quality-v1".to_string(),
        corpus_id: corpus_id.to_string(),
        total_documents: total_docs,
        canonical_documents: corpus_entry.document_count,
        total_lexical_tokens: total_tokens,
        unique_lexical_tokens: unique_tokens,
        script_distribution: script_dist,
        lexicon_matching: LexiconMatchingMetrics {
            seed_lexicon_tokens: seed_matched_tokens,
            seed_lexicon_unique_types: seed_matched_types,
            reviewed_lexicon_tokens: reviewed_matched_tokens,
            reviewed_lexicon_unique_types: reviewed_matched_types,
            experimental_lexicon_tokens: exp_matched_tokens,
            experimental_lexicon_unique_types: exp_matched_types,
            seed_lexicon_token_coverage_percent: round_2_dp(seed_token_coverage),
            reviewed_lexicon_token_coverage_percent: round_2_dp(reviewed_token_coverage),
            experimental_lexicon_token_coverage_percent: round_2_dp(exp_token_coverage),
            experimental_lexicon_type_coverage_percent: round_2_dp(exp_type_coverage),
        },
        document_anomalies: DocumentAnomalyMetrics {
            empty_documents: empty_docs,
            low_content_documents: low_content_docs,
            technical_markup_dominated_documents: markup_dominated_docs,
        },
        source_quality,
        lexical_quality,
    };

    // 5. Save report artifacts to data/reports/corpus-quality/{corpus_id}/
    let report_dir = root.join(format!("data/reports/corpus-quality/{}", corpus_id));
    fs::create_dir_all(&report_dir).map_err(|e| {
        format!(
            "Failed to create quality report directory at {:?}: {}",
            report_dir, e
        )
    })?;

    let summary_path = report_dir.join("summary.json");
    let summary_bytes = serde_json::to_vec_pretty(&metrics)
        .map_err(|e| format!("Failed to serialize summary: {}", e))?;
    fs::write(&summary_path, &summary_bytes)
        .map_err(|e| format!("Failed to write summary: {}", e))?;

    let source_quality_path = report_dir.join("source-quality-summary.json");
    let source_bytes = serde_json::to_vec_pretty(&metrics.source_quality)
        .map_err(|e| format!("Failed to serialize source quality: {}", e))?;
    fs::write(&source_quality_path, &source_bytes)
        .map_err(|e| format!("Failed to write source quality: {}", e))?;

    let doc_summary_path = report_dir.join("document-quality-summary.json");
    let doc_bytes = serde_json::to_vec_pretty(&metrics.document_anomalies)
        .map_err(|e| format!("Failed to serialize doc summary: {}", e))?;
    fs::write(&doc_summary_path, &doc_bytes)
        .map_err(|e| format!("Failed to write doc summary: {}", e))?;

    // Sort tokens by count descending, then lexically
    let mut sorted_tokens: Vec<(String, usize)> = lexical_token_counts.into_iter().collect();
    sorted_tokens.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let top_tokens_path = report_dir.join("top-tokens.jsonl");
    let mut top_file = File::create(&top_tokens_path)
        .map_err(|e| format!("Failed to create top-tokens.jsonl: {}", e))?;

    let top_oov_path = report_dir.join("top-oov-tokens.jsonl");
    let mut oov_file = File::create(&top_oov_path)
        .map_err(|e| format!("Failed to create top-oov-tokens.jsonl: {}", e))?;

    let mut oov_rank = 1usize;

    for (rank, (tok, count)) in (1usize..).zip(sorted_tokens.iter()) {
        let is_seed = seed_set.contains(tok);
        let is_rev = reviewed_set.contains(tok);
        let is_exp = experimental_set.contains(tok);
        let noise_reason = classify_technical_noise(tok);
        let is_noise = noise_reason != "none";

        let rec = TopTokenRecord {
            rank,
            token: tok.clone(),
            normalized_token: tok.clone(),
            count: *count,
            is_in_seed_lexicon: is_seed,
            is_in_reviewed_lexicon: is_rev,
            is_in_experimental_lexicon: is_exp,
            is_technical_noise: is_noise,
            noise_reason,
        };

        if rank <= 500 {
            let line = serde_json::to_string(&rec).unwrap();
            writeln!(top_file, "{}", line).unwrap();
        }

        if !is_exp && oov_rank <= 500 {
            let mut oov_rec = rec.clone();
            oov_rec.rank = oov_rank;
            let line = serde_json::to_string(&oov_rec).unwrap();
            writeln!(oov_file, "{}", line).unwrap();
            oov_rank += 1;
        }
    }

    // 6. Generate checksum manifest artifacts.sha256
    let artifact_files = [
        "summary.json",
        "source-quality-summary.json",
        "document-quality-summary.json",
        "top-tokens.jsonl",
        "top-oov-tokens.jsonl",
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

    Ok(metrics)
}

fn classify_script(tok: &str, dist: &mut ScriptDistribution) {
    let mut has_latin = false;
    let mut has_arabic = false;
    let mut has_cyrillic = false;
    let mut has_other = false;

    for ch in tok.chars() {
        if !is_letter(ch) {
            continue;
        }
        let script = ch.script();
        match script {
            Script::Latin => has_latin = true,
            Script::Arabic => has_arabic = true,
            Script::Cyrillic => has_cyrillic = true,
            _ => has_other = true,
        }
    }

    let script_count = (has_latin as usize)
        + (has_arabic as usize)
        + (has_cyrillic as usize)
        + (has_other as usize);

    if script_count > 1 {
        dist.mixed_script_tokens += 1;
    } else if has_latin {
        dist.latin_tokens += 1;
    } else if has_arabic {
        dist.arabic_tokens += 1;
    } else if has_cyrillic {
        dist.cyrillic_tokens += 1;
    } else {
        dist.other_script_tokens += 1;
    }
}

pub fn classify_technical_noise(tok: &str) -> String {
    let lower = tok.to_lowercase();

    // 1. Control / replacement chars
    if tok
        .chars()
        .any(|ch| ch.is_control() || ch == '\u{FFFD}' || ch == '\u{200B}')
    {
        return "control_character".to_string();
    }

    // 2. Pure numeric (evaluated over token string)
    if tok.chars().all(|ch| ch.is_numeric()) {
        return "pure_numeric".to_string();
    }

    // 3. No letter characters
    if !tok.chars().any(is_letter) {
        return "no_letter_characters".to_string();
    }

    // 4. Pathological length
    let char_count = tok.chars().count();
    if char_count > MAX_REVIEW_TOKEN_LENGTH
        || (char_count == 1 && !is_letter(tok.chars().next().unwrap()))
    {
        return "pathological_length".to_string();
    }

    // 5. Post-tokenization protocol markers and URL fragments
    if lower == "http"
        || lower == "https"
        || lower == "www"
        || lower == "ftp"
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("www.")
        || lower.starts_with("ftp://")
        || lower.contains("://")
        || (lower.contains("@") && lower.contains("."))
    {
        return "url_email_fragment".to_string();
    }

    // 6. MediaWiki structural tag syntax remnants (e.g. <ref ...>, şablon:, wêne:)
    // Note: Ordinary lexical words (wêne, şablon, dosye, kategorî, binêre, category, file, references) must NOT be blacklisted as bare tokens.
    if lower.starts_with("<ref")
        || lower.starts_with("</ref")
        || lower.starts_with("<references")
        || lower.starts_with("şablon:")
        || lower.starts_with("wêne:")
        || lower.starts_with("dosye:")
        || lower.starts_with("category:")
        || lower.starts_with("file:")
    {
        return "mediawiki_structural_remnant".to_string();
    }

    "none".to_string()
}

fn round_2_dp(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}
