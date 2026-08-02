//! Authoritative language pack policy loader and validator (`pack-policy-v1`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const PACK_POLICY_SCHEMA_VERSION: &str = "pack-policy-v1";

/// Pack policy definition for an individual pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackDefinition {
    pub description: String,
    pub opt_in: bool,
    pub allow_as_default: bool,
    pub model_profile: String,
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
            if def.model_profile != "none" {
                return Err(format!(
                    "Pack '{}' specifies model_profile '{}' (only 'none' supported in 4A.2)",
                    pack_id, def.model_profile
                ));
            }
        }

        Ok(())
    }
}
