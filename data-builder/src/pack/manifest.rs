//! Language pack manifest schema (`language-pack-manifest-v1`) and dynamic license generator.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::pack::language_model::{load_language_model, LanguageModel};
use crate::pack::policy::{PackPolicyConfig, MODEL_PROFILE_NONE};
use crate::sources::SourceRegistry;

pub const LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION: &str = "language-pack-manifest-v1";

/// SPDX License entry in manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataLicenseEntry {
    pub source_id: String,
    pub spdx: String,
}

/// Source-level review provenance entry in manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceReviewProvenance {
    pub source_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decisions_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_artifact_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_provenance_manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_queue_manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controlled_review_report_manifest_sha256: Option<String>,
}

/// Provenance and licensing record for the language model a pack carries. The
/// `redistribution_determination` is recorded verbatim from the model manifest and is
/// `pending-review` until a human licensing review sets it; the code makes no legal judgement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageModelProvenance {
    pub model_id: String,
    pub model_manifest_sha256: String,
    pub corpus_id: String,
    pub corpus_version: String,
    pub corpus_name: String,
    /// Every corpus that contributed statistics (the model's own corpus only).
    pub contributing_corpora: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus_source_artifact_sha256: Option<String>,
    pub corpus_documents_sha256: String,
    pub train_partition_sha256: String,
    /// SHA-256 of the corpus-scoped TRAIN document set the statistics were computed from.
    pub train_document_set_sha256: String,
    pub license: String,
    pub license_spdx: String,
    pub license_url: String,
    pub attribution: String,
    pub redistribution_determination: String,
}

impl LanguageModelProvenance {
    /// The one authoritative way to derive pack provenance from a loaded model; used by the
    /// builder to record it and by the validator to check it.
    pub fn from_model(model: &LanguageModel) -> Self {
        let m = &model.manifest;
        Self {
            model_id: m.model_id.clone(),
            model_manifest_sha256: model.manifest_sha256.clone(),
            corpus_id: m.corpus_id.clone(),
            corpus_version: m.corpus_version.clone(),
            corpus_name: m.licensing.corpus_name.clone(),
            contributing_corpora: m.contributing_corpora.clone(),
            corpus_source_artifact_sha256: m.corpus_source_artifact_sha256.clone(),
            corpus_documents_sha256: m.corpus_documents_sha256.clone(),
            train_partition_sha256: m.train_partition_sha256.clone(),
            train_document_set_sha256: m.train_document_set_sha256.clone(),
            license: m.licensing.license.clone(),
            license_spdx: m.licensing.license_spdx.clone(),
            license_url: m.licensing.license_url.clone(),
            attribution: m.licensing.attribution.clone(),
            redistribution_determination: m.licensing.redistribution_determination.clone(),
        }
    }

    /// `data_licenses` entry a pack must carry for this model.
    pub fn data_license_entry(&self) -> DataLicenseEntry {
        DataLicenseEntry {
            source_id: language_model_source_id(&self.model_id),
            spdx: self.license_spdx.clone(),
        }
    }
}

/// `source_id` under which a language model appears in `data_licenses` and `attribution.txt`.
pub fn language_model_source_id(model_id: &str) -> String {
    format!("language-model:{}", model_id)
}

