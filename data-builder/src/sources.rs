use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredFile {
    pub path: String,
    pub upstream_path: Option<String>,
    pub url: Option<String>,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRegistryEntry {
    pub source_id: String,
    pub source_name: String,
    pub source_type: Option<String>,
    pub language: Option<String>,
    pub script: Option<String>,
    pub author: String,
    pub copyright: Option<String>,
    pub license: String,
    pub license_url: String,
    pub url: String,
    pub version: String,
    pub redistribution: String,
    pub attribution_required: Option<bool>,
    pub share_alike_required: Option<bool>,
    pub disclosed_upstream_sources: Option<Vec<String>>,
    pub retrieval_date: Option<String>,
    pub notes: String,
    #[serde(default)]
    pub files: Vec<RegisteredFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRegistry {
    pub sources: Vec<SourceRegistryEntry>,
}

impl SourceRegistry {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read source registry file: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("Failed to parse source registry TOML: {}", e))
    }

    /// Generic verifier checking schema, commit SHA pinning, URL safety, and local SHA-256 checksums
    pub fn verify_preserved_files<P: AsRef<Path>>(&self, root_dir: P) -> Result<(), String> {
        let root = root_dir.as_ref();
        for source in &self.sources {
            // Verify full 40-character commit SHA for git commit-pinned sources
            if source.url.contains("github.com") && source.source_id != "manual-seed" {
                if source.version.len() != 40
                    || !source.version.chars().all(|c| c.is_ascii_hexdigit())
                {
                    return Err(format!(
                        "Source '{}' version '{}' is not a full 40-character commit SHA",
                        source.source_id, source.version
                    ));
                }

                // Verify license_url is commit-pinned and does NOT use moving branches (/main/, /master/, /latest/)
                let l_url = &source.license_url;
                if l_url.contains("/main/")
                    || l_url.contains("/master/")
                    || l_url.contains("/latest/")
                {
                    return Err(format!(
                        "Source '{}' license_url is not commit-pinned: contains moving branch reference in '{}'",
                        source.source_id, l_url
                    ));
                }
            }

            for file_entry in &source.files {
                let file_path = root.join(&file_entry.path);
                if !file_path.exists() {
                    return Err(format!(
                        "Registered preserved file missing for source '{}': {:?}",
                        source.source_id, file_path
                    ));
                }

                // Validate every file URL if present
                if let Some(ref file_url) = file_entry.url {
                    if file_url.contains("/main/")
                        || file_url.contains("/master/")
                        || file_url.contains("/latest/")
                    {
                        return Err(format!(
                            "File URL for '{}' is not commit-pinned: contains moving branch reference in '{}'",
                            file_entry.path, file_url
                        ));
                    }

                    if source.url.contains("github.com")
                        && source.source_id != "manual-seed"
                        && !file_url.contains(&source.version)
                    {
                        return Err(format!(
                            "File URL '{}' does not contain exact declared commit SHA '{}'",
                            file_url, source.version
                        ));
                    }
                }

                let bytes = fs::read(&file_path)
                    .map_err(|e| format!("Failed to read file {:?}: {}", file_path, e))?;

                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                let calculated_hash = format!("{:x}", hasher.finalize());

                if calculated_hash != file_entry.sha256 {
                    return Err(format!(
                        "SHA-256 mismatch for file {:?}: expected {}, calculated {}",
                        file_path, file_entry.sha256, calculated_hash
                    ));
                }
            }
        }
        Ok(())
    }

    /// Deterministic acquisition of source files from commit-pinned URLs or custom base URL
    pub fn acquire_source<P: AsRef<Path>>(
        &self,
        source_id: &str,
        root_dir: P,
        url_override: Option<&str>,
    ) -> Result<(), String> {
        let root = root_dir.as_ref();
        let source = self
            .sources
            .iter()
            .find(|s| s.source_id == source_id)
            .ok_or_else(|| format!("Source ID '{}' not found in registry", source_id))?;

        // 1. Enforce 40-character commit SHA
        if source.version.len() != 40 || !source.version.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "Source '{}' version '{}' is not a full 40-character commit SHA",
                source.source_id, source.version
            ));
        }

        // 2. Reject moving branch references
        let v = &source.version;
        if v.eq_ignore_ascii_case("main")
            || v.eq_ignore_ascii_case("master")
            || v.eq_ignore_ascii_case("latest")
        {
            return Err(format!(
                "Source '{}' revision cannot be a moving branch reference like '{}'",
                source.source_id, v
            ));
        }

        // 3. Create temporary acquisition directory
        let temp_dir = root.join(format!(".tmp_acquire_{}_{}", source_id, std::process::id()));
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temp acquisition dir: {}", e))?;

        // Guard cleanup struct to remove temp dir on failure
        struct TempCleanup<'a>(&'a PathBuf);
        impl Drop for TempCleanup<'_> {
            fn drop(&mut self) {
                if self.0.exists() {
                    let _ = fs::remove_dir_all(self.0);
                }
            }
        }
        let cleanup = TempCleanup(&temp_dir);

        let mut staged_downloads: Vec<(PathBuf, PathBuf, String)> = Vec::new();

        for file_entry in &source.files {
            let final_target_path = root.join(&file_entry.path);

            // Idempotency & Overwrite Safety Check:
            // If file exists locally and matches expected SHA-256, skip re-downloading.
            // If file exists locally but HAS DIFFERENT BYTES, refuse silent overwrite!
            if final_target_path.exists() {
                let existing_bytes = fs::read(&final_target_path).map_err(|e| {
                    format!(
                        "Failed to read existing file {:?}: {}",
                        final_target_path, e
                    )
                })?;
                let mut hasher = Sha256::new();
                hasher.update(&existing_bytes);
                let existing_hash = format!("{:x}", hasher.finalize());

                if existing_hash == file_entry.sha256 {
                    // Already acquired and 100% matching
                    continue;
                } else {
                    return Err(format!(
                        "Existing file {:?} has SHA-256 {} which does not match registered checksum {}; refusing to overwrite!",
                        final_target_path, existing_hash, file_entry.sha256
                    ));
                }
            }

            // Determine download URL
            let download_url = if let Some(override_url) = url_override {
                format!(
                    "{}/{}",
                    override_url.trim_end_matches('/'),
                    file_entry
                        .upstream_path
                        .as_deref()
                        .unwrap_or(&file_entry.path)
                )
            } else if let Some(ref file_url) = file_entry.url {
                file_url.clone()
            } else {
                return Err(format!(
                    "No download URL recorded for file entry '{}'",
                    file_entry.path
                ));
            };

            // Reject moving URLs
            if download_url.contains("/main/")
                || download_url.contains("/master/")
                || download_url.contains("/latest/")
            {
                return Err(format!(
                    "Download URL '{}' is not commit-pinned",
                    download_url
                ));
            }

            // Download using ureq HTTP client
            let response = ureq::get(&download_url)
                .call()
                .map_err(|e| format!("HTTP fetch failed for URL '{}': {}", download_url, e))?;

            if response.status() != 200 {
                return Err(format!(
                    "HTTP fetch failed for URL '{}': status code {}",
                    download_url,
                    response.status()
                ));
            }

            let mut downloaded_bytes = Vec::new();
            response
                .into_reader()
                .read_to_end(&mut downloaded_bytes)
                .map_err(|e| format!("Failed to read HTTP body from '{}': {}", download_url, e))?;

            // Hash exact downloaded bytes
            let mut hasher = Sha256::new();
            hasher.update(&downloaded_bytes);
            let downloaded_hash = format!("{:x}", hasher.finalize());

            if downloaded_hash != file_entry.sha256 {
                return Err(format!(
                    "Checksum mismatch for URL '{}': expected SHA-256 {}, downloaded {}",
                    download_url, file_entry.sha256, downloaded_hash
                ));
            }

            // Stage into temp dir
            let rel_path = Path::new(&file_entry.path);
            let filename = rel_path.file_name().unwrap();
            let temp_file_path = temp_dir.join(filename);

            fs::write(&temp_file_path, &downloaded_bytes).map_err(|e| {
                format!(
                    "Failed to write staged temp file {:?}: {}",
                    temp_file_path, e
                )
            })?;

            staged_downloads.push((temp_file_path, final_target_path, file_entry.sha256.clone()));
        }

        // All files are downloaded and validated before installation begins.
        // Each file is installed using a same-filesystem rename.
        for (temp_path, final_target, _expected_hash) in staged_downloads {
            if let Some(parent) = final_target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create target dir {:?}: {}", parent, e))?;
            }

            fs::rename(&temp_path, &final_target).map_err(|e| {
                format!(
                    "Failed to install staged file from {:?} to {:?}: {}",
                    temp_path, final_target, e
                )
            })?;
        }

        drop(cleanup);
        Ok(())
    }
}
