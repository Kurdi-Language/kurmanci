use crate::validate::SourceLexiconEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub const MAGIC_BYTES: &[u8; 4] = b"KRM1";
pub const PACK_VERSION: u32 = 2;
pub const LANGUAGE_TAG: &str = "ku-Latn";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub project: String,
    pub language: String,
    pub data_version: String,
    pub format_version: u32,
    pub entry_count: usize,
    pub checksum_sha256: String,
    pub build_configuration_hash: String,
    pub reproducible: bool,
}

pub fn compile_binary_pack(entries: &[SourceLexiconEntry]) -> Result<Vec<u8>, String> {
    // 1. Build Payload
    let mut payload = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        write_string(&mut payload, &entry.word)
            .map_err(|e| format!("Entry {}: 'word' field error: {}", i + 1, e))?;
        write_string(&mut payload, &entry.lemma)
            .map_err(|e| format!("Entry {}: 'lemma' field error: {}", i + 1, e))?;
        write_string(&mut payload, &entry.normalized)
            .map_err(|e| format!("Entry {}: 'normalized' field error: {}", i + 1, e))?;
        write_string(&mut payload, &entry.part_of_speech)
            .map_err(|e| format!("Entry {}: 'part_of_speech' field error: {}", i + 1, e))?;
        payload.extend_from_slice(&entry.frequency.to_le_bytes());
        write_string(&mut payload, &entry.status)
            .map_err(|e| format!("Entry {}: 'status' field error: {}", i + 1, e))?;

        // Encode regions deterministically
        let mut sorted_regions = entry.regions.clone();
        sorted_regions.sort();
        sorted_regions.dedup();
        if sorted_regions.len() > u16::MAX as usize {
            return Err(format!(
                "Entry {}: Too many regions ({})",
                i + 1,
                sorted_regions.len()
            ));
        }
        payload.extend_from_slice(&(sorted_regions.len() as u16).to_le_bytes());
        for reg in &sorted_regions {
            write_string(&mut payload, reg)
                .map_err(|e| format!("Entry {}: 'region' field error: {}", i + 1, e))?;
        }

        // Encode sources deterministically
        let mut sorted_sources = entry.sources.clone();
        sorted_sources.sort();
        sorted_sources.dedup();
        if sorted_sources.len() > u16::MAX as usize {
            return Err(format!(
                "Entry {}: Too many sources ({})",
                i + 1,
                sorted_sources.len()
            ));
        }
        payload.extend_from_slice(&(sorted_sources.len() as u16).to_le_bytes());
        for src in &sorted_sources {
            write_string(&mut payload, src)
                .map_err(|e| format!("Entry {}: 'source' field error: {}", i + 1, e))?;
        }

        // Encode FrequencyMetadata (Version 2 extension)
        let freq_meta = entry
            .frequency_metadata
            .as_ref()
            .cloned()
            .unwrap_or_default();
        payload.extend_from_slice(&freq_meta.token_count.to_le_bytes());
        payload.extend_from_slice(&freq_meta.document_count.to_le_bytes());
        payload.extend_from_slice(&freq_meta.zipf_milli.to_le_bytes());
    }

    // Calculate 32-byte raw SHA-256 of payload
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let payload_checksum: [u8; 32] = hasher.finalize().into();

    // 2. Build Header
    let mut header = Vec::new();
    header.extend_from_slice(MAGIC_BYTES);
    header.extend_from_slice(&PACK_VERSION.to_le_bytes());
    write_string(&mut header, LANGUAGE_TAG)?;
    let entry_count = check_entry_count(entries.len())?;
    let payload_len = check_payload_len(payload.len())?;
    header.extend_from_slice(&entry_count.to_le_bytes());
    header.extend_from_slice(&payload_len.to_le_bytes());
    header.extend_from_slice(&payload_checksum);

    // Combine Header + Payload
    header.extend_from_slice(&payload);
    Ok(header)
}

pub fn check_entry_count(len: usize) -> Result<u32, String> {
    u32::try_from(len).map_err(|_| "Entry count exceeds u32::MAX".to_string())
}

pub fn check_payload_len(len: usize) -> Result<u64, String> {
    u64::try_from(len).map_err(|_| "Payload length exceeds u64::MAX".to_string())
}

pub fn calculate_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn write_artifacts<P: AsRef<Path>>(
    build_dir: P,
    binary_bytes: &[u8],
    manifest: &ReleaseManifest,
) -> Result<(), String> {
    let dir = build_dir.as_ref();
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create build dir: {}", e))?;

    let bin_path = dir.join("lexicon.bin");
    fs::write(&bin_path, binary_bytes)
        .map_err(|e| format!("Failed to write lexicon.bin: {}", e))?;

    let manifest_path = dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| format!("Failed to write manifest.json: {}", e))?;

    Ok(())
}

pub fn write_string(buf: &mut Vec<u8>, s: &str) -> Result<(), String> {
    let bytes = s.as_bytes();
    if bytes.len() > u16::MAX as usize {
        return Err(format!(
            "String length ({} bytes) exceeds maximum allowable limit (u16::MAX = 65535 bytes)",
            bytes.len()
        ));
    }
    let len = bytes.len() as u16;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_string_bounds() {
        let mut buf = Vec::new();
        let valid_str = "a".repeat(100);
        assert!(write_string(&mut buf, &valid_str).is_ok());

        let oversized_str = "x".repeat(u16::MAX as usize + 1);
        let res = write_string(&mut buf, &oversized_str);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("exceeds maximum allowable limit"));
    }

    #[test]
    fn test_conversion_boundaries() {
        assert!(check_entry_count(100).is_ok());
        assert_eq!(check_entry_count(100).unwrap(), 100);
        assert!(check_payload_len(5000).is_ok());
        assert_eq!(check_payload_len(5000).unwrap(), 5000);

        #[cfg(target_pointer_width = "64")]
        {
            let overflow_entry_count = u32::MAX as usize + 1;
            let res = check_entry_count(overflow_entry_count);
            assert!(res.is_err());
            assert!(res.unwrap_err().contains("exceeds u32::MAX"));
        }
    }
}