/// Checks the model-related fields of one pack manifest against the policy definition and
/// the committed model (loaded fail-closed through `load_language_model`). A `none` pack must
/// carry no model reference, provenance or `language-model:*` licence entry; a model-backed
/// pack must pin the model's manifest hash, carry provenance identical to
/// `LanguageModelProvenance::from_model`, and carry exactly one matching licence entry.
pub fn validate_pack_model_provenance<P: AsRef<Path>>(
    root: P,
    manifest: &PackManifest,
    policy: &PackPolicyConfig,
) -> Result<(), String> {
    let root = root.as_ref();
    let pack_id = &manifest.pack_id;
    let pack_def = policy
        .packs
        .get(pack_id)
        .ok_or_else(|| format!("Pack '{}' not declared in data/pack-policy.toml", pack_id))?;
    if manifest.model_profile != pack_def.model_profile {
        return Err(format!(
            "Pack '{}' model_profile '{}' mismatch (policy expects '{}')",
            pack_id, manifest.model_profile, pack_def.model_profile
        ));
    }
    let model_entries: Vec<&DataLicenseEntry> = manifest
        .data_licenses
        .iter()
        .filter(|l| l.source_id.starts_with("language-model:"))
        .collect();

    if !pack_def.uses_model() {
        if manifest.frequency_entry_count != 0
            || manifest.bigram_count != 0
            || manifest.trigram_count != 0
        {
            return Err(format!(
                "Pack '{}' model_profile '{}' must carry no model data (freq={}, bi={}, tri={})",
                pack_id,
                MODEL_PROFILE_NONE,
                manifest.frequency_entry_count,
                manifest.bigram_count,
                manifest.trigram_count
            ));
        }
        if manifest.language_model_id.is_some()
            || manifest.language_model_manifest_sha256.is_some()
            || manifest.language_model_provenance.is_some()
        {
            return Err(format!(
                "Pack '{}' model_profile '{}' must not reference a language model (id, manifest sha or provenance present)",
                pack_id, MODEL_PROFILE_NONE
            ));
        }
        if !model_entries.is_empty() {
            return Err(format!(
                "Pack '{}' model_profile '{}' must not carry language-model data_licenses entries (found {:?})",
                pack_id,
                MODEL_PROFILE_NONE,
                model_entries.iter().map(|l| &l.source_id).collect::<Vec<_>>()
            ));
        }
        return Ok(());
    }

    let model_id = pack_def
        .language_model
        .as_deref()
        .ok_or_else(|| format!("Pack '{}' policy lacks language_model", pack_id))?;
    if manifest.language_model_id.as_deref() != Some(model_id) {
        return Err(format!(
            "Pack '{}' language_model_id {:?} mismatch (policy expects '{}')",
            pack_id, manifest.language_model_id, model_id
        ));
    }
    let model = load_language_model(root, model_id)?;
    if manifest.language_model_manifest_sha256.as_deref() != Some(model.manifest_sha256.as_str()) {
        return Err(format!(
            "Pack '{}' language_model_manifest_sha256 {:?} does not match data/language-model/{}/manifest.json ({})",
            pack_id, manifest.language_model_manifest_sha256, model_id, model.manifest_sha256
        ));
    }
    let expected = LanguageModelProvenance::from_model(&model);
    match &manifest.language_model_provenance {
        Some(actual) if actual == &expected => {}
        Some(_) => {
            return Err(format!(
                "Pack '{}' language_model_provenance does not match the committed model '{}' (expected {:?})",
                pack_id, model_id, expected
            ));
        }
        None => {
            return Err(format!(
                "Pack '{}' model_profile '{}' lacks language_model_provenance for model '{}'",
                pack_id, manifest.model_profile, model_id
            ));
        }
    }
    let expected_entry = expected.data_license_entry();
    match model_entries.as_slice() {
        [single] if **single == expected_entry => {}
        [] => {
            return Err(format!(
                "Pack '{}' lacks the '{}' data_licenses entry (spdx '{}')",
                pack_id, expected_entry.source_id, expected_entry.spdx
            ));
        }
        [single] => {
            return Err(format!(
                "Pack '{}' data_licenses entry {:?} contradicts the model licensing {:?}",
                pack_id, single, expected_entry
            ));
        }
        many => {
            return Err(format!(
                "Pack '{}' carries {} language-model data_licenses entries; exactly one ({:?}) is allowed",
                pack_id,
                many.len(),
                expected_entry
            ));
        }
    }
    if pack_def.uses_frequencies() {
        if manifest.frequency_entry_count == 0 {
            return Err(format!(
                "Pack '{}' model_profile '{}' produced no frequency entries",
                pack_id, manifest.model_profile
            ));
        }
    } else if manifest.frequency_entry_count != 0 {
        return Err(format!(
            "Pack '{}' model_profile '{}' must carry no frequency entries (found {})",
            pack_id, manifest.model_profile, manifest.frequency_entry_count
        ));
    }
    if pack_def.uses_ngrams() {
        if manifest.bigram_count == 0 || manifest.trigram_count == 0 {
            return Err(format!(
                "Pack '{}' model_profile '{}' produced no n-gram predictions (bi={}, tri={})",
                pack_id, manifest.model_profile, manifest.bigram_count, manifest.trigram_count
            ));
        }
    } else if manifest.bigram_count != 0 || manifest.trigram_count != 0 {
        return Err(format!(
            "Pack '{}' model_profile '{}' must carry no n-grams (bi={}, tri={})",
            pack_id, manifest.model_profile, manifest.bigram_count, manifest.trigram_count
        ));
    }
    Ok(())
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
    /// Committed language model consumed by this pack (absent for `model_profile = "none"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_model_manifest_sha256: Option<String>,
    /// Corpus provenance and licensing snapshot of the consumed language model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_model_provenance: Option<LanguageModelProvenance>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_provenance: Vec<SourceReviewProvenance>,
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
    // One strict policy interpretation, shared with `build-pack`.
    let policy = PackPolicyConfig::load_from_file(root.join("data/pack-policy.toml"))?;

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
        validate_pack_model_provenance(root, &manifest, &policy)?;

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

        // Verify source_provenance array invariants if non-empty
        if !manifest.source_provenance.is_empty() {
            let mut prev_src: Option<&str> = None;
            let mut seen_sources = BTreeSet::new();

            for src_prov in &manifest.source_provenance {
                if let Some(prev) = prev_src {
                    if src_prov.source_id.as_str() <= prev {
                        return Err(format!(
                            "Pack '{}' source_provenance not strictly sorted by source_id: '{}' <= '{}'",
                            pack_id, src_prov.source_id, prev
                        ));
                    }
                }
                if !seen_sources.insert(&src_prov.source_id) {
                    return Err(format!(
                        "Pack '{}' duplicate source_id in source_provenance: '{}'",
                        pack_id, src_prov.source_id
                    ));
                }
                prev_src = Some(&src_prov.source_id);

                if src_prov.source_id == "kurdish-hunspell-kmr" {
                    if src_prov.decisions_sha256 != manifest.review_decisions_sha256 {
                        return Err(format!(
                            "Pack '{}' source_provenance decisions_sha256 mismatch for kurdish-hunspell-kmr",
                            pack_id
                        ));
                    }
                    if src_prov.review_queue_manifest_sha256
                        != manifest.review_queue_manifest_sha256
                    {
                        return Err(format!(
                            "Pack '{}' source_provenance review_queue_manifest_sha256 mismatch for kurdish-hunspell-kmr",
                            pack_id
                        ));
                    }
                    if src_prov.controlled_review_report_manifest_sha256
                        != manifest.controlled_review_report_manifest_sha256
                    {
                        return Err(format!(
                            "Pack '{}' source_provenance controlled_review_report_manifest_sha256 mismatch for kurdish-hunspell-kmr",
                            pack_id
                        ));
                    }
                } else if src_prov.source_id == "kuwiki-batch-001" {
                    if src_prov
                        .decisions_sha256
                        .as_deref()
                        .unwrap_or_default()
                        .is_empty()
                    {
                        return Err(format!(
                            "Pack '{}' kuwiki-batch-001 decisions_sha256 empty",
                            pack_id
                        ));
                    }
                    if src_prov
                        .candidates_artifact_sha256
                        .as_deref()
                        .unwrap_or_default()
                        .is_empty()
                    {
                        return Err(format!(
                            "Pack '{}' kuwiki-batch-001 candidates_artifact_sha256 empty",
                            pack_id
                        ));
                    }
                    if src_prov
                        .batch_manifest_sha256
                        .as_deref()
                        .unwrap_or_default()
                        .is_empty()
                    {
                        return Err(format!(
                            "Pack '{}' kuwiki-batch-001 batch_manifest_sha256 empty",
                            pack_id
                        ));
                    }
                    if src_prov
                        .decision_provenance_manifest_sha256
                        .as_deref()
                        .unwrap_or_default()
                        .is_empty()
                    {
                        return Err(format!(
                            "Pack '{}' kuwiki-batch-001 decision_provenance_manifest_sha256 empty",
                            pack_id
                        ));
                    }
                }
            }
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
