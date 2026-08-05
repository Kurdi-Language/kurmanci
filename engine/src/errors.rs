use std::io;
use thiserror::Error;

/// Low-level binary pack decoder errors.
#[derive(Debug, Error)]
pub enum PackLoadError {
    #[error("binary pack file too short ({0} bytes)")]
    TooShort(usize),

    #[error("invalid language-pack header magic bytes")]
    InvalidMagicBytes,

    #[error("unsupported language-pack version: {found}")]
    UnsupportedVersion { found: u32 },

    #[error("incompatible language tag: '{found}' (expected 'ku-Latn')")]
    IncompatibleLanguage { found: String },

    #[error("language-pack payload is truncated")]
    TruncatedPayload,

    #[error("language-pack payload checksum mismatch")]
    ChecksumMismatch,

    #[error("invalid language-pack payload: {message}")]
    InvalidPayload { message: String },
}

/// Primary consumer-facing error type for engine initialization and pack loading operations.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("failed to read language pack file: {0}")]
    Io(#[from] io::Error),

    #[error("failed to load language pack: {0}")]
    PackLoad(#[from] PackLoadError),
}
