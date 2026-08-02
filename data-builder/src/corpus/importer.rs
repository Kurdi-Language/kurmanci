//! Corpus Importer module for importing registered text corpora into canonical JSONL format.

use super::registry::{CorpusRegistry, CorpusRegistryEntry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const CANONICAL_SCHEMA_VERSION: &str = "canonical-corpus-v1";

/// A single canonical document record inside `documents.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalDocumentRecord {
    pub corpus_id: String,
    pub document_id: String,
    pub source_file: String,
    pub source_record: usize,
    pub source_sha256: String,
    pub text: String,
}

/// Source file entry inside canonical manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceFileManifestEntry {
    pub path: String,
    pub sha256: String,
}

/// Corpus summary entry inside canonical manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalCorpusManifestEntry {
    pub corpus_id: String,
    pub documents_file: String,
    pub documents_sha256: String,
    pub document_count: usize,
    pub source_files: Vec<SourceFileManifestEntry>,
}

/// Overall canonical import manifest at `data/imported-canonical/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalImportManifest {
    pub schema_version: String,
    pub registry_sha256: String,
    pub corpora: Vec<CanonicalCorpusManifestEntry>,
}

/// Summary report emitted when a corpus is successfully imported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusImportSummaryReport {
    pub corpus_id: String,
    pub corpus_name: String,
    pub language: String,
    pub license: String,
    pub imported_files_count: usize,
    pub total_bytes: u64,
    pub document_count: usize,
    pub checksum_verification_passed: bool,
}

/// Helper struct for lock file management using RAII.
pub struct LockFileGuard {
    lock_path: PathBuf,
    file: Option<File>,
}

impl LockFileGuard {
    pub fn acquire<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let lock_path = path.as_ref().to_path_buf();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|e| format!("Failed to acquire import lock {:?}: {}", lock_path, e))?;
        Ok(Self {
            lock_path,
            file: Some(file),
        })
    }

    pub fn release(mut self) -> Result<(), String> {
        self.file.take();
        if self.lock_path.exists() {
            fs::remove_file(&self.lock_path)
                .map_err(|e| format!("Failed to remove lock file {:?}: {}", self.lock_path, e))?;
        }
        Ok(())
    }
}

impl Drop for LockFileGuard {
    fn drop(&mut self) {
        self.file.take();
        if self.lock_path.exists() {
            if let Err(e) = fs::remove_file(&self.lock_path) {
                eprintln!(
                    "Warning: failed to remove lock file {:?}: {}",
                    self.lock_path, e
                );
            }
        }
    }
}

/// Normalizes relative source path using validate_registry_relative_path.
fn normalize_relative_path(path_str: &str) -> Result<String, String> {
    super::registry::validate_registry_relative_path(path_str)
}

