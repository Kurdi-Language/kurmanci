//! Corpus Registry implementation for loading and verifying registered text corpora.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Component, Path};

/// Registered file entry inside a corpus definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusFile {
    pub path: String,
    pub sha256: String,
}

/// Metadata entry for a registered text corpus in `corpora.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusRegistryEntry {
    pub corpus_id: String,
    pub corpus_name: String,
    pub language: String,
    pub license: String,
    pub license_url: String,
    pub url: String,
    pub version: String,
    pub description: String,
    pub attribution: String,
    pub notes: String,
    #[serde(default = "default_document_format")]
    pub document_format: String,
    #[serde(default)]
    pub document_id_field: Option<String>,
    #[serde(default)]
    pub text_field: Option<String>,
    #[serde(default)]
    pub files: Vec<CorpusFile>,
}

fn default_document_format() -> String {
    "one-document-per-line".to_string()
}

/// Validates that a registry file path is a safe, relative path within the repository root.
/// Rejects empty paths, absolute paths (Unix/Windows), path prefixes, root components,
/// "." and ".." components, and double slashes.
pub fn validate_registry_relative_path(path_str: &str) -> Result<String, String> {
    let normalized = path_str.replace('\\', "/");

    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.starts_with('\\')
        || normalized.contains("//")
    {
        return Err(format!("Invalid relative registry path: '{path_str}'"));
    }

    // Check for Windows drive prefix (e.g. C:, D:)
    if normalized.len() >= 2 {
        let bytes = normalized.as_bytes();
        if bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Err(format!("Windows drive path is forbidden: '{path_str}'"));
        }
    }

    for part in normalized.split('/') {
        if part == "." || part == ".." || part.is_empty() {
            return Err(format!("Unsafe component in registry path: '{path_str}'"));
        }
    }

    let path = Path::new(&normalized);

    if path.is_absolute() {
        return Err(format!("Absolute registry path is forbidden: '{path_str}'"));
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(format!("Unsafe component in registry path: '{path_str}'"));
            }
        }
    }

    Ok(normalized)
}

impl CorpusRegistryEntry {
    /// Validates format-sensitive schema rules and path safety for this corpus entry.
    pub fn validate_schema(&self) -> Result<(), String> {
        for file in &self.files {
            validate_registry_relative_path(&file.path)?;
        }

        match self.document_format.as_str() {
            "one-document-per-line" => {
                if self.document_id_field.is_some() {
                    return Err(format!(
                        "Corpus '{}': document_id_field must be absent for 'one-document-per-line' format",
                        self.corpus_id
                    ));
                }
                if self.text_field.is_some() {
                    return Err(format!(
                        "Corpus '{}': text_field must be absent for 'one-document-per-line' format",
                        self.corpus_id
                    ));
                }
            }
            "jsonl" => {
                if self
                    .document_id_field
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
                {
                    return Err(format!(
                        "Corpus '{}': document_id_field is required for 'jsonl' format",
                        self.corpus_id
                    ));
                }
                if self.text_field.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(format!(
                        "Corpus '{}': text_field is required for 'jsonl' format",
                        self.corpus_id
                    ));
                }
            }
            other => {
                return Err(format!(
                    "Corpus '{}': unsupported document_format '{}'",
                    self.corpus_id, other
                ));
            }
        }
        Ok(())
    }
}

/// Container struct representing `corpora.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusRegistry {
    #[serde(default)]
    pub corpora: Vec<CorpusRegistryEntry>,
}

impl CorpusRegistry {
    /// Loads, parses, and validates the corpus registry TOML file.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read corpus registry {:?}: {}", path.as_ref(), e))?;
        let registry: CorpusRegistry = toml::from_str(&content).map_err(|e| {
            format!(
                "Failed to parse corpus registry TOML {:?}: {}",
                path.as_ref(),
                e
            )
        })?;

        for entry in &registry.corpora {
            entry.validate_schema()?;
        }

        Ok(registry)
    }

    /// Finds a registered corpus by ID.
    pub fn find_corpus(&self, corpus_id: &str) -> Option<&CorpusRegistryEntry> {
        self.corpora.iter().find(|c| c.corpus_id == corpus_id)
    }

    /// Verifies preserved files and SHA-256 checksums for a registered corpus.
    pub fn verify_corpus_files<P: AsRef<Path>>(
        &self,
        entry: &CorpusRegistryEntry,
        root_dir: P,
    ) -> Result<(), String> {
        let root = root_dir.as_ref();
        if entry.files.is_empty() {
            return Err(format!(
                "Corpus '{}' has no registered files in registry",
                entry.corpus_id
            ));
        }

        for file_entry in &entry.files {
            let file_path = root.join(&file_entry.path);
            if !file_path.exists() {
                return Err(format!(
                    "Registered corpus file missing for '{}': {:?}",
                    entry.corpus_id, file_path
                ));
            }

            let mut f = fs::File::open(&file_path)
                .map_err(|e| format!("Failed to open corpus file {:?}: {}", file_path, e))?;
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 8192];
            loop {
                let n = f
                    .read(&mut buffer)
                    .map_err(|e| format!("Error reading {:?}: {}", file_path, e))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }

            let computed = format!("{:x}", hasher.finalize());
            if computed != file_entry.sha256 {
                return Err(format!(
                    "Checksum mismatch for corpus '{}' file {:?}:\n  expected: {}\n  actual:   {}",
                    entry.corpus_id, file_entry.path, file_entry.sha256, computed
                ));
            }
        }

        Ok(())
    }
}
