//! Acquisition of `external` corpora registered in `corpora.toml`.
//!
//! An external corpus is derived from a large public artifact (for example a Wikimedia
//! XML dump) that is not tracked in git. `acquire_corpus` downloads the artifact to its
//! registered local path (or reuses a present copy), verifies the registered SHA-256
//! (and upstream SHA-1 when given), runs the registered deterministic extractor, and
//! fail-closes unless the derived file matches the SHA-256 pinned in the registry.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use super::extractors::extract_wikimedia_file;
use super::registry::{CorpusRegistry, CorpusRegistryEntry, EXTRACTOR_WIKIMEDIA_XML};

/// Summary of one acquisition run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusAcquisitionReport {
    pub corpus_id: String,
    pub version: String,
    pub artifact_path: String,
    pub artifact_sha256: String,
    pub artifact_downloaded: bool,
    pub derived_file_path: String,
    pub derived_file_sha256: String,
    pub derived_document_count: usize,
}

fn sha256_of_file(path: &Path) -> Result<String, String> {
    let mut f =
        File::open(path).map_err(|e| format!("Failed to open {:?} for hashing: {}", path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("Read error on {:?}: {}", path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn download_to(url: &str, target: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP fetch failed for '{}': {}", url, e))?;
    if response.status() != 200 {
        return Err(format!(
            "HTTP fetch failed for '{}': status code {}",
            url,
            response.status()
        ));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
    }
    let tmp = target.with_extension("part");
    {
        let mut out = File::create(&tmp)
            .map_err(|e| format!("Failed to create temp download file {:?}: {}", tmp, e))?;
        let mut reader = response.into_reader();
        let mut buf = [0u8; 65536];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("Download read error from '{}': {}", url, e))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|e| format!("Write error to {:?}: {}", tmp, e))?;
        }
        out.flush().map_err(|e| format!("Flush error: {}", e))?;
    }
    fs::rename(&tmp, target)
        .map_err(|e| format!("Failed to move {:?} to {:?}: {}", tmp, target, e))?;
    Ok(())
}

/// Acquires the external corpus `corpus_id` under `root_dir`. Idempotent: a present and
/// verified artifact is not downloaded again, and a present and verified derived file is
/// not re-extracted.
pub fn acquire_corpus<P: AsRef<Path>>(
    corpus_id: &str,
    root_dir: P,
) -> Result<CorpusAcquisitionReport, String> {
    let root = root_dir.as_ref();
    let registry_path = root.join("data/source-registry/corpora.toml");
    let registry = CorpusRegistry::load_from_file(&registry_path)?;
    let entry: &CorpusRegistryEntry = registry
        .find_corpus(corpus_id)
        .ok_or_else(|| format!("Corpus '{}' is not registered in corpora.toml", corpus_id))?;
    if !entry.is_external() {
        return Err(format!(
            "Corpus '{}' is a tracked corpus; only external corpora are acquired",
            corpus_id
        ));
    }
    let artifact = entry
        .source_artifact
        .as_ref()
        .ok_or_else(|| format!("Corpus '{}' has no source_artifact", corpus_id))?;
    let derived = entry
        .files
        .first()
        .ok_or_else(|| format!("Corpus '{}' registers no derived file", corpus_id))?;

    // 1. Artifact: reuse when present and verified, otherwise download and verify.
    let artifact_path = root.join(&artifact.path);
    let mut artifact_downloaded = false;
    let present_ok = artifact_path.exists() && sha256_of_file(&artifact_path)? == artifact.sha256;
    if !present_ok {
        println!(
            "  Downloading artifact {} -> {}",
            artifact.url, artifact.path
        );
        download_to(&artifact.url, &artifact_path)?;
        artifact_downloaded = true;
        let actual = sha256_of_file(&artifact_path)?;
        if actual != artifact.sha256 {
            let _ = fs::remove_file(&artifact_path);
            return Err(format!(
                "Artifact SHA-256 mismatch for '{}': registry {}, downloaded {}",
                artifact.url, artifact.sha256, actual
            ));
        }
    } else {
        println!("  Artifact already present and verified: {}", artifact.path);
    }

    // 2. Derived file: reuse when present and verified, otherwise extract deterministically.
    let derived_path = root.join(&derived.path);
    let derived_ok = derived_path.exists() && sha256_of_file(&derived_path)? == derived.sha256;
    if !derived_ok {
        if let Some(parent) = derived_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
        }
        match artifact.extractor.as_str() {
            EXTRACTOR_WIKIMEDIA_XML => {
                println!("  Extracting {} -> {}", artifact.path, derived.path);
                let report = extract_wikimedia_file(
                    &artifact_path,
                    &derived_path,
                    artifact.sha1.as_deref(),
                )?;
                if report.output_checksum_sha256 != derived.sha256 {
                    let _ = fs::remove_file(&derived_path);
                    return Err(format!(
                        "Derived file SHA-256 mismatch for '{}': registry {}, extracted {}. The extractor or the artifact changed; do not register the new hash without review.",
                        derived.path, derived.sha256, report.output_checksum_sha256
                    ));
                }
            }
            other => return Err(format!("Unsupported extractor '{}'", other)),
        }
    } else {
        println!(
            "  Derived file already present and verified: {}",
            derived.path
        );
    }

    // 3. Final registry-level verification and document count.
    registry.verify_corpus_files(entry, root)?;
    let derived_sha = sha256_of_file(&derived_path)?;
    let derived_document_count = {
        let f = File::open(&derived_path)
            .map_err(|e| format!("Failed to open {:?}: {}", derived_path, e))?;
        use std::io::BufRead;
        std::io::BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .count()
    };

    Ok(CorpusAcquisitionReport {
        corpus_id: entry.corpus_id.clone(),
        version: entry.version.clone(),
        artifact_path: artifact.path.clone(),
        artifact_sha256: artifact.sha256.clone(),
        artifact_downloaded,
        derived_file_path: derived.path.clone(),
        derived_file_sha256: derived_sha,
        derived_document_count,
    })
}