/// Calculates SHA-256 of file contents on disk.
fn calculate_file_sha256<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let mut file = File::open(&path)
        .map_err(|e| format!("Failed to open for hashing {:?}: {}", path.as_ref(), e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .map_err(|e| format!("Failed to hash file {:?}: {}", path.as_ref(), e))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verifies that `data/imported-canonical/` exists and matches `manifest.json`.
pub fn verify_canonical_manifest<P: AsRef<Path>>(
    root_dir: P,
) -> Result<CanonicalImportManifest, String> {
    let root = root_dir.as_ref();
    let imported_dir = root.join("data/imported-canonical");
    let manifest_path = imported_dir.join("manifest.json");

    if !manifest_path.exists() {
        return Err(format!(
            "Canonical import manifest missing at {:?}. Run import-all-corpora first.",
            manifest_path
        ));
    }

    let manifest_bytes = fs::read(&manifest_path).map_err(|e| {
        format!(
            "Failed to read canonical manifest {:?}: {}",
            manifest_path, e
        )
    })?;
    let manifest: CanonicalImportManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| {
            format!(
                "Failed to parse canonical manifest {:?}: {}",
                manifest_path, e
            )
        })?;

    if manifest.schema_version != CANONICAL_SCHEMA_VERSION {
        return Err(format!(
            "Canonical manifest schema version mismatch: expected '{}', got '{}'",
            CANONICAL_SCHEMA_VERSION, manifest.schema_version
        ));
    }

    let registry_path = root.join("data/source-registry/corpora.toml");
    if !registry_path.exists() {
        return Err(format!(
            "Corpus registry missing at {:?}. Cannot verify canonical manifest.",
            registry_path
        ));
    }

    let actual_reg_sha256 = calculate_file_sha256(&registry_path)?;
    if actual_reg_sha256 != manifest.registry_sha256 {
        return Err(format!(
            "Registry SHA-256 mismatch in canonical manifest: expected '{}', actual '{}'",
            manifest.registry_sha256, actual_reg_sha256
        ));
    }

    let registry = CorpusRegistry::load_from_file(&registry_path)?;

    // Verify exact corpus set equality and duplicate checks
    let manifest_corpus_ids: std::collections::BTreeSet<String> = manifest
        .corpora
        .iter()
        .map(|c| c.corpus_id.clone())
        .collect();
    if manifest_corpus_ids.len() != manifest.corpora.len() {
        return Err("Duplicate corpus_id found in canonical manifest".to_string());
    }

    let reg_corpus_ids: std::collections::BTreeSet<String> = registry
        .corpora
        .iter()
        .map(|c| c.corpus_id.clone())
        .collect();
    if reg_corpus_ids.len() != registry.corpora.len() {
        return Err("Duplicate corpus_id found in corpora.toml registry".to_string());
    }

    if manifest_corpus_ids != reg_corpus_ids {
        return Err(format!(
            "Manifest/Registry corpus set mismatch: manifest={:?}, registry={:?}",
            manifest_corpus_ids, reg_corpus_ids
        ));
    }

    let mut seen_doc_files = std::collections::HashSet::new();
    for corpus_entry in &manifest.corpora {
        if !seen_doc_files.insert(&corpus_entry.documents_file) {
            return Err(format!(
                "Duplicate documents_file path in manifest: '{}'",
                corpus_entry.documents_file
            ));
        }

        let docs_path = imported_dir.join(&corpus_entry.documents_file);
        if !docs_path.exists() {
            return Err(format!(
                "Missing canonical documents.jsonl for corpus '{}': {:?}",
                corpus_entry.corpus_id, docs_path
            ));
        }

        let actual_sha256 = calculate_file_sha256(&docs_path)?;
        if actual_sha256 != corpus_entry.documents_sha256 {
            return Err(format!(
                "Canonical document SHA-256 mismatch for corpus '{}': expected '{}', actual '{}'",
                corpus_entry.corpus_id, corpus_entry.documents_sha256, actual_sha256
            ));
        }

        let file =
            File::open(&docs_path).map_err(|e| format!("Failed to open {:?}: {}", docs_path, e))?;
        let reader = BufReader::new(file);
        let mut actual_doc_count = 0usize;
        for (line_index, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|error| {
                format!(
                    "Failed reading canonical corpus '{}' at line {}: {}",
                    corpus_entry.corpus_id,
                    line_index + 1,
                    error
                )
            })?;
            if !line.trim().is_empty() {
                actual_doc_count += 1;
            }
        }

        if actual_doc_count != corpus_entry.document_count {
            return Err(format!(
                "Document count mismatch for corpus '{}': expected {}, actual {}",
                corpus_entry.corpus_id, corpus_entry.document_count, actual_doc_count
            ));
        }
    }

    // Verify exact directory structure of data/imported-canonical/
    let mut expected_imported_entries: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    expected_imported_entries.insert("manifest.json".to_string());
    for c_id in &manifest_corpus_ids {
        expected_imported_entries.insert(c_id.clone());
    }

    let mut actual_dir_entries = std::collections::BTreeSet::new();
    let dir_reader = fs::read_dir(&imported_dir).map_err(|e| {
        format!(
            "Failed to read imported-canonical dir {:?}: {}",
            imported_dir, e
        )
    })?;
    for entry_res in dir_reader {
        let entry = entry_res.map_err(|e| {
            format!(
                "Failed reading dir entry in imported-canonical {:?}: {}",
                imported_dir, e
            )
        })?;
        actual_dir_entries.insert(entry.file_name().to_string_lossy().to_string());
    }

    if actual_dir_entries != expected_imported_entries {
        return Err(format!(
            "Unexpected or unmanifested contents in data/imported-canonical/: expected {:?}, actual {:?}",
            expected_imported_entries, actual_dir_entries
        ));
    }

    Ok(manifest)
}

