use super::ngrams::split_into_sentences;
use super::registry::CorpusRegistry;
use super::tokenizer::tokenize_text;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerCorpusInventoryStats {
    pub corpus_id: String,
    pub corpus_name: String,
    pub domain: String,
    pub license: String,
    pub file_count: usize,
    pub document_count: usize,
    pub sentence_count: usize,
    pub token_count: usize,
    pub unique_token_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusInventorySummary {
    pub corpus_count: usize,
    pub file_count: usize,
    pub document_count: usize,
    pub sentence_count: usize,
    pub token_count: usize,
    pub unique_token_count: usize,
    pub license_breakdown: BTreeMap<String, usize>,
    pub domain_breakdown: BTreeMap<String, usize>,
    pub per_corpus_stats: Vec<PerCorpusInventoryStats>,
}

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

/// Generates corpus inventory reports under `data/reports/corpus-inventory/`.
pub fn generate_corpus_inventory<P: AsRef<Path>>(
    root_dir: P,
) -> Result<CorpusInventorySummary, String> {
    let root = root_dir.as_ref();
    let manifest = super::importer::verify_canonical_manifest(root)?;
    let imported_dir = root.join("data/imported-canonical");

    let registry_path = root.join("data/source-registry/corpora.toml");
    let registry = CorpusRegistry::load_from_file(&registry_path)?;

    let mut total_files = 0usize;
    let mut total_documents = 0usize;
    let mut total_sentences = 0usize;
    let mut total_tokens = 0usize;
    let mut global_vocabulary: BTreeSet<String> = BTreeSet::new();

    let mut license_breakdown: BTreeMap<String, usize> = BTreeMap::new();
    let mut domain_breakdown: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_corpus_stats = Vec::new();

    for corpus_entry in &manifest.corpora {
        let reg_entry = registry
            .find_corpus(&corpus_entry.corpus_id)
            .ok_or_else(|| format!("Corpus '{}' not in registry", corpus_entry.corpus_id))?;

        let doc_file_path = imported_dir.join(&corpus_entry.documents_file);
        if !doc_file_path.exists() {
            return Err(format!(
                "Missing canonical document file {:?}",
                doc_file_path
            ));
        }

        let file = File::open(&doc_file_path)
            .map_err(|e| format!("Failed to open {:?}: {}", doc_file_path, e))?;
        let reader = BufReader::new(file);

        let mut corpus_sentences = 0usize;
        let mut corpus_tokens = 0usize;
        let mut corpus_vocab = BTreeSet::new();
        let mut corpus_docs = 0usize;

        for line_res in reader.lines() {
            let line = line_res.map_err(|e| format!("Read line error: {}", e))?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let val: serde_json::Value =
                serde_json::from_str(trimmed).map_err(|e| format!("JSONL parse error: {}", e))?;
            let text = val.get("text").and_then(|v| v.as_str()).unwrap_or("");

            let sents = split_into_sentences(text);
            corpus_sentences += sents.len();

            let tokens = tokenize_text(text);
            corpus_tokens += tokens.len();
            for tok in tokens {
                corpus_vocab.insert(tok.clone());
                global_vocabulary.insert(tok);
            }
            corpus_docs += 1;
        }

        let domain =
            if reg_entry.notes.contains("dialogue") || reg_entry.corpus_name.contains("Dialogue") {
                "dialogue".to_string()
            } else {
                "prose".to_string()
            };

        *license_breakdown
            .entry(reg_entry.license.clone())
            .or_insert(0) += corpus_tokens;
        *domain_breakdown.entry(domain.clone()).or_insert(0) += corpus_tokens;

        total_files += corpus_entry.source_files.len();
        total_documents += corpus_docs;
        total_sentences += corpus_sentences;
        total_tokens += corpus_tokens;

        per_corpus_stats.push(PerCorpusInventoryStats {
            corpus_id: corpus_entry.corpus_id.clone(),
            corpus_name: reg_entry.corpus_name.clone(),
            domain,
            license: reg_entry.license.clone(),
            file_count: corpus_entry.source_files.len(),
            document_count: corpus_docs,
            sentence_count: corpus_sentences,
            token_count: corpus_tokens,
            unique_token_count: corpus_vocab.len(),
        });
    }

    per_corpus_stats.sort_by(|a, b| a.corpus_id.cmp(&b.corpus_id));

    let summary = CorpusInventorySummary {
        corpus_count: manifest.corpora.len(),
        file_count: total_files,
        document_count: total_documents,
        sentence_count: total_sentences,
        token_count: total_tokens,
        unique_token_count: global_vocabulary.len(),
        license_breakdown,
        domain_breakdown,
        per_corpus_stats,
    };

    // Write Report Suite to staged dir
    let reports_dir = root.join("data/reports/corpus-inventory");
    let stage_reports_dir = root.join("data/reports/corpus-inventory.tmp_stage");
    let backup_reports_dir = root.join("data/reports/corpus-inventory.tmp_backup");

    if stage_reports_dir.exists() {
        remove_dir_or_file(&stage_reports_dir)
            .map_err(|e| format!("Failed to clean stage reports dir: {}", e))?;
    }
    fs::create_dir_all(&stage_reports_dir).map_err(|e| {
        format!(
            "Failed to create inventory report stage dir {:?}: {}",
            stage_reports_dir, e
        )
    })?;

    let summary_path = stage_reports_dir.join("summary.json");
    let summary_json = serde_json::to_string_pretty(&summary)
        .map_err(|e| format!("Failed to serialize summary: {}", e))?;
    fs::write(&summary_path, &summary_json)
        .map_err(|e| format!("Failed to write summary {:?}: {}", summary_path, e))?;

    let readme_content = format!(
        "# Kurmancî Corpus Inventory Report\n\n\
        - **Total Corpora**: {}\n\
        - **Total Source Files**: {}\n\
        - **Total Documents**: {}\n\
        - **Total Sentences**: {}\n\
        - **Total Tokens**: {}\n\
        - **Unique Vocabulary Size**: {}\n",
        summary.corpus_count,
        summary.file_count,
        summary.document_count,
        summary.sentence_count,
        summary.token_count,
        summary.unique_token_count
    );
    let readme_path = stage_reports_dir.join("README.md");
    fs::write(&readme_path, readme_content)
        .map_err(|e| format!("Failed to write README {:?}", e))?;

    // Manifest generation
    let mut manifest_entries = Vec::new();
    let expected_files = ["summary.json", "README.md"];
    for name in &expected_files {
        let fpath = stage_reports_dir.join(name);
        let content = fs::read(&fpath)
            .map_err(|e| format!("Failed to read report artifact {:?}: {}", fpath, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        let rel_path = format!("data/reports/corpus-inventory/{}", name);
        manifest_entries.push(format!("{} {}", hash, rel_path));
    }
    manifest_entries.sort();
    let manifest_bytes = manifest_entries.join("\n") + "\n";
    fs::write(stage_reports_dir.join("artifacts.sha256"), manifest_bytes)
        .map_err(|e| format!("Failed to write artifacts.sha256: {}", e))?;

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
                        "Failed to install inventory report dir: {}; rollback also failed: {}",
                        err, rollback_err
                    ));
                }
            }
            return Err(format!("Failed to install inventory report dir: {}", err));
        }
    }

    println!("⚡ CORPUS INVENTORY COMPLETED! Reports at data/reports/corpus-inventory/");
    Ok(summary)
}
