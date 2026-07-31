//! Corpus Importer module for importing registered text corpora.

use super::registry::CorpusRegistry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Summary report emitted when a corpus is successfully imported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusImportSummaryReport {
    pub corpus_id: String,
    pub corpus_name: String,
    pub language: String,
    pub license: String,
    pub imported_files_count: usize,
    pub total_bytes: u64,
    pub checksum_verification_passed: bool,
}

/// Imports a registered corpus by verifying registry entry, checksums, and copying files.
pub fn import_corpus<P: AsRef<Path>>(
    corpus_id: &str,
    root_dir: P,
) -> Result<CorpusImportSummaryReport, String> {
    let root = root_dir.as_ref();

    let registry_path = root.join("data/source-registry/corpora.toml");
    if !registry_path.exists() {
        return Err(format!("Corpus registry missing at {:?}", registry_path));
    }

    let registry = CorpusRegistry::load_from_file(&registry_path)?;
    let entry = registry.find_corpus(corpus_id).ok_or_else(|| {
        format!(
            "Unknown corpus_id '{}' — not registered in corpora.toml",
            corpus_id
        )
    })?;

    // Verify file existence and SHA-256 checksums
    println!("  [1/3] Verifying corpus checksums for '{}'...", corpus_id);
    registry.verify_corpus_files(entry, root)?;

    let imported_parent = root.join("data/imported");
    fs::create_dir_all(&imported_parent).map_err(|e| {
        format!(
            "Failed to create imported parent dir {:?}: {}",
            imported_parent, e
        )
    })?;

    let dest_dir = imported_parent.join(corpus_id);
    let stage_dest = dest_dir.with_extension(format!(
        "tmp_import_stage_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));

    if stage_dest.exists() {
        let _ = fs::remove_dir_all(&stage_dest);
    }
    fs::create_dir_all(&stage_dest)
        .map_err(|e| format!("Failed to create stage import dir {:?}: {}", stage_dest, e))?;

    println!(
        "  [2/3] Copying preserved corpus files into data/imported/{}...",
        corpus_id
    );
    let mut total_bytes = 0u64;

    for file_entry in &entry.files {
        let src_path = root.join(&file_entry.path);
        let filename = Path::new(&file_entry.path)
            .file_name()
            .ok_or_else(|| format!("Invalid file path in corpus entry: {}", file_entry.path))?;
        let target_path = stage_dest.join(filename);

        let bytes_copied = fs::copy(&src_path, &target_path)
            .map_err(|e| format!("Failed to copy {:?} to {:?}: {}", src_path, target_path, e))?;
        total_bytes += bytes_copied;
    }

    let backup_dest = dest_dir.with_extension(format!(
        "tmp_import_backup_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));

    if backup_dest.exists() {
        let _ = fs::remove_dir_all(&backup_dest);
    }

    if dest_dir.exists() {
        fs::rename(&dest_dir, &backup_dest).map_err(|e| {
            format!(
                "Failed to move dest_dir {:?} to backup_dest {:?}: {}",
                dest_dir, backup_dest, e
            )
        })?;
    }

    match fs::rename(&stage_dest, &dest_dir) {
        Ok(()) => {
            if backup_dest.exists() {
                let _ = fs::remove_dir_all(&backup_dest);
            }
        }
        Err(err) => {
            if backup_dest.exists() {
                let _ = fs::rename(&backup_dest, &dest_dir);
            }
            return Err(format!(
                "Failed to install imported corpus dir {:?}: {}",
                dest_dir, err
            ));
        }
    }

    let report = CorpusImportSummaryReport {
        corpus_id: corpus_id.to_string(),
        corpus_name: entry.corpus_name.clone(),
        language: entry.language.clone(),
        license: entry.license.clone(),
        imported_files_count: entry.files.len(),
        total_bytes,
        checksum_verification_passed: true,
    };

    println!(
        "  [3/3] Writing import report to data/reports/corpora/{}/import-summary.json...",
        corpus_id
    );
    let report_dir = root.join("data/reports/corpora").join(corpus_id);
    fs::create_dir_all(&report_dir)
        .map_err(|e| format!("Failed to create report dir {:?}: {}", report_dir, e))?;
    let report_path = report_dir.join("import-summary.json");
    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("Failed to serialize import summary: {}", e))?;
    fs::write(&report_path, json)
        .map_err(|e| format!("Failed to write report {:?}: {}", report_path, e))?;

    Ok(report)
}