/// Processes a single corpus entry into canonical document records in `stage_dir`.
fn process_corpus_canonical(
    entry: &CorpusRegistryEntry,
    root: &Path,
    stage_dir: &Path,
) -> Result<CanonicalCorpusManifestEntry, String> {
    let corpus_stage_dir = stage_dir.join(&entry.corpus_id);
    fs::create_dir_all(&corpus_stage_dir).map_err(|e| {
        format!(
            "Failed to create stage dir for '{}': {}",
            entry.corpus_id, e
        )
    })?;

    let docs_file_path = corpus_stage_dir.join("documents.jsonl");
    let mut docs_file = File::create(&docs_file_path).map_err(|e| {
        format!(
            "Failed to create documents.jsonl for '{}': {}",
            entry.corpus_id, e
        )
    })?;

    let mut doc_count = 0usize;
    let mut seen_doc_ids = std::collections::HashSet::new();
    let mut source_files = Vec::new();

    for file_entry in &entry.files {
        let rel_path_norm = normalize_relative_path(&file_entry.path)?;

        let src_file_path = root.join(&file_entry.path);
        let file_bytes = fs::read(&src_file_path)
            .map_err(|e| format!("Failed to read source file {:?}: {}", src_file_path, e))?;
        let actual_file_sha256 = format!("{:x}", Sha256::digest(&file_bytes));

        if actual_file_sha256 != file_entry.sha256 {
            return Err(format!(
                "Source changed before canonical ingestion for '{}': expected {}, actual {}",
                file_entry.path, file_entry.sha256, actual_file_sha256
            ));
        }

        source_files.push(SourceFileManifestEntry {
            path: rel_path_norm.clone(),
            sha256: actual_file_sha256.clone(),
        });

        let reader = BufReader::new(std::io::Cursor::new(&file_bytes));

        for (line_idx, line_res) in reader.lines().enumerate() {
            let line_num = line_idx + 1;
            let raw_line = line_res.map_err(|e| {
                format!(
                    "Failed to read line {} in {:?}: {}",
                    line_num, src_file_path, e
                )
            })?;

            match entry.document_format.as_str() {
                "one-document-per-line" => {
                    let trimmed = raw_line.trim();
                    if trimmed.is_empty() {
                        return Err(format!(
                            "Invalid source record: empty or whitespace-only document at '{}:{}'",
                            rel_path_norm, line_num
                        ));
                    }

                    let doc_id = format!("{}:{}", rel_path_norm, line_num);
                    if !seen_doc_ids.insert(doc_id.clone()) {
                        return Err(format!(
                            "Duplicate document_id '{}' generated in corpus '{}'",
                            doc_id, entry.corpus_id
                        ));
                    }

                    let record = CanonicalDocumentRecord {
                        corpus_id: entry.corpus_id.clone(),
                        document_id: doc_id,
                        source_file: rel_path_norm.clone(),
                        source_record: line_num,
                        source_sha256: actual_file_sha256.clone(),
                        text: trimmed.to_string(),
                    };

                    let json = serde_json::to_string(&record)
                        .map_err(|e| format!("Serialization error: {}", e))?;
                    writeln!(docs_file, "{}", json)
                        .map_err(|e| format!("Failed to write documents.jsonl: {}", e))?;
                    doc_count += 1;
                }
                "jsonl" => {
                    let trimmed = raw_line.trim();
                    if trimmed.is_empty() {
                        return Err(format!(
                            "Invalid source record: empty or whitespace-only document at '{}:{}'",
                            rel_path_norm, line_num
                        ));
                    }

                    let val: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
                        format!(
                            "JSONL parse error at '{}:{}': {}",
                            rel_path_norm, line_num, e
                        )
                    })?;

                    let id_field = entry.document_id_field.as_deref().unwrap();
                    let text_field = entry.text_field.as_deref().unwrap();

                    let raw_doc_id =
                        val.get(id_field).and_then(|v| v.as_str()).ok_or_else(|| {
                            format!(
                                "Missing or invalid document_id field '{}' at '{}:{}'",
                                id_field, rel_path_norm, line_num
                            )
                        })?;

                    let text = val
                        .get(text_field)
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            format!(
                                "Missing or invalid text field '{}' at '{}:{}'",
                                text_field, rel_path_norm, line_num
                            )
                        })?;

                    let trimmed_text = text.trim();
                    if trimmed_text.is_empty() {
                        return Err(format!(
                            "Invalid source record: empty or whitespace-only text at '{}:{}'",
                            rel_path_norm, line_num
                        ));
                    }

                    let doc_id = format!("{}:{}", rel_path_norm, raw_doc_id);
                    if !seen_doc_ids.insert(doc_id.clone()) {
                        return Err(format!(
                            "Duplicate document_id '{}' in corpus '{}'",
                            doc_id, entry.corpus_id
                        ));
                    }

                    let record = CanonicalDocumentRecord {
                        corpus_id: entry.corpus_id.clone(),
                        document_id: doc_id,
                        source_file: rel_path_norm.clone(),
                        source_record: line_num,
                        source_sha256: actual_file_sha256.clone(),
                        text: trimmed_text.to_string(),
                    };

                    let json = serde_json::to_string(&record)
                        .map_err(|e| format!("Serialization error: {}", e))?;
                    writeln!(docs_file, "{}", json)
                        .map_err(|e| format!("Failed to write documents.jsonl: {}", e))?;
                    doc_count += 1;
                }
                other => {
                    return Err(format!(
                        "Unsupported document format '{}' in corpus '{}'",
                        other, entry.corpus_id
                    ));
                }
            }
        }
    }

    docs_file
        .flush()
        .map_err(|e| format!("Flush error: {}", e))?;

    let docs_sha256 = calculate_file_sha256(&docs_file_path)?;
    source_files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(CanonicalCorpusManifestEntry {
        corpus_id: entry.corpus_id.clone(),
        documents_file: format!("{}/documents.jsonl", entry.corpus_id),
        documents_sha256: docs_sha256,
        document_count: doc_count,
        source_files,
    })
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

