//! Corpus Quality Audit and Script/Deduplication Analyzer.

use super::importer::CanonicalDocumentRecord;
use super::ngrams::split_into_sentences;
use super::tokenizer::tokenize_text;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

pub const DOCUMENT_DEDUP_VERSION: &str = "document-dedup-v1";
pub const SENTENCE_DEDUP_VERSION: &str = "sentence-dedup-v1";

/// Document deduplication normalization:
/// UTF-8 validation, Unicode NFC, CRLF/CR -> LF, trim trailing line whitespace, trim leading/trailing blank lines.
pub fn normalize_document_for_dedup(text: &str) -> String {
    let nfc = text.nfc().collect::<String>();
    let lf_only = nfc.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = lf_only.lines().map(|l| l.trim_end()).collect();
    let joined = lines.join("\n");
    joined.trim_matches('\n').to_string()
}

/// Computes the deterministic `duplicate_group_id` for a document string.
pub fn compute_duplicate_group_id(text: &str) -> String {
    let normalized = normalize_document_for_dedup(text);
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

/// Sentence deduplication normalization:
/// Unicode NFC, Kurmancî lowercase, collapse Unicode whitespace to single space, trim.
pub fn normalize_sentence_for_dedup(text: &str) -> String {
    let nfc = text.nfc().collect::<String>();
    let lower = nfc.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    words.join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptValidationMetrics {
    pub total_tokens: usize,
    pub latin_tokens: usize,
    pub non_latin_tokens: usize,
    pub numeric_tokens: usize,
    pub url_email_tokens: usize,
    pub control_character_tokens: usize,
    pub replacement_character_tokens: usize,
    pub lexicon_covered_tokens: usize,
    pub lexicon_coverage_percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerCorpusAuditStats {
    pub corpus_id: String,
    pub document_count: usize,
    pub sentence_count: usize,
    pub token_count: usize,
    pub latin_token_count: usize,
    pub non_latin_token_count: usize,
    pub numeric_token_count: usize,
    pub url_email_token_count: usize,
    pub lexicon_coverage_percentage: f64,
    pub duplicate_document_members: usize,
    pub duplicate_sentence_occurrences: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusAuditSummary {
    pub corpus_count: usize,
    pub total_source_files: usize,
    pub duplicate_files_count: usize,
    pub total_documents: usize,
    pub distinct_duplicate_groups: usize,
    pub duplicate_documents_count: usize,
    pub total_sentences: usize,
    pub duplicate_sentences_count: usize,
    pub script_validation: ScriptValidationMetrics,
    pub per_corpus_statistics: Vec<PerCorpusAuditStats>,
}

/// Audits imported canonical corpora and writes reports to `data/reports/corpus-quality/`.
pub fn audit_corpora<P: AsRef<Path>>(root_dir: P) -> Result<CorpusAuditSummary, String> {
    let root = root_dir.as_ref();
    let manifest = super::importer::verify_canonical_manifest(root)?;
    let imported_dir = root.join("data/imported-canonical");

    // Load canonical lexicon for coverage analysis
    let lexicon_path = root.join("data/reviewed/lexicon.jsonl");
    if !lexicon_path.exists() {
        return Err(format!(
            "Reviewed lexicon missing at {:?}; cannot calculate corpus coverage",
            lexicon_path
        ));
    }

    let lexicon_file = File::open(&lexicon_path)
        .map_err(|e| format!("Failed to open reviewed lexicon {:?}: {}", lexicon_path, e))?;
    let mut canonical_words = BTreeSet::new();

    for (line_index, line_result) in BufReader::new(lexicon_file).lines().enumerate() {
        let line = line_result.map_err(|error| {
            format!(
                "Failed reading reviewed lexicon {:?} at line {}: {}",
                lexicon_path,
                line_index + 1,
                error
            )
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let value: serde_json::Value = serde_json::from_str(&line).map_err(|error| {
            format!(
                "Malformed reviewed lexicon JSON {:?} at line {}: {}",
                lexicon_path,
                line_index + 1,
                error
            )
        })?;

        let normalized = value
            .get("normalized")
            .and_then(|v| v.as_str())
            .filter(|word| !word.is_empty())
            .ok_or_else(|| {
                format!(
                    "Missing normalized field in {:?} at line {}",
                    lexicon_path,
                    line_index + 1
                )
            })?;

        canonical_words.insert(normalized.to_string());
    }

    let reports_dir = root.join("data/reports/corpus-quality");
    let stage_reports_dir = root.join("data/reports/corpus-quality.tmp_stage");
    let backup_reports_dir = root.join("data/reports/corpus-quality.tmp_backup");

    if stage_reports_dir.exists() {
        remove_dir_or_file(&stage_reports_dir).map_err(|e| {
            format!(
                "Failed to clean stage reports dir {:?}: {}",
                stage_reports_dir, e
            )
        })?;
    }
    fs::create_dir_all(&stage_reports_dir).map_err(|e| {
        format!(
            "Failed to create quality report stage dir {:?}: {}",
            stage_reports_dir, e
        )
    })?;

    // 1. File Duplicates Analysis
    let mut files_by_sha256: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for corpus_entry in &manifest.corpora {
        for src_file in &corpus_entry.source_files {
            files_by_sha256
                .entry(src_file.sha256.clone())
                .or_default()
                .push((corpus_entry.corpus_id.clone(), src_file.path.clone()));
        }
    }

    let dup_files_path = stage_reports_dir.join("duplicate-files.jsonl");
    let mut dup_files_writer = File::create(&dup_files_path)
        .map_err(|e| format!("Create duplicate-files.jsonl failed: {}", e))?;
    let mut duplicate_files_count = 0usize;
    for (sha256, paths) in &files_by_sha256 {
        if paths.len() > 1 {
            duplicate_files_count += paths.len() - 1;
            let json = serde_json::json!({
                "sha256": sha256,
                "count": paths.len(),
                "files": paths
            });
            writeln!(dup_files_writer, "{}", json)
                .map_err(|e| format!("Write duplicate-files failed: {}", e))?;
        }
    }

    // 2. Document & Sentence Duplicates & Script Analysis
    let mut docs_by_group: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut sentence_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut sentence_examples: BTreeMap<String, String> = BTreeMap::new();

    let mut total_tokens = 0usize;
    let mut latin_tokens = 0usize;
    let mut non_latin_tokens = 0usize;
    let mut numeric_tokens = 0usize;
    let mut url_email_tokens = 0usize;
    let mut control_tokens = 0usize;
    let mut replacement_tokens = 0usize;
    let mut lexicon_covered_tokens = 0usize;
    let mut total_docs = 0usize;
    let mut total_sentences = 0usize;
    let mut per_corpus_statistics = Vec::new();

    for corpus_entry in &manifest.corpora {
        let doc_path = imported_dir.join(&corpus_entry.documents_file);
        let file =
            File::open(&doc_path).map_err(|e| format!("Read doc error {:?}: {}", doc_path, e))?;

        let mut corpus_docs = 0usize;
        let mut corpus_sents = 0usize;
        let mut corpus_tokens = 0usize;
        let mut corpus_latin = 0usize;
        let mut corpus_non_latin = 0usize;
        let mut corpus_numeric = 0usize;
        let mut corpus_url_email = 0usize;
        let mut corpus_lexicon_covered = 0usize;
        let mut corpus_dup_docs = 0usize;
        let mut corpus_dup_sents = 0usize;

        for line_res in BufReader::new(file).lines() {
            let line = line_res.map_err(|e| format!("Read line error: {}", e))?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let record: CanonicalDocumentRecord = serde_json::from_str(trimmed)
                .map_err(|e| format!("JSONL record parse error: {}", e))?;
            total_docs += 1;
            corpus_docs += 1;

            let group_id = compute_duplicate_group_id(&record.text);
            let group_entry = docs_by_group.entry(group_id).or_default();
            if !group_entry.is_empty() {
                corpus_dup_docs += 1;
            }
            group_entry.push((record.corpus_id.clone(), record.document_id.clone()));

            let sents = split_into_sentences(&record.text);
            total_sentences += sents.len();
            corpus_sents += sents.len();

            for sent in sents {
                let norm_sent = normalize_sentence_for_dedup(&sent);
                if !norm_sent.is_empty() {
                    let sc = sentence_counts.entry(norm_sent.clone()).or_insert(0);
                    if *sc > 0 {
                        corpus_dup_sents += 1;
                    }
                    *sc += 1;
                    sentence_examples.entry(norm_sent).or_insert(sent);
                }
            }

            let tokens = tokenize_text(&record.text);
            total_tokens += tokens.len();
            corpus_tokens += tokens.len();

            for tok in tokens {
                if tok.contains('@')
                    || tok.contains("http://")
                    || tok.contains("https://")
                    || tok.contains("www.")
                {
                    url_email_tokens += 1;
                    corpus_url_email += 1;
                } else if tok.chars().all(|c| c.is_numeric() || c == '.' || c == ',') {
                    numeric_tokens += 1;
                    corpus_numeric += 1;
                } else if tok.chars().any(|c| c == '\u{FFFD}') {
                    replacement_tokens += 1;
                } else if tok.chars().any(|c| c.is_control()) {
                    control_tokens += 1;
                } else if tok.chars().any(|c| {
                    ('\u{0600}'..='\u{06FF}').contains(&c) || ('\u{0400}'..='\u{04FF}').contains(&c)
                }) {
                    non_latin_tokens += 1;
                    corpus_non_latin += 1;
                } else {
                    latin_tokens += 1;
                    corpus_latin += 1;
                }

                if canonical_words.contains(&tok) {
                    lexicon_covered_tokens += 1;
                    corpus_lexicon_covered += 1;
                }
            }
        }

        let c_cov_pct = if corpus_tokens > 0 {
            (corpus_lexicon_covered as f64 / corpus_tokens as f64) * 100.0
        } else {
            0.0
        };

        per_corpus_statistics.push(PerCorpusAuditStats {
            corpus_id: corpus_entry.corpus_id.clone(),
            document_count: corpus_docs,
            sentence_count: corpus_sents,
            token_count: corpus_tokens,
            latin_token_count: corpus_latin,
            non_latin_token_count: corpus_non_latin,
            numeric_token_count: corpus_numeric,
            url_email_token_count: corpus_url_email,
            lexicon_coverage_percentage: (c_cov_pct * 100.0).round() / 100.0,
            duplicate_document_members: corpus_dup_docs,
            duplicate_sentence_occurrences: corpus_dup_sents,
        });
    }

    per_corpus_statistics.sort_by(|a, b| a.corpus_id.cmp(&b.corpus_id));

    // Write Document Duplicates Report
    let dup_docs_path = stage_reports_dir.join("duplicate-documents.jsonl");
    let mut dup_docs_writer = File::create(&dup_docs_path)
        .map_err(|e| format!("Create duplicate-documents.jsonl failed: {}", e))?;
    let mut duplicate_docs_count = 0usize;
    for (group_id, docs) in &docs_by_group {
        if docs.len() > 1 {
            duplicate_docs_count += docs.len() - 1;
            let json = serde_json::json!({
                "duplicate_group_id": group_id,
                "count": docs.len(),
                "canonical_representative": docs.iter().min().unwrap(),
                "documents": docs
            });
            writeln!(dup_docs_writer, "{}", json)
                .map_err(|e| format!("Write duplicate-documents failed: {}", e))?;
        }
    }

    // Write Sentence Duplicates Report
    let dup_sents_path = stage_reports_dir.join("duplicate-sentences.jsonl");
    let mut dup_sents_writer = File::create(&dup_sents_path)
        .map_err(|e| format!("Create duplicate-sentences.jsonl failed: {}", e))?;
    let mut duplicate_sents_count = 0usize;
    for (norm_sent, count) in &sentence_counts {
        if *count > 1 {
            duplicate_sents_count += count - 1;
            let json = serde_json::json!({
                "normalized_sentence": norm_sent,
                "sample": sentence_examples.get(norm_sent).unwrap(),
                "count": count
            });
            writeln!(dup_sents_writer, "{}", json)
                .map_err(|e| format!("Write duplicate-sentences failed: {}", e))?;
        }
    }

    let coverage_pct = if total_tokens > 0 {
        (lexicon_covered_tokens as f64 / total_tokens as f64) * 100.0
    } else {
        0.0
    };

    let script_metrics = ScriptValidationMetrics {
        total_tokens,
        latin_tokens,
        non_latin_tokens,
        numeric_tokens,
        url_email_tokens,
        control_character_tokens: control_tokens,
        replacement_character_tokens: replacement_tokens,
        lexicon_covered_tokens,
        lexicon_coverage_percentage: (coverage_pct * 100.0).round() / 100.0,
    };

    fs::write(
        stage_reports_dir.join("script-validation.json"),
        serde_json::to_string_pretty(&script_metrics).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Write script-validation failed: {}", e))?;

    fs::write(
        stage_reports_dir.join("per-corpus-statistics.json"),
        serde_json::to_string_pretty(&per_corpus_statistics).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Write per-corpus-statistics failed: {}", e))?;

    let summary = CorpusAuditSummary {
        corpus_count: manifest.corpora.len(),
        total_source_files: files_by_sha256.values().map(|v| v.len()).sum(),
        duplicate_files_count,
        total_documents: total_docs,
        distinct_duplicate_groups: docs_by_group.len(),
        duplicate_documents_count: duplicate_docs_count,
        total_sentences,
        duplicate_sentences_count: duplicate_sents_count,
        script_validation: script_metrics,
        per_corpus_statistics,
    };

    fs::write(
        stage_reports_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Write summary failed: {}", e))?;

    let readme = format!(
        "# Kurmancî Corpus Quality Audit\n\n\
        - **Total Documents Audited**: {}\n\
        - **Distinct Duplicate Groups**: {}\n\
        - **Duplicate Documents**: {}\n\
        - **Duplicate Sentences**: {}\n\
        - **Lexicon Coverage**: {:.2}%\n",
        summary.total_documents,
        summary.distinct_duplicate_groups,
        summary.duplicate_documents_count,
        summary.duplicate_sentences_count,
        summary.script_validation.lexicon_coverage_percentage
    );
    fs::write(stage_reports_dir.join("README.md"), readme)
        .map_err(|e| format!("Write README failed: {}", e))?;

    // Manifest generation
    let mut manifest_entries = Vec::new();
    let expected = [
        "summary.json",
        "duplicate-files.jsonl",
        "duplicate-documents.jsonl",
        "duplicate-sentences.jsonl",
        "script-validation.json",
        "per-corpus-statistics.json",
        "README.md",
    ];
    for name in &expected {
        let fpath = stage_reports_dir.join(name);
        let content = fs::read(&fpath).map_err(|e| format!("Read report file failed: {}", e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        let rel_path = format!("data/reports/corpus-quality/{}", name);
        manifest_entries.push(format!("{} {}", hash, rel_path));
    }
    manifest_entries.sort();
    let manifest_bytes = manifest_entries.join("\n") + "\n";
    fs::write(stage_reports_dir.join("artifacts.sha256"), manifest_bytes)
        .map_err(|e| format!("Write artifacts.sha256 failed: {}", e))?;

    fn remove_dir_or_file<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
        let p = path.as_ref();
        if p.is_dir() {
            fs::remove_dir_all(p)
        } else if p.exists() || p.symlink_metadata().is_ok() {
            fs::remove_file(p)
        } else {
            Ok(())
        }
    }

    // Atomic Swap
    if backup_reports_dir.exists() {
        remove_dir_or_file(&backup_reports_dir)
            .map_err(|e| format!("Failed to clean backup reports dir: {}", e))?;
    }
    if reports_dir.exists() {
        fs::rename(&reports_dir, &backup_reports_dir)
            .map_err(|e| format!("Failed to rename report dir to backup: {}", e))?;
    }
    match fs::rename(&stage_reports_dir, &reports_dir) {
        Ok(()) => {
            if backup_reports_dir.exists() {
                if let Err(e) = remove_dir_or_file(&backup_reports_dir) {
                    eprintln!(
                        "Warning: failed to clean up backup dir {:?}: {}",
                        backup_reports_dir, e
                    );
                }
            }
        }
        Err(err) => {
            if backup_reports_dir.exists() {
                if let Err(rollback_err) = fs::rename(&backup_reports_dir, &reports_dir) {
                    return Err(format!(
                        "Failed to install quality report dir: {}; rollback also failed: {}",
                        err, rollback_err
                    ));
                }
            }
            return Err(format!("Failed to install quality report dir: {}", err));
        }
    }

    println!("⚡ CORPUS QUALITY AUDIT COMPLETED! Reports at data/reports/corpus-quality/");
    Ok(summary)
}
