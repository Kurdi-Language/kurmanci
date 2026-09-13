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

/// Upstream artifact of an `external` corpus: a large public file (for example a
/// Wikimedia XML dump) that is downloaded and verified locally instead of being tracked
/// in git. The registered `files` of the corpus are derived from it by `extractor`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusSourceArtifact {
    /// Immutable download URL of the upstream artifact.
    pub url: String,
    /// Repository-relative path where the artifact is stored locally (git-ignored).
    pub path: String,
    /// Upstream-published SHA-1 (Wikimedia dumps publish SHA-1 sums), if any.
    #[serde(default)]
    pub sha1: Option<String>,
    /// SHA-256 of the exact artifact bytes.
    pub sha256: String,
    /// Deterministic extractor that derives the registered files from the artifact.
    /// Supported: `wikimedia-xml` (MediaWiki `pages-articles.xml.bz2` → documents.jsonl).
    pub extractor: String,
}

pub const ACQUISITION_TRACKED: &str = "tracked";
pub const ACQUISITION_EXTERNAL: &str = "external";
pub const EXTRACTOR_WIKIMEDIA_XML: &str = "wikimedia-xml";

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
    /// `tracked` (default): registered files are committed to the repository.
    /// `external`: registered files are git-ignored derivatives of `source_artifact`,
    /// re-created locally with `acquire-corpus <corpus_id>`; pipeline steps that
    /// enumerate all corpora skip an external corpus whose files are absent.
    #[serde(default = "default_acquisition")]
    pub acquisition: String,
    #[serde(default)]
    pub source_artifact: Option<CorpusSourceArtifact>,
    #[serde(default)]
    pub files: Vec<CorpusFile>,
}

fn default_document_format() -> String {
    "one-document-per-line".to_string()
}

fn default_acquisition() -> String {
    ACQUISITION_TRACKED.to_string()
}

