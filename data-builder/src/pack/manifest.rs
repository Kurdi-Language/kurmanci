//! Language pack manifest schema (`language-pack-manifest-v1`) and dynamic license generator.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::sources::SourceRegistry;

pub const LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION: &str = "language-pack-manifest-v1";

/// SPDX License entry in manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataLicenseEntry {
    pub source_id: String,
    pub spdx: String,
}

/// Authoritative language pack manifest schema (`language-pack-manifest-v1`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackManifest {
    pub schema_version: String,
    pub pack_id: String,
    pub pack_format_version: u32,
    pub language: String,
    pub is_default: bool,
    pub is_experimental: bool,
    pub model_profile: String,
    pub frequency_entry_count: usize,
    pub bigram_count: usize,
    pub trigram_count: usize,
    pub manual_seed_selected_count: usize,
    pub external_approved_selected_count: usize,
    pub external_metadata_replacement_selected_count: usize,
    pub external_experimental_selected_count: usize,
    pub external_unreviewed_selected_count: usize,
    pub external_excluded_by_status_count: usize,
    pub external_discarded_by_collision_count: usize,
    pub final_unique_entry_count: usize,
    pub pack_policy_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_decisions_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_queue_manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controlled_review_report_manifest_sha256: Option<String>,
    pub binary_sha256: String,
    pub binary_size_bytes: u64,
    pub data_licenses: Vec<DataLicenseEntry>,
    pub attribution_files: Vec<String>,
}

/// Generates source-derived licensing array and `attribution.txt` content dynamically from registry.
pub fn generate_licensing_and_attribution<P: AsRef<Path>>(
    root: P,
    included_sources: &[String],
) -> Result<(Vec<DataLicenseEntry>, String), String> {
    let registry_path = root.as_ref().join("data/source-registry/sources.toml");
    let registry = SourceRegistry::load_from_file(&registry_path)?;

    let mut licenses = Vec::new();
    let mut attribution_sections = Vec::new();

    for source_id in included_sources {
        let src_entry = registry
            .sources
            .iter()
            .find(|s| s.source_id == *source_id)
            .ok_or_else(|| {
                format!(
                    "Source '{}' not registered in data/source-registry/sources.toml",
                    source_id
                )
            })?;

        licenses.push(DataLicenseEntry {
            source_id: source_id.clone(),
            spdx: src_entry.license.clone(),
        });

        attribution_sections.push(format!(
            "=== Source: {} ===\n\
            License: {}\n\
            Upstream Project: {}\n\
            Author: {}\n\
            Source Revision: {}\n\
            Modification Notice: Filtered, normalized, and converted to binary language pack format.\n",
            source_id,
            src_entry.license,
            src_entry.source_name,
            src_entry.author,
            src_entry.version
        ));
    }

    let attribution_text = attribution_sections.join("\n");
    Ok((licenses, attribution_text))
}

/// Calculates SHA-256 hex string for given bytes.
pub fn calculate_bytes_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Calculates SHA-256 hex string for file path.
pub fn calculate_file_sha256<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let p = path.as_ref();
    let content = fs::read(p).map_err(|e| format!("Failed to read {:?}: {}", p, e))?;
    Ok(calculate_bytes_sha256(&content))
}

