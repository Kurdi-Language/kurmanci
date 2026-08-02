//! Corpus Partitioning Engine for Leakage-Free Train/Development/Evaluation Splits.

use super::audit::{compute_duplicate_group_id, DOCUMENT_DEDUP_VERSION, SENTENCE_DEDUP_VERSION};
use super::importer::{CanonicalDocumentRecord, CANONICAL_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

pub const PARTITION_POLICY_VERSION: &str = "kurmanci-partition-v1";

/// Output record inside `train.jsonl`, `development.jsonl`, or `evaluation.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartitionDocumentRecord {
    pub partition: String,
    pub corpus_id: String,
    pub document_id: String,
    pub duplicate_group_id: String,
    pub is_duplicate: bool,
    pub canonical_corpus_id: String,
    pub canonical_document_id: String,
    pub source_file: String,
    pub source_sha256: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartitionBuildManifest {
    pub partition_policy_version: String,
    pub document_normalization_version: String,
    pub sentence_normalization_version: String,
    pub canonical_schema_version: String,
    pub registry_sha256: String,
    pub canonical_input_manifest_sha256: String,
    pub train_documents: usize,
    pub development_documents: usize,
    pub evaluation_documents: usize,
    pub duplicate_group_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionSummary {
    pub total_documents: usize,
    pub train_documents: usize,
    pub development_documents: usize,
    pub evaluation_documents: usize,
    pub duplicate_group_count: usize,
    pub manifest: PartitionBuildManifest,
}

/// Assigns a domain-separated partition ("train", "development", "evaluation") for a key.
pub fn assign_partition(key: &[u8]) -> &'static str {
    let mut hasher = Sha256::new();
    hasher.update(b"kurmanci-partition-v1\0");
    hasher.update(key);
    let digest = hasher.finalize();

    let bytes: [u8; 8] = digest[0..8].try_into().unwrap();
    let num = u64::from_be_bytes(bytes);
    let bucket = num % 100;

    if bucket < 80 {
        "train"
    } else if bucket < 90 {
        "development"
    } else {
        "evaluation"
    }
}

/// Partitions canonical corpus documents into `data/build/corpus-partitions/`.
pub fn partition_corpora<P: AsRef<Path>>(root_dir: P) -> Result<PartitionSummary, String> {
    let root = root_dir.as_ref();
    let manifest = super::importer::verify_canonical_manifest(root)?;
    let imported_dir = root.join("data/imported-canonical");
    let manifest_path = imported_dir.join("manifest.json");

    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|e| format!("Read manifest error {:?}: {}", manifest_path, e))?;
    let canonical_input_manifest_sha256 = format!("{:x}", Sha256::digest(&manifest_bytes));

    // 1. Ingest all canonical document records
    let mut all_records: Vec<CanonicalDocumentRecord> = Vec::new();
    for corpus_entry in &manifest.corpora {
        let doc_path = imported_dir.join(&corpus_entry.documents_file);
        let file =
            File::open(&doc_path).map_err(|e| format!("Read doc error {:?}: {}", doc_path, e))?;

        for line_res in BufReader::new(file).lines() {
            let line = line_res.map_err(|e| format!("Read line error: {}", e))?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let rec: CanonicalDocumentRecord =
                serde_json::from_str(trimmed).map_err(|e| format!("JSONL record error: {}", e))?;
            all_records.push(rec);
        }
    }

    // 2. Group by duplicate_group_id
    let mut groups: BTreeMap<String, Vec<CanonicalDocumentRecord>> = BTreeMap::new();
    for rec in all_records {
        let gid = compute_duplicate_group_id(&rec.text);
        groups.entry(gid).or_default().push(rec);
    }

    // 3. Assign partition per group and determine canonical representative
    let mut partition_records: Vec<PartitionDocumentRecord> = Vec::new();
    let mut train_docs = 0usize;
    let mut dev_docs = 0usize;
    let mut eval_docs = 0usize;

    for (group_id, recs) in &groups {
        let is_dup = recs.len() > 1;

        // Canonical representative is lexicographically smallest (corpus_id, document_id)
        let canon_rec = recs
            .iter()
            .min_by(|a, b| {
                a.corpus_id
                    .cmp(&b.corpus_id)
                    .then_with(|| a.document_id.cmp(&b.document_id))
            })
            .ok_or_else(|| format!("Empty duplicate group '{}'", group_id))?;
        let canon_corpus = canon_rec.corpus_id.clone();
        let canon_doc = canon_rec.document_id.clone();

        let partition_key = if is_dup {
            format!("duplicate\0{}", group_id)
        } else {
            let sole = &recs[0];
            format!("document\0{}\0{}", sole.corpus_id, sole.document_id)
        };

        let partition_name = assign_partition(partition_key.as_bytes()).to_string();

        for rec in recs {
            match partition_name.as_str() {
                "train" => train_docs += 1,
                "development" => dev_docs += 1,
                "evaluation" => eval_docs += 1,
                _ => {}
            }

            partition_records.push(PartitionDocumentRecord {
                partition: partition_name.clone(),
                corpus_id: rec.corpus_id.clone(),
                document_id: rec.document_id.clone(),
                duplicate_group_id: group_id.clone(),
                is_duplicate: is_dup,
                canonical_corpus_id: canon_corpus.clone(),
                canonical_document_id: canon_doc.clone(),
                source_file: rec.source_file.clone(),
                source_sha256: rec.source_sha256.clone(),
                text: rec.text.clone(),
            });
        }
    }

    // Sort deterministically by corpus_id ASC, document_id ASC
    partition_records.sort_by(|a, b| {
        a.corpus_id
            .cmp(&b.corpus_id)
            .then_with(|| a.document_id.cmp(&b.document_id))
    });

    // 4. Write `data/build/corpus-partitions/` using atomic staged directory swap
    let build_part_dir = root.join("data/build/corpus-partitions");
    let stage_build_dir = root.join("data/build/corpus-partitions.tmp_stage");
    let backup_build_dir = root.join("data/build/corpus-partitions.tmp_backup");

    if stage_build_dir.exists() {
        remove_dir_or_file(&stage_build_dir).map_err(|e| {
            format!(
                "Failed to clean stage build dir {:?}: {}",
                stage_build_dir, e
            )
        })?;
    }
    fs::create_dir_all(&stage_build_dir).map_err(|e| {
        format!(
            "Failed to create build partition stage dir {:?}: {}",
            stage_build_dir, e
        )
    })?;

    let mut train_file = File::create(stage_build_dir.join("train.jsonl"))
        .map_err(|e| format!("Create train.jsonl failed: {}", e))?;
    let mut dev_file = File::create(stage_build_dir.join("development.jsonl"))
        .map_err(|e| format!("Create development.jsonl failed: {}", e))?;
    let mut eval_file = File::create(stage_build_dir.join("evaluation.jsonl"))
        .map_err(|e| format!("Create evaluation.jsonl failed: {}", e))?;

    for rec in &partition_records {
        let json =
            serde_json::to_string(rec).map_err(|e| format!("Serialize record failed: {}", e))?;
        match rec.partition.as_str() {
            "train" => writeln!(train_file, "{}", json)
                .map_err(|e| format!("Write train failed: {}", e))?,
            "development" => {
                writeln!(dev_file, "{}", json).map_err(|e| format!("Write dev failed: {}", e))?
            }
            "evaluation" => {
                writeln!(eval_file, "{}", json).map_err(|e| format!("Write eval failed: {}", e))?
            }
            _ => {}
        }
    }

    let build_manifest = PartitionBuildManifest {
        partition_policy_version: PARTITION_POLICY_VERSION.to_string(),
        document_normalization_version: DOCUMENT_DEDUP_VERSION.to_string(),
        sentence_normalization_version: SENTENCE_DEDUP_VERSION.to_string(),
        canonical_schema_version: CANONICAL_SCHEMA_VERSION.to_string(),
        registry_sha256: manifest.registry_sha256,
        canonical_input_manifest_sha256,
        train_documents: train_docs,
        development_documents: dev_docs,
        evaluation_documents: eval_docs,
        duplicate_group_count: groups.len(),
    };

    let manifest_json = serde_json::to_string_pretty(&build_manifest)
        .map_err(|e| format!("Serialize manifest failed: {}", e))?;
    fs::write(stage_build_dir.join("manifest.json"), manifest_json)
        .map_err(|e| format!("Write manifest.json failed: {}", e))?;

    let summary = PartitionSummary {
        total_documents: partition_records.len(),
        train_documents: train_docs,
        development_documents: dev_docs,
        evaluation_documents: eval_docs,
        duplicate_group_count: groups.len(),
        manifest: build_manifest,
    };

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

    // Swap build directory
    if backup_build_dir.exists() {
        remove_dir_or_file(&backup_build_dir)
            .map_err(|e| format!("Failed to clean backup build dir: {}", e))?;
    }
    if build_part_dir.exists() {
        fs::rename(&build_part_dir, &backup_build_dir)
            .map_err(|e| format!("Failed to rename build partition dir to backup: {}", e))?;
    }
    match fs::rename(&stage_build_dir, &build_part_dir) {
        Ok(()) => {
            if backup_build_dir.exists() {
                if let Err(e) = remove_dir_or_file(&backup_build_dir) {
                    eprintln!(
                        "Warning: failed to clean up backup dir {:?}: {}",
                        backup_build_dir, e
                    );
                }
            }
        }
        Err(err) => {
            if backup_build_dir.exists() {
                if let Err(rollback_err) = fs::rename(&backup_build_dir, &build_part_dir) {
                    return Err(format!(
                        "Failed to install build partition dir: {}; rollback also failed: {}",
                        err, rollback_err
                    ));
                }
            }
            return Err(format!("Failed to install build partition dir: {}", err));
        }
    }

    // 5. Write Report Suite `data/reports/corpus-partitions/` using atomic staged swap
    let reports_dir = root.join("data/reports/corpus-partitions");
    let stage_reports_dir = root.join("data/reports/corpus-partitions.tmp_stage");
    let backup_reports_dir = root.join("data/reports/corpus-partitions.tmp_backup");

    if stage_reports_dir.exists() {
        remove_dir_or_file(&stage_reports_dir)
            .map_err(|e| format!("Failed to clean stage reports dir: {}", e))?;
    }
    fs::create_dir_all(&stage_reports_dir).map_err(|e| {
        format!(
            "Failed to create partition reports stage dir {:?}: {}",
            stage_reports_dir, e
        )
    })?;

    fs::write(
        stage_reports_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Write summary failed: {}", e))?;

    let readme = format!(
        "# Kurmancî Corpus Partition Report\n\n\
        - **Partition Policy**: {}\n\
        - **Total Documents**: {}\n\
        - **Train Documents (80%)**: {}\n\
        - **Dev Documents (10%)**: {}\n\
        - **Eval Documents (10%)**: {}\n\
        - **Duplicate Groups**: {}\n",
        PARTITION_POLICY_VERSION,
        summary.total_documents,
        summary.train_documents,
        summary.development_documents,
        summary.evaluation_documents,
        summary.duplicate_group_count
    );
    fs::write(stage_reports_dir.join("README.md"), readme)
        .map_err(|e| format!("Write README failed: {}", e))?;

    // Manifest generation
    let mut manifest_entries = Vec::new();
    let expected = ["summary.json", "README.md"];
    for name in &expected {
        let fpath = stage_reports_dir.join(name);
        let content = fs::read(&fpath).map_err(|e| format!("Read report file failed: {}", e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        let rel_path = format!("data/reports/corpus-partitions/{}", name);
        manifest_entries.push(format!("{} {}", hash, rel_path));
    }
    manifest_entries.sort();
    let manifest_bytes = manifest_entries.join("\n") + "\n";
    fs::write(stage_reports_dir.join("artifacts.sha256"), manifest_bytes)
        .map_err(|e| format!("Write artifacts.sha256 failed: {}", e))?;

    // Swap report directory
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
                        "Failed to install partition report dir: {}; rollback also failed: {}",
                        err, rollback_err
                    ));
                }
            }
            return Err(format!("Failed to install partition report dir: {}", err));
        }
    }

    println!("⚡ CORPUS PARTITIONING COMPLETED! Outputs at data/build/corpus-partitions/");
    Ok(summary)
}