fn is_hex_of_len(value: &str, len: usize) -> bool {
    value.len() == len && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Path components of a registry path with `\` treated as `/` (no normalisation beyond that;
/// callers validate traversal and absolute paths separately).
fn normalized_components(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .map(|s| s.to_string())
        .collect()
}

/// Requires `path` to be `data/<area>/<corpus_id>/<at least one more component>`, compared
/// component by component, so that external-corpus acquisition can only ever write inside
/// the generated-data directory of the corpus that registers it.
fn require_path_under_corpus_root(
    path: &str,
    area: &str,
    corpus_id: &str,
    what: &str,
) -> Result<(), String> {
    let parts = normalized_components(path);
    let ok = parts.len() >= 4
        && parts[0] == "data"
        && parts[1] == area
        && parts[2] == corpus_id
        && parts[3..]
            .iter()
            .all(|p| !p.is_empty() && p != "." && p != "..");
    if !ok {
        return Err(format!(
            "Corpus '{}': {} '{}' must be inside data/{}/{}/",
            corpus_id, what, path, area, corpus_id
        ));
    }
    Ok(())
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
    /// True when the corpus is an `external` corpus (files derived locally from an artifact).
    pub fn is_external(&self) -> bool {
        self.acquisition == ACQUISITION_EXTERNAL
    }

    /// True when every registered file of this corpus exists under `root_dir` (no hashing).
    pub fn files_present<P: AsRef<Path>>(&self, root_dir: P) -> bool {
        !self.files.is_empty()
            && self
                .files
                .iter()
                .all(|f| root_dir.as_ref().join(&f.path).exists())
    }

    /// True when this corpus must be skipped by whole-registry pipeline steps: it is
    /// external and its derived files have not been acquired on this machine.
    pub fn is_skippable_absent<P: AsRef<Path>>(&self, root_dir: P) -> bool {
        self.is_external() && !self.files_present(root_dir)
    }

    /// Validates format-sensitive schema rules and path safety for this corpus entry.
    pub fn validate_schema(&self) -> Result<(), String> {
        for file in &self.files {
            validate_registry_relative_path(&file.path)?;
            if !is_hex_of_len(&file.sha256, 64) {
                return Err(format!(
                    "Corpus '{}': file '{}' sha256 must be 64 hex digits",
                    self.corpus_id, file.path
                ));
            }
        }

        match self.acquisition.as_str() {
            ACQUISITION_TRACKED => {
                if self.source_artifact.is_some() {
                    return Err(format!(
                        "Corpus '{}': source_artifact is only allowed for acquisition = \"external\"",
                        self.corpus_id
                    ));
                }
            }
            ACQUISITION_EXTERNAL => {
                let artifact = self.source_artifact.as_ref().ok_or_else(|| {
                    format!(
                        "Corpus '{}': acquisition = \"external\" requires a [corpora.source_artifact] table",
                        self.corpus_id
                    )
                })?;
                // `acquire-corpus` writes to these two paths, so they are confined to the
                // corpus-specific generated-data roots (checked by path component, never by
                // string prefix) and must be distinct from each other.
                validate_registry_relative_path(&artifact.path)?;
                require_path_under_corpus_root(
                    &artifact.path,
                    "original",
                    &self.corpus_id,
                    "source_artifact.path",
                )?;
                for file in &self.files {
                    require_path_under_corpus_root(
                        &file.path,
                        "imported",
                        &self.corpus_id,
                        "derived file path",
                    )?;
                    if normalized_components(&file.path) == normalized_components(&artifact.path) {
                        return Err(format!(
                            "Corpus '{}': source_artifact.path and derived file path must differ ('{}')",
                            self.corpus_id, file.path
                        ));
                    }
                }
                if !(artifact.url.starts_with("https://") || artifact.url.starts_with("http://")) {
                    return Err(format!(
                        "Corpus '{}': source_artifact.url must be an http(s) URL",
                        self.corpus_id
                    ));
                }
                if !is_hex_of_len(&artifact.sha256, 64) {
                    return Err(format!(
                        "Corpus '{}': source_artifact.sha256 must be 64 hex digits",
                        self.corpus_id
                    ));
                }
                if let Some(sha1) = &artifact.sha1 {
                    if !is_hex_of_len(sha1, 40) {
                        return Err(format!(
                            "Corpus '{}': source_artifact.sha1 must be 40 hex digits",
                            self.corpus_id
                        ));
                    }
                }
                if artifact.extractor != EXTRACTOR_WIKIMEDIA_XML {
                    return Err(format!(
                        "Corpus '{}': unsupported source_artifact.extractor '{}' (supported: '{}')",
                        self.corpus_id, artifact.extractor, EXTRACTOR_WIKIMEDIA_XML
                    ));
                }
                if self.files.len() != 1 {
                    return Err(format!(
                        "Corpus '{}': extractor '{}' derives exactly one registered file (found {})",
                        self.corpus_id,
                        artifact.extractor,
                        self.files.len()
                    ));
                }
                if self.document_format != "jsonl" {
                    return Err(format!(
                        "Corpus '{}': extractor '{}' emits document_format = \"jsonl\"",
                        self.corpus_id, artifact.extractor
                    ));
                }
            }
            other => {
                return Err(format!(
                    "Corpus '{}': unsupported acquisition '{}' (expected 'tracked' or 'external')",
                    self.corpus_id, other
                ));
            }
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

        // The registry is the trust boundary: corpus ids must be unique so that lookups
        // (and therefore acquisition and import) never depend on entry order.
        let mut seen_ids = std::collections::BTreeSet::new();
        for entry in &registry.corpora {
            if !seen_ids.insert(entry.corpus_id.as_str()) {
                return Err(format!(
                    "Duplicate corpus_id '{}' in corpus registry {:?}",
                    entry.corpus_id,
                    path.as_ref()
                ));
            }
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
