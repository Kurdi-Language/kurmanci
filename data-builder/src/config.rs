use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub language_tag: String,
    pub data_version: String,
    pub format_version: u32,
    pub strict_mode: bool,
    pub source_file: String,
    pub build_dir: String,
    pub reports_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizationConfig {
    pub form: String,
    pub preserve_diacritics: bool,
    pub accepted_script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub min_word_length: usize,
    pub max_word_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderConfig {
    pub build: BuildConfig,
    pub normalization: NormalizationConfig,
    pub limits: LimitsConfig,
}

impl Default for BuilderConfig {
    fn default() -> Self {
        Self {
            build: BuildConfig {
                language_tag: "ku-Latn".to_string(),
                data_version: "2026.1".to_string(),
                format_version: 1,
                strict_mode: true,
                source_file: "data/reviewed/lexicon.jsonl".to_string(),
                build_dir: "data/build".to_string(),
                reports_dir: "data/reports".to_string(),
            },
            normalization: NormalizationConfig {
                form: "NFC".to_string(),
                preserve_diacritics: true,
                accepted_script: "Latin".to_string(),
            },
            limits: LimitsConfig {
                min_word_length: 1,
                max_word_length: 64,
            },
        }
    }
}

impl BuilderConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read config file: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("Failed to parse config TOML: {}", e))
    }
}
