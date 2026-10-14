//! Provenance shared by every evaluator that measures a built pack on a held-out partition.
//!
//! One chain, used by all of them, because each link protects a measurement rather than a
//! build: the pack must be the one its manifest describes, the manifest must name the language
//! model and record its manifest hash, the model manifest must be that model and of the schema
//! the evaluators understand, and the partitioning on disk must be the one the model was built
//! from. That last link is the one that makes a held-out partition actually held out: against a
//! different partitioning, the DEVELOPMENT documents may be documents the model trained on.
//!
//! A second copy of this chain in a second evaluator would be a copy that can rot, so it lives
//! here and `evaluate-prediction-coverage` and the pack-quality benchmark both call it.

use crate::pack::language_model::{validate_model_id, LANGUAGE_MODEL_SCHEMA_VERSION};
use crate::pack::manifest::LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct PackManifestModelFields {
    #[serde(default)]
    schema_version: Option<String>,
    #[serde(default)]
    model_profile: String,
    #[serde(default)]
    binary_sha256: Option<String>,
    #[serde(default)]
    language_model_id: Option<String>,
    #[serde(default)]
    language_model_manifest_sha256: Option<String>,
}

#[derive(Deserialize)]
struct PartitionManifestCounts {
    #[serde(default)]
    development_documents: Option<usize>,
    #[serde(default)]
    evaluation_documents: Option<usize>,
}

#[derive(Deserialize)]
struct ModelManifestFields {
    #[serde(default)]
    schema_version: Option<String>,
    model_id: String,
    corpus_id: String,
    partition_manifest_sha256: String,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A pack proved to belong with its manifest, its language model and the partitioning the model
/// was built from, together with the held-out partition's bytes.
pub struct VerifiedPackOnPartition {
    pub pack_bytes: Vec<u8>,
    pub pack_sha256: String,
    pub pack_manifest_sha256: String,
    pub model_id: String,
    pub model_manifest_sha256: String,
    pub corpus_id: String,
    pub partition: String,
    pub partition_manifest_sha256: String,
    pub partition_declared_documents: usize,
    pub partition_path: PathBuf,
    pub partition_bytes: Vec<u8>,
    pub partition_sha256: String,
}

/// Verifies the whole chain and returns it. `partition` must be `development` or `evaluation`:
/// the train partition built the model and measuring on it would measure memorisation.
pub fn verify_pack_on_held_out_partition(
    root: &Path,
    pack_path: &Path,
    partition: &str,
) -> Result<VerifiedPackOnPartition, String> {
    if partition != "development" && partition != "evaluation" {
        return Err(format!(
            "partition must be 'development' or 'evaluation' (the train partition built the model), got '{partition}'"
        ));
    }

    // 1. The pack and its manifest: which model it embeds.
    let pack_bytes =
        fs::read(pack_path).map_err(|e| format!("Failed to read pack {:?}: {}", pack_path, e))?;
    let pack_sha256 = sha256_hex(&pack_bytes);
    let manifest_path = pack_path
        .parent()
        .map(|p| p.join("manifest.json"))
        .ok_or_else(|| format!("pack path {:?} has no parent directory", pack_path))?;
    let manifest_bytes = fs::read(&manifest_path).map_err(|e| {
        format!(
            "Failed to read the pack manifest {:?} next to the pack: {}",
            manifest_path, e
        )
    })?;
    let pack_manifest: PackManifestModelFields = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("Invalid pack manifest {:?}: {}", manifest_path, e))?;
    // The manifest must be of the schema this evaluator understands before any of its
    // provenance fields are interpreted.
    if pack_manifest.schema_version.as_deref() != Some(LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION) {
        return Err(format!(
            "pack manifest {:?} has schema {:?}, expected {:?}; refusing to interpret its provenance",
            manifest_path, pack_manifest.schema_version, LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION
        ));
    }
    // The pack must be the one its manifest describes before the manifest is trusted for
    // anything else: a modified or swapped lexicon.bin never borrows a manifest's provenance.
    let recorded_pack_sha = pack_manifest.binary_sha256.as_deref().ok_or_else(|| {
        format!(
            "pack manifest {:?} records no binary_sha256; cannot bind the pack to its manifest",
            manifest_path
        )
    })?;
    if recorded_pack_sha != pack_sha256 {
        return Err(format!(
            "pack {:?} sha256 {} differs from the binary_sha256 its manifest records ({}); the pack and its manifest do not belong together",
            pack_path, pack_sha256, recorded_pack_sha
        ));
    }
    let model_id = pack_manifest.language_model_id.ok_or_else(|| {
        format!(
            "pack {:?} embeds no language model (model profile '{}'); nothing to measure",
            pack_path, pack_manifest.model_profile
        )
    })?;
    let recorded_model_sha = pack_manifest
        .language_model_manifest_sha256
        .ok_or_else(|| {
            format!(
                "pack manifest {:?} records no language_model_manifest_sha256",
                manifest_path
            )
        })?;

