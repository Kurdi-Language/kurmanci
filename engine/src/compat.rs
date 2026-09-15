//! Runtime compatibility contract of the engine.
//!
//! Everything a consumer needs to decide, before or after loading, whether a language pack
//! can be used by this engine build, and how a failed load is classified. The rules are
//! deterministic and are the single source of truth for the C ABI, the SDK wrappers and the
//! release compatibility manifest:
//!
//! | Condition | Result |
//! |---|---|
//! | magic bytes are not `KRM1` | corrupt pack (`InvalidMagicBytes`) |
//! | pack schema version not in `SUPPORTED_PACK_SCHEMA_VERSIONS` | unsupported schema (`UnsupportedVersion`), for older and newer packs alike |
//! | language tag is not `SUPPORTED_LANGUAGE_TAG` | wrong language (`IncompatibleLanguage`) |
//! | header or payload shorter than declared | truncated (`TooShort`, `TruncatedPayload`) |
//! | payload checksum mismatch | corrupt (`ChecksumMismatch`) |
//! | any structural rule of the schema violated | corrupt (`InvalidPayload`) |
//!
//! A pack that loads is exactly a pack that passes every rule; there is no partial or
//! best-effort load. Loading never panics on malformed input and never allocates more than
//! the payload can physically describe (see `Engine::load_binary_pack`).
//!
//! Language-model statistics (bigram and trigram sections) have no separate version field in
//! the binary: their encoding and semantics are part of the pack schema. Pack schema 4 carries
//! language-model schema 1. The data revision and the exact model that produced a pack are
//! recorded in the pack's `manifest.json` (`language_model_id`, `language_model_provenance`),
//! which the runtime does not read; release bundles carry both alongside this table.

use crate::errors::PackLoadError;
use crate::format::{MAGIC_BYTES, PACK_VERSION};
use serde::Serialize;

/// Engine crate version (semantic version of the Rust engine).
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Pack schema version this engine writes and reads.
pub const PACK_SCHEMA_VERSION: u32 = PACK_VERSION;

/// Pack schema versions this engine loads. Any other value, older or newer, is rejected
/// with `PackLoadError::UnsupportedVersion`.
pub const SUPPORTED_PACK_SCHEMA_VERSIONS: &[u32] = &[PACK_VERSION];

/// Language-model statistics schema carried by the current pack schema.
pub const LANGUAGE_MODEL_SCHEMA_VERSION: u32 = 1;

/// Language-model schemas this engine understands, by way of the pack schemas that carry them.
pub const SUPPORTED_LANGUAGE_MODEL_SCHEMA_VERSIONS: &[u32] = &[LANGUAGE_MODEL_SCHEMA_VERSION];

/// The only language tag this engine serves. Packs for any other language are rejected with
/// `PackLoadError::IncompatibleLanguage`.
pub const SUPPORTED_LANGUAGE_TAG: &str = "ku-Latn";

/// Language-model schema carried by a given pack schema, if that pack schema is supported.
pub fn language_model_schema_for_pack_schema(pack_schema: u32) -> Option<u32> {
    match pack_schema {
        PACK_VERSION => Some(LANGUAGE_MODEL_SCHEMA_VERSION),
        _ => None,
    }
}

/// True when this engine loads packs of `pack_schema`.
pub fn is_pack_schema_supported(pack_schema: u32) -> bool {
    SUPPORTED_PACK_SCHEMA_VERSIONS.contains(&pack_schema)
}

/// True when this engine serves `language_tag`.
pub fn is_language_tag_supported(language_tag: &str) -> bool {
    language_tag == SUPPORTED_LANGUAGE_TAG
}

/// Machine-readable compatibility table of this engine build (what a release bundle ships).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompatibilityTable {
    pub engine_version: &'static str,
    pub pack_magic: &'static str,
    pub pack_schema_version: u32,
    pub supported_pack_schemas: &'static [u32],
    pub language_model_schema_version: u32,
    pub supported_language_model_schemas: &'static [u32],
    pub language_tag: &'static str,
}

impl CompatibilityTable {
    /// The table for this engine build.
    pub fn current() -> Self {
        Self {
            engine_version: ENGINE_VERSION,
            pack_magic: "KRM1",
            pack_schema_version: PACK_SCHEMA_VERSION,
            supported_pack_schemas: SUPPORTED_PACK_SCHEMA_VERSIONS,
            language_model_schema_version: LANGUAGE_MODEL_SCHEMA_VERSION,
            supported_language_model_schemas: SUPPORTED_LANGUAGE_MODEL_SCHEMA_VERSIONS,
            language_tag: SUPPORTED_LANGUAGE_TAG,
        }
    }
}

/// Coarse classification of a failed load, stable across engine versions. The C ABI and the
/// SDK wrappers map their error codes from this, never from error message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum LoadFailureClass {
    /// The bytes are not a complete pack: shorter than the header or the declared payload.
    Truncated,
    /// The bytes are a pack of a schema version this engine does not load (older or newer).
    UnsupportedSchema,
    /// The pack is for another language.
    WrongLanguage,
    /// Wrong magic, checksum mismatch, or a structural rule of the schema violated.
    Corrupt,
}