/// Strictly validates manifest invariants and decoded binary counts for all built packs.
pub fn validate_all_pack_manifests<P: AsRef<Path>>(root_dir: P) -> Result<(), String> {
    let root = root_dir.as_ref();
    let packs_dir = root.join("data/build/packs");

    let expected_packs = vec![
        ("seed", true, false),
        ("reviewed", false, false),
        ("experimental-full", false, true),
    ];

    for (pack_id, expected_default, expected_experimental) in expected_packs {
        let pack_dir = packs_dir.join(pack_id);
        if !pack_dir.exists() {
            return Err(format!("Pack directory missing at {:?}", pack_dir));
        }

        // Verify exact 5-artifact set
        let dir_entries: Vec<String> = fs::read_dir(&pack_dir)
            .map_err(|e| format!("Failed to read pack dir {:?}: {}", pack_dir, e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Dir entry error in {:?}: {}", pack_dir, e))?
            .into_iter()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        let mut expected_artifacts = vec![
            "artifacts.sha256".to_string(),
            "attribution.txt".to_string(),
            "collision-report.jsonl".to_string(),
            "lexicon.bin".to_string(),
            "manifest.json".to_string(),
        ];
        expected_artifacts.sort();
        let mut actual_artifacts = dir_entries.clone();
        actual_artifacts.sort();

        if actual_artifacts != expected_artifacts {
            return Err(format!(
                "Pack '{}' artifact set mismatch: expected {:?}, found {:?}",
                pack_id, expected_artifacts, actual_artifacts
            ));
        }

        // Read manifest
        let manifest_path = pack_dir.join("manifest.json");
        let manifest_bytes = fs::read(&manifest_path)
            .map_err(|e| format!("Failed to read manifest {:?}: {}", manifest_path, e))?;
        let manifest: PackManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| format!("Failed to parse manifest {:?}: {}", manifest_path, e))?;

        if manifest.pack_id != pack_id {
            return Err(format!(
                "Manifest pack_id '{}' mismatch (expected '{}')",
                manifest.pack_id, pack_id
            ));
        }
        if manifest.is_default != expected_default {
            return Err(format!(
                "Pack '{}' is_default '{}' mismatch (expected '{}')",
                pack_id, manifest.is_default, expected_default
            ));
        }
        if manifest.is_experimental != expected_experimental {
            return Err(format!(
                "Pack '{}' is_experimental '{}' mismatch (expected '{}')",
                pack_id, manifest.is_experimental, expected_experimental
            ));
        }
        if manifest.model_profile != "none" {
            return Err(format!(
                "Pack '{}' model_profile '{}' mismatch (expected 'none')",
                pack_id, manifest.model_profile
            ));
        }
        if manifest.frequency_entry_count != 0 {
            return Err(format!(
                "Pack '{}' frequency_entry_count must be 0 (found {})",
                pack_id, manifest.frequency_entry_count
            ));
        }
        if manifest.bigram_count != 0 {
            return Err(format!(
                "Pack '{}' bigram_count must be 0 (found {})",
                pack_id, manifest.bigram_count
            ));
        }
        if manifest.trigram_count != 0 {
            return Err(format!(
                "Pack '{}' trigram_count must be 0 (found {})",
                pack_id, manifest.trigram_count
            ));
        }

        // Verify binary SHA-256 and size
        let bin_path = pack_dir.join("lexicon.bin");
        let bin_bytes = fs::read(&bin_path)
            .map_err(|e| format!("Failed to read binary pack {:?}: {}", bin_path, e))?;
        let actual_bin_sha = calculate_bytes_sha256(&bin_bytes);
        if actual_bin_sha != manifest.binary_sha256 {
            return Err(format!(
                "Pack '{}' binary_sha256 mismatch: manifest {}, actual {}",
                pack_id, manifest.binary_sha256, actual_bin_sha
            ));
        }
        if bin_bytes.len() as u64 != manifest.binary_size_bytes {
            return Err(format!(
                "Pack '{}' binary_size_bytes mismatch: manifest {}, actual {}",
                pack_id,
                manifest.binary_size_bytes,
                bin_bytes.len()
            ));
        }

        // Verify artifacts.sha256 file hashes and exact 4-path set
        let art_path = pack_dir.join("artifacts.sha256");
        let art_content = fs::read_to_string(&art_path).map_err(|e| e.to_string())?;

        let expected_rel_paths: BTreeSet<String> = [
            format!("data/build/packs/{}/lexicon.bin", pack_id),
            format!("data/build/packs/{}/manifest.json", pack_id),
            format!("data/build/packs/{}/collision-report.jsonl", pack_id),
            format!("data/build/packs/{}/attribution.txt", pack_id),
        ]
        .into_iter()
        .collect();

        let mut actual_manifest_paths = BTreeSet::new();

        for line in art_content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 2 {
                return Err(format!("Malformed line in {:?}: '{}'", art_path, line));
            }
            let exp_hash = parts[0];
            let rel_file = parts[1];

            if rel_file.starts_with('/') {
                return Err(format!("Absolute path in artifacts.sha256: '{}'", rel_file));
            }
            if rel_file.contains("/../") || rel_file.starts_with("../") || rel_file.ends_with("/..")
            {
                return Err(format!(
                    "Path traversal in artifacts.sha256: '{}'",
                    rel_file
                ));
            }
            if actual_manifest_paths.contains(rel_file) {
                return Err(format!(
                    "Duplicate path in artifacts.sha256: '{}'",
                    rel_file
                ));
            }

            if !expected_rel_paths.contains(rel_file) {
                return Err(format!(
                    "Unexpected path in artifacts.sha256 for pack '{}': '{}'",
                    pack_id, rel_file
                ));
            }

            actual_manifest_paths.insert(rel_file.to_string());

            let fname = rel_file
                .split('/')
                .next_back()
                .ok_or_else(|| "Invalid rel_file path".to_string())?;
            let target_f = pack_dir.join(fname);
            let act_hash = calculate_file_sha256(&target_f)?;
            if act_hash != exp_hash {
                return Err(format!(
                    "Artifact hash mismatch for {:?}: manifest {}, actual {}",
                    target_f, exp_hash, act_hash
                ));
            }
        }

        if actual_manifest_paths != expected_rel_paths {
            return Err(format!(
                "Pack '{}' artifacts.sha256 path set mismatch: expected {:?}, found {:?}",
                pack_id, expected_rel_paths, actual_manifest_paths
            ));
        }

        // Verify decoded binary count
        let mut engine = kurmanci_engine::Engine::new();
        let loaded_count = engine
            .load_binary_pack(&bin_bytes)
            .map_err(|e| format!("Engine failed to load pack '{}': {}", pack_id, e))?;

        if loaded_count != manifest.final_unique_entry_count {
            return Err(format!(
                "Pack '{}' decoded entry count {} != manifest final_unique_entry_count {}",
                pack_id, loaded_count, manifest.final_unique_entry_count
            ));
        }

        // Verify source-derived license consistency
        let license_source_ids: Vec<String> = manifest
            .data_licenses
            .iter()
            .map(|l| l.source_id.clone())
            .collect();
        let attr_text =
            fs::read_to_string(pack_dir.join("attribution.txt")).map_err(|e| e.to_string())?;
        for lic_id in license_source_ids {
            if !attr_text.contains(&format!("=== Source: {} ===", lic_id)) {
                return Err(format!(
                    "Pack '{}' license '{}' missing from attribution.txt",
                    pack_id, lic_id
                ));
            }
        }
    }
    Ok(())
}
