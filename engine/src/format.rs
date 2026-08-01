//! Shared Binary Format Constants and Invariants for Kurmancî Engine Packs.
//! Contains ONLY format specification constants with zero runtime engine dependencies.

/// Current binary format version.
pub const PACK_VERSION: u32 = 4;

/// Maximum predictions per previous word context in Section 2 (Bigrams).
pub const MAX_BIGRAM_PREDICTIONS_PER_CONTEXT: usize = 16;

/// Maximum predictions per two-word context in Section 3 (Trigrams).
pub const MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT: usize = 12;

/// Fixed-point probability scale factor (millionths).
pub const PROBABILITY_SCALE: u32 = 1_000_000;

/// Pack magic bytes.
pub const MAGIC_BYTES: &[u8; 4] = b"KRM1";