    // 2. The model manifest: its identity must be the one the pack recorded, and it names the
    //    corpus and the partitioning the TRAIN data came from.
    // The id is validated before any path is derived from it (the repository's own rule).
    validate_model_id(&model_id)?;
    let model_manifest_path = root
        .join("data/language-model")
        .join(&model_id)
        .join("manifest.json");
    let model_manifest_bytes = fs::read(&model_manifest_path).map_err(|e| {
        format!(
            "language model '{}' named by the pack has no manifest at {:?}: {}",
            model_id, model_manifest_path, e
        )
    })?;
    let model_manifest_sha256 = sha256_hex(&model_manifest_bytes);
    if model_manifest_sha256 != recorded_model_sha {
        return Err(format!(
            "language model '{}' manifest sha256 {} differs from the one the pack recorded ({}); the pack and the model do not belong together",
            model_id, model_manifest_sha256, recorded_model_sha
        ));
    }
    let model: ModelManifestFields =
        serde_json::from_slice(&model_manifest_bytes).map_err(|e| {
            format!(
                "Invalid language model manifest {:?}: {}",
                model_manifest_path, e
            )
        })?;
    if model.schema_version.as_deref() != Some(LANGUAGE_MODEL_SCHEMA_VERSION) {
        return Err(format!(
            "language model manifest {:?} has schema {:?}, expected {:?}; refusing to interpret its corpus and partition provenance",
            model_manifest_path, model.schema_version, LANGUAGE_MODEL_SCHEMA_VERSION
        ));
    }
    if model.model_id != model_id {
        return Err(format!(
            "language model manifest {:?} declares model_id '{}', the pack named '{}'",
            model_manifest_path, model.model_id, model_id
        ));
    }

    // 3. The local partitioning must be the one the model was built from.
    let partitions_dir = root.join("data/build/corpus-partitions");
    let partition_manifest_path = partitions_dir.join("manifest.json");
    let partition_manifest_bytes = fs::read(&partition_manifest_path).map_err(|e| {
        format!(
            "partition manifest missing at {:?}: {}",
            partition_manifest_path, e
        )
    })?;
    let local_partition_manifest_sha256 = sha256_hex(&partition_manifest_bytes);
    if local_partition_manifest_sha256 != model.partition_manifest_sha256 {
        return Err(format!(
            "the local partition manifest ({}) is not the partitioning the model '{}' was built from ({}); the held-out partition would not be held out from this model",
            local_partition_manifest_sha256, model_id, model.partition_manifest_sha256
        ));
    }
    let counts: PartitionManifestCounts = serde_json::from_slice(&partition_manifest_bytes)
        .map_err(|e| {
            format!(
                "Invalid partition manifest {:?}: {}",
                partition_manifest_path, e
            )
        })?;
    let expected_records = match partition {
        "development" => counts.development_documents,
        _ => counts.evaluation_documents,
    }
    .ok_or_else(|| {
        format!(
            "partition manifest {:?} declares no {}_documents count",
            partition_manifest_path, partition
        )
    })?;
    let partition_path = partitions_dir.join(format!("{partition}.jsonl"));
    let partition_bytes = fs::read(&partition_path)
        .map_err(|e| format!("Failed to read partition {:?}: {}", partition_path, e))?;
    let partition_sha256 = sha256_hex(&partition_bytes);

    Ok(VerifiedPackOnPartition {
        pack_bytes,
        pack_sha256,
        pack_manifest_sha256: sha256_hex(&manifest_bytes),
        model_id,
        model_manifest_sha256,
        corpus_id: model.corpus_id,
        partition: partition.to_string(),
        partition_manifest_sha256: local_partition_manifest_sha256,
        partition_declared_documents: expected_records,
        partition_path,
        partition_bytes,
        partition_sha256,
    })
}