impl PackLoadError {
    /// Deterministic classification of this error (see `LoadFailureClass`).
    pub fn failure_class(&self) -> LoadFailureClass {
        match self {
            PackLoadError::TooShort(_) | PackLoadError::TruncatedPayload => {
                LoadFailureClass::Truncated
            }
            PackLoadError::UnsupportedVersion { .. } => LoadFailureClass::UnsupportedSchema,
            PackLoadError::IncompatibleLanguage { .. } => LoadFailureClass::WrongLanguage,
            PackLoadError::InvalidMagicBytes
            | PackLoadError::ChecksumMismatch
            | PackLoadError::InvalidPayload { .. } => LoadFailureClass::Corrupt,
        }
    }
}

/// Header fields of a pack, read without decoding the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackHeader {
    /// Schema version declared by the pack (may be unsupported; see `is_supported`).
    pub pack_schema_version: u32,
    /// Language tag declared by the pack (may be unsupported; see `is_supported`).
    pub language_tag: String,
    pub entry_count: u32,
    pub payload_len: u64,
    /// True when both the schema version and the language tag are supported by this engine.
    pub is_supported: bool,
}

/// Reads the pack header (magic, schema version, language tag, entry count, payload length)
/// without decoding entries, so that a consumer can report *why* a pack is unusable before
/// attempting a load. Fails only when the bytes are not a pack at all (bad magic) or are
/// shorter than the header; an unsupported schema or language is reported through
/// `is_supported`, not as an error.
pub fn probe_pack_header(bytes: &[u8]) -> Result<PackHeader, PackLoadError> {
    if bytes.len() < 12 {
        return Err(PackLoadError::TooShort(bytes.len()));
    }
    if &bytes[0..4] != MAGIC_BYTES {
        return Err(PackLoadError::InvalidMagicBytes);
    }
    let pack_schema_version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let tag_len = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
    let tag_end = 10 + tag_len;
    if tag_end + 12 > bytes.len() {
        return Err(PackLoadError::TruncatedPayload);
    }
    let language_tag = std::str::from_utf8(&bytes[10..tag_end])
        .map_err(|e| PackLoadError::InvalidPayload {
            message: format!("UTF-8 error reading language tag: {}", e),
        })?
        .to_string();
    let entry_count = u32::from_le_bytes(bytes[tag_end..tag_end + 4].try_into().unwrap());
    let payload_len = u64::from_le_bytes(bytes[tag_end + 4..tag_end + 12].try_into().unwrap());
    let is_supported =
        is_pack_schema_supported(pack_schema_version) && is_language_tag_supported(&language_tag);
    Ok(PackHeader {
        pack_schema_version,
        language_tag,
        entry_count,
        payload_len,
        is_supported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_consistent_with_format_constants() {
        let t = CompatibilityTable::current();
        assert_eq!(t.pack_schema_version, PACK_VERSION);
        assert!(t.supported_pack_schemas.contains(&PACK_VERSION));
        assert_eq!(
            language_model_schema_for_pack_schema(PACK_VERSION),
            Some(t.language_model_schema_version)
        );
        assert_eq!(
            language_model_schema_for_pack_schema(PACK_VERSION + 1),
            None
        );
        assert!(is_language_tag_supported("ku-Latn"));
        assert!(!is_language_tag_supported("ku-Arab"));
        assert!(!is_pack_schema_supported(PACK_VERSION + 1));
        assert!(!is_pack_schema_supported(PACK_VERSION - 1));
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"supported_pack_schemas\":[4]"));
        assert!(json.contains("\"language_tag\":\"ku-Latn\""));
    }

    #[test]
    fn every_error_has_a_class() {
        assert_eq!(
            PackLoadError::TooShort(3).failure_class(),
            LoadFailureClass::Truncated
        );
        assert_eq!(
            PackLoadError::TruncatedPayload.failure_class(),
            LoadFailureClass::Truncated
        );
        assert_eq!(
            PackLoadError::UnsupportedVersion { found: 9 }.failure_class(),
            LoadFailureClass::UnsupportedSchema
        );
        assert_eq!(
            PackLoadError::IncompatibleLanguage { found: "en".into() }.failure_class(),
            LoadFailureClass::WrongLanguage
        );
        assert_eq!(
            PackLoadError::InvalidMagicBytes.failure_class(),
            LoadFailureClass::Corrupt
        );
        assert_eq!(
            PackLoadError::ChecksumMismatch.failure_class(),
            LoadFailureClass::Corrupt
        );
        assert_eq!(
            PackLoadError::InvalidPayload {
                message: "x".into()
            }
            .failure_class(),
            LoadFailureClass::Corrupt
        );
    }

    #[test]
    fn probe_reports_unsupported_without_failing() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"KRM1");
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(b"en");
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&99u64.to_le_bytes());
        let h = probe_pack_header(&bytes).unwrap();
        assert_eq!(h.pack_schema_version, 7);
        assert_eq!(h.language_tag, "en");
        assert_eq!(h.entry_count, 5);
        assert_eq!(h.payload_len, 99);
        assert!(!h.is_supported);

        assert!(matches!(
            probe_pack_header(b"KRM1"),
            Err(PackLoadError::TooShort(4))
        ));
        assert!(matches!(
            probe_pack_header(&[0u8; 32]),
            Err(PackLoadError::InvalidMagicBytes)
        ));
        let mut short = bytes.clone();
        short.truncate(14);
        assert!(matches!(
            probe_pack_header(&short),
            Err(PackLoadError::TruncatedPayload)
        ));
    }
}