/// Imports all registered corpora into `data/imported-canonical/` atomically.
pub fn import_all_corpora<P: AsRef<Path>>(
    root_dir: P,
) -> Result<Vec<CorpusImportSummaryReport>, String> {
    let root = root_dir.as_ref();
    let lock_path = root.join("data/imported-canonical.lock");
    let lock = LockFileGuard::acquire(&lock_path)?;

    let registry_path = root.join("data/source-registry/corpora.toml");
    if !registry_path.exists() {
        return Err(format!("Corpus registry missing at {:?}", registry_path));
    }

    let registry_sha256 = calculate_file_sha256(&registry_path)?;
    let registry = CorpusRegistry::load_from_file(&registry_path)?;

    if registry.corpora.is_empty() {
        return Err("No registered corpora found in corpora.toml".to_string());
    }

    let mut registered_corpora = registry.corpora.clone();
    registered_corpora.sort_by(|a, b| a.corpus_id.cmp(&b.corpus_id));

    println!("=== Kurmancî Atomic Canonical Corpus Importer ===");
    for entry in &registered_corpora {
        println!("  Verifying source integrity for '{}'...", entry.corpus_id);
        registry.verify_corpus_files(entry, root)?;
    }

    let stage_dir = root.join("data/imported-canonical.tmp_stage");
    let backup_dir = root.join("data/imported-canonical.tmp_backup");
    let target_dir = root.join("data/imported-canonical");

    if stage_dir.exists() {
        remove_dir_or_file(&stage_dir)
            .map_err(|e| format!("Failed to clean existing stage dir {:?}: {}", stage_dir, e))?;
    }
    fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("Failed to create stage dir {:?}: {}", stage_dir, e))?;

    let mut manifest_entries = Vec::new();
    let mut reports = Vec::new();

    for entry in &registered_corpora {
        println!(
            "  Ingesting canonical documents for '{}'...",
            entry.corpus_id
        );
        let manifest_entry = process_corpus_canonical(entry, root, &stage_dir)?;
        manifest_entries.push(manifest_entry.clone());

        let doc_file_meta = fs::metadata(stage_dir.join(&entry.corpus_id).join("documents.jsonl"))
            .map_err(|e| {
                format!(
                    "Failed to read metadata for documents.jsonl in corpus '{}': {}",
                    entry.corpus_id, e
                )
            })?;

        let report = CorpusImportSummaryReport {
            corpus_id: entry.corpus_id.clone(),
            corpus_name: entry.corpus_name.clone(),
            language: entry.language.clone(),
            license: entry.license.clone(),
            imported_files_count: entry.files.len(),
            total_bytes: doc_file_meta.len(),
            document_count: manifest_entry.document_count,
            checksum_verification_passed: true,
        };
        reports.push(report);
    }

    manifest_entries.sort_by(|a, b| a.corpus_id.cmp(&b.corpus_id));

    let canonical_manifest = CanonicalImportManifest {
        schema_version: CANONICAL_SCHEMA_VERSION.to_string(),
        registry_sha256,
        corpora: manifest_entries,
    };

    let manifest_path = stage_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&canonical_manifest)
        .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| format!("Failed to write manifest {:?}: {}", manifest_path, e))?;

    // Atomic Directory Swap
    if backup_dir.exists() {
        remove_dir_or_file(&backup_dir).map_err(|e| {
            format!(
                "Failed to clean existing backup dir {:?}: {}",
                backup_dir, e
            )
        })?;
    }

    if target_dir.exists() {
        fs::rename(&target_dir, &backup_dir).map_err(|e| {
            format!(
                "Failed to rename target_dir {:?} to backup_dir {:?}: {}",
                target_dir, backup_dir, e
            )
        })?;
    }

    match fs::rename(&stage_dir, &target_dir) {
        Ok(()) => {
            if backup_dir.exists() {
                if let Err(e) = remove_dir_or_file(&backup_dir) {
                    eprintln!(
                        "Warning: failed to clean up backup dir {:?}: {}",
                        backup_dir, e
                    );
                }
            }
        }
        Err(err) => {
            if backup_dir.exists() {
                if let Err(rollback_err) = fs::rename(&backup_dir, &target_dir) {
                    return Err(format!(
                        "Failed to install canonical import dir {:?}: {}; rollback also failed: {}",
                        target_dir, err, rollback_err
                    ));
                }
            }
            return Err(format!(
                "Failed to install canonical import dir {:?}: {}",
                target_dir, err
            ));
        }
    }

    println!(
        "⚡ CANONICAL IMPORT SUCCESSFUL across {} corpora!",
        reports.len()
    );
    lock.release()?;
    Ok(reports)
}

/// Legacy wrapper importing a single corpus or all corpora.
pub fn import_corpus<P: AsRef<Path>>(
    corpus_id: &str,
    root_dir: P,
) -> Result<CorpusImportSummaryReport, String> {
    let root = root_dir.as_ref();
    let registry_path = root.join("data/source-registry/corpora.toml");
    if registry_path.exists() {
        let registry = CorpusRegistry::load_from_file(&registry_path)?;
        if registry.find_corpus(corpus_id).is_none() {
            return Err(format!(
                "Unknown corpus_id '{}' — not registered in corpora.toml",
                corpus_id
            ));
        }
    }

    let reports = import_all_corpora(root)?;
    reports
        .into_iter()
        .find(|r| r.corpus_id == corpus_id)
        .ok_or_else(|| {
            format!(
                "Unknown corpus_id '{}' — not registered in corpora.toml",
                corpus_id
            )
        })
}
