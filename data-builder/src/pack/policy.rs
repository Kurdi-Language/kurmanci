//! Authoritative language pack policy loader and validator (`pack-policy-v1`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const PACK_POLICY_SCHEMA_VERSION: &str = "pack-policy-v1";

/// No frequency or n-gram data: lexicon only.
pub const MODEL_PROFILE_NONE: &str = "none";
/// Unigram frequency metadata from the committed language model (frequency-aware ranking).
pub const MODEL_PROFILE_FREQUENCY: &str = "frequency";
/// Unigram frequencies plus bigram and trigram predictions from the committed language model.
pub const MODEL_PROFILE_NGRAM: &str = "ngram";
/// Bigram and trigram predictions only: next-word prediction without any change to
/// suggestion ranking (no unigram frequency metadata is encoded).
pub const MODEL_PROFILE_PREDICTION: &str = "prediction";
pub const MODEL_PROFILES: [&str; 4] = [
    MODEL_PROFILE_NONE,
    MODEL_PROFILE_FREQUENCY,
    MODEL_PROFILE_NGRAM,
    MODEL_PROFILE_PREDICTION,
];

/// Pack policy definition for an individual pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackDefinition {
    pub description: String,
    pub opt_in: bool,
    pub allow_as_default: bool,
    pub model_profile: String,
    /// Committed language model id under `data/language-model/` (required unless
    /// `model_profile = "none"`, forbidden otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_model: Option<String>,
}

impl PackDefinition {
    /// True when the pack consumes a committed language model at all.
    pub fn uses_model(&self) -> bool {
        self.model_profile != MODEL_PROFILE_NONE
    }

    /// True when unigram frequency metadata is encoded (affects suggestion ranking).
    pub fn uses_frequencies(&self) -> bool {
        self.model_profile == MODEL_PROFILE_FREQUENCY || self.model_profile == MODEL_PROFILE_NGRAM
    }

    /// True when bigram/trigram predictions are encoded (next-word prediction).
    pub fn uses_ngrams(&self) -> bool {
        self.model_profile == MODEL_PROFILE_NGRAM || self.model_profile == MODEL_PROFILE_PREDICTION
    }
}

/// Root configuration schema (`data/pack-policy.toml`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackPolicyConfig {
    pub schema_version: String,
    pub default_pack: String,
    pub packs: BTreeMap<String, PackDefinition>,
}

impl PackPolicyConfig {
    /// Loads and validates `pack-policy.toml` from file path.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let p = path.as_ref();
        if !p.exists() {
            return Err(format!("Pack policy file missing at {:?}", p));
        }
        let content = fs::read_to_string(p)
            .map_err(|e| format!("Failed to read pack policy {:?}: {}", p, e))?;
        let config: Self = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse pack policy {:?}: {}", p, e))?;
        config.validate()?;
        Ok(config)
    }

    /// Strict invariant checks for `PackPolicyConfig`.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PACK_POLICY_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported pack policy schema_version '{}' (expected '{}')",
                self.schema_version, PACK_POLICY_SCHEMA_VERSION
            ));
        }

        let keys: Vec<String> = self.packs.keys().cloned().collect();
        let expected = vec![
            "experimental-full".to_string(),
            "reviewed".to_string(),
            "seed".to_string(),
        ];
        if keys != expected {
            return Err(format!(
                "Pack policy must contain exactly the three packs ['seed', 'reviewed', 'experimental-full'] (found {:?})",
                keys
            ));
        }

        let default_def = self.packs.get(&self.default_pack).ok_or_else(|| {
            format!(
                "Default pack '{}' not found in packs table",
                self.default_pack
            )
        })?;

        if default_def.opt_in {
            return Err(format!(
                "Default pack '{}' cannot have opt_in = true",
                self.default_pack
            ));
        }

        if !default_def.allow_as_default {
            return Err(format!(
                "Default pack '{}' must have allow_as_default = true",
                self.default_pack
            ));
        }

        if let Some(exp_def) = self.packs.get("experimental-full") {
            if !exp_def.opt_in {
                return Err("experimental-full pack must have opt_in = true".to_string());
            }
            if exp_def.allow_as_default {
                return Err("experimental-full pack must have allow_as_default = false".to_string());
            }
        }

        for (pack_id, def) in &self.packs {
            if !MODEL_PROFILES.contains(&def.model_profile.as_str()) {
                return Err(format!(
                    "Pack '{}' specifies unsupported model_profile '{}' (expected one of {:?})",
                    pack_id, def.model_profile, MODEL_PROFILES
                ));
            }
            match (&def.language_model, def.model_profile.as_str()) {
                (Some(_), MODEL_PROFILE_NONE) => {
                    return Err(format!(
                        "Pack '{}' sets language_model but model_profile is 'none'",
                        pack_id
                    ));
                }
                (None, profile) if profile != MODEL_PROFILE_NONE => {
                    return Err(format!(
                        "Pack '{}' model_profile '{}' requires a language_model id",
                        pack_id, profile
                    ));
                }
                (Some(id), _) if id.trim().is_empty() => {
                    return Err(format!(
                        "Pack '{}' language_model must not be empty",
                        pack_id
                    ));
                }
                _ => {}
            }
        }

        Ok(())
    }
}
