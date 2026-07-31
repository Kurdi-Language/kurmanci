//! Corpus Registry implementation for loading and verifying registered text corpora.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

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
    #[serde(default)]
    pub files: Vec<CorpusFile>,
}

/// Container struct representing `corpora.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusRegistry {
    #[serde(default)]
    pub corpora: Vec<CorpusRegistryEntry>,
}

impl CorpusRegistry {
    /// Loads and parses the corpus registry TOML file.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read corpus registry {:?}: {}", path.as_ref(), e))?;
        toml::from_str(&content).map_err(|e| {
            format!(
                "Failed to parse corpus registry TOML {:?}: {}",
                path.as_ref(),
                e
            )
        })
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
