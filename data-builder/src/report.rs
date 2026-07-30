use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildReport {
    pub timestamp: String,
    pub total_source_entries: usize,
    pub validated_entries: usize,
    pub unique_lexicon_entries: usize,
    pub binary_pack_size_bytes: usize,
    pub checksum_sha256: String,
    pub status: String,
}

pub fn generate_and_save_report<P: AsRef<Path>>(
    reports_dir: P,
    report: &BuildReport,
) -> Result<(), String> {
    let dir = reports_dir.as_ref();
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create reports directory: {}", e))?;

    let report_path = dir.join("build-report.json");
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| format!("Failed to serialize report: {}", e))?;
    fs::write(report_path, json).map_err(|e| format!("Failed to write build report: {}", e))?;

    Ok(())
}
