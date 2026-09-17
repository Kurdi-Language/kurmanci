use crate::distance::weighted_damerau_levenshtein;
use crate::errors::PackLoadError;
use crate::index::{QueryIndex, MAX_STRIPPED_UNIT_DISTANCE};
use crate::lexicon::{LexiconStore, LexiconStoreBuilder};
use crate::normalization::{normalize, strip_diacritics};
use crate::ranking::{
    calculate_score, FrequencyMetadata, NextWordPrediction, RankedCandidate, RankingConfig,
    Suggestion, SuggestionKind, UnknownContextPolicy,
};
use crate::trie::Trie;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::convert::TryInto;

pub use crate::format::{
    MAGIC_BYTES, MAX_BIGRAM_PREDICTIONS_PER_CONTEXT, MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT,
    PACK_VERSION, PROBABILITY_SCALE,
};

/// Smallest possible encoded sizes of the variable-length records of pack schema 4 (every
/// string empty, no regions, no sources, one prediction per context). Counts declared in a
/// pack header are untrusted; pre-allocation is bounded by what the payload can physically
/// contain, so a corrupt count can never trigger a huge allocation.
const MIN_ENCODED_ENTRY_BYTES: usize = 2 + 2 + 2 + 2 + 8 + 2 + 2 + 2 + 8 + 8 + 4;
const MIN_ENCODED_BIGRAM_CONTEXT_BYTES: usize = 4 + 2 + 16;
const MIN_ENCODED_TRIGRAM_CONTEXT_BYTES: usize = 4 + 4 + 2 + 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconEntry {
    pub word: String,
    pub normalized: String,
    pub lemma: String,
    pub part_of_speech: String,
    pub frequency: u64,
    pub regions: Vec<String>,
    pub status: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub frequency_metadata: FrequencyMetadata,
}

pub type TrigramContextKey = (usize, usize);
pub type TrigramPredictionEntry = (usize, u64, u32);
pub type TrigramIndex = HashMap<TrigramContextKey, Vec<TrigramPredictionEntry>>;

#[derive(Debug, Clone, Default)]
pub struct Engine {
    /// The lexicon in compact storage (see `crate::lexicon`); entry order is pack order.
    pub(crate) lexicon: LexiconStore,
    pub(crate) trie: Trie,
    max_frequency: u64,
    #[allow(dead_code)]
    pub(crate) typo_map: HashMap<String, String>,
    pub(crate) bigram_index: HashMap<usize, Vec<(usize, u64, u32)>>,
    pub(crate) trigram_index: TrigramIndex,
    /// Candidate index for suggest/correct/complete (see `crate::index`); rebuilt on load.
    pub(crate) index: QueryIndex,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of entries currently loaded in the lexicon.
    pub fn len(&self) -> usize {
        self.lexicon.len()
    }

    /// Checks if the lexicon is empty.
    pub fn is_empty(&self) -> bool {
        self.lexicon.is_empty()
    }

    /// Loads lexicon entries from JSON structures.
    ///
    /// Entries are appended after any already loaded, in the given order. The compact store
    /// is rebuilt around the existing entries by borrowing their fields (no per-entry
    /// re-materialization). All fallible work happens on staged values: the store, the
    /// trie and the maximum frequency are rebuilt locally and the query index is built from
    /// the completed store; only then are they assigned to the engine together. A lexicon
    /// that exceeds the store's `u32` representation (text, list or table sizes) cannot be
    /// represented: this method panics naming the limit and leaves the engine exactly as it
    /// was (binary packs report the same condition as `PackLoadError::InvalidPayload`).
    pub fn load_lexicon(&mut self, entries: Vec<LexiconEntry>) {
        const LIMIT: &str = "load_lexicon: lexicon exceeds the compact store's u32 representation";
        let mut builder = LexiconStoreBuilder::with_capacity(self.lexicon.len() + entries.len());
        let mut staged_trie = Trie::default();
        let mut staged_max_frequency = 0u64;
        for i in 0..self.lexicon.len() {
            let frequency = self.lexicon.frequency(i);
            staged_max_frequency = staged_max_frequency.max(frequency);
            staged_trie.insert(self.lexicon.normalized(i), frequency);
            builder
                .push(
                    self.lexicon.word(i),
                    self.lexicon.normalized(i),
                    self.lexicon.lemma(i),
                    self.lexicon.part_of_speech(i),
                    frequency,
                    self.lexicon.status(i),
                    self.lexicon.regions(i),
                    self.lexicon.sources(i),
                    self.lexicon.frequency_metadata(i),
                )
                .unwrap_or_else(|e| panic!("{}: {}", LIMIT, e));
        }
        for entry in &entries {
            staged_max_frequency = staged_max_frequency.max(entry.frequency);
            staged_trie.insert(&entry.normalized, entry.frequency);
            builder
                .push(
                    &entry.word,
                    &entry.normalized,
                    &entry.lemma,
                    &entry.part_of_speech,
                    entry.frequency,
                    &entry.status,
                    entry.regions.iter().map(String::as_str),
                    entry.sources.iter().map(String::as_str),
                    entry.frequency_metadata,
                )
                .unwrap_or_else(|e| panic!("{}: {}", LIMIT, e));
        }
        let staged_lexicon = builder.finish();
        staged_trie.build();
        let staged_index = QueryIndex::build(&staged_lexicon);
        // Nothing above touched `self`; commit the staged state together.
        self.lexicon = staged_lexicon;
        self.trie = staged_trie;
        self.max_frequency = self.max_frequency.max(staged_max_frequency);
        self.index = staged_index;
    }

    /// Loads custom typo mappings into the engine.
    pub fn load_typos(&mut self, typos: HashMap<String, String>) {
        self.typo_map = typos;
    }

    /// Loads a compiled binary pack (.bin format v4) with strict header and SHA-256 checksum integrity verification.
    pub fn load_binary_pack(&mut self, bytes: &[u8]) -> Result<usize, PackLoadError> {
        if bytes.len() < 12 {
            return Err(PackLoadError::TooShort(bytes.len()));
        }

        // 1. Magic Bytes Check
        if &bytes[0..4] != MAGIC_BYTES {
            return Err(PackLoadError::InvalidMagicBytes);
        }

        // 2. Format Version Check
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != PACK_VERSION {
            return Err(PackLoadError::UnsupportedVersion { found: version });
        }

        let mut cursor = 8;

        // 3. Language Tag Check
        let (lang_tag, new_cursor) = read_string(bytes, cursor)?;
        cursor = new_cursor;
        if lang_tag != "ku-Latn" {
            return Err(PackLoadError::IncompatibleLanguage { found: lang_tag });
        }

        // 4. Entry Count & Payload Length
        if cursor + 12 > bytes.len() {
            return Err(PackLoadError::TruncatedPayload);
        }
        let count = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;

        let payload_len = usize::try_from(u64::from_le_bytes(
            bytes[cursor..cursor + 8].try_into().unwrap(),
        ))
        .map_err(|_| PackLoadError::InvalidPayload {
            message: "Payload length overflow".to_string(),
        })?;
        cursor += 8;

        // 5. Payload Checksum Verification
        if cursor + 32 > bytes.len() {
            return Err(PackLoadError::TruncatedPayload);
        }
        let stored_checksum: [u8; 32] = bytes[cursor..cursor + 32].try_into().unwrap();
        cursor += 32;

        let payload_bytes = &bytes[cursor..];
        if payload_bytes.len() != payload_len {
            return Err(PackLoadError::InvalidPayload {
                message: format!(
                    "Payload size mismatch: expected {} bytes, found {}",
                    payload_len,
                    payload_bytes.len()
                ),
            });
        }

        let mut hasher = Sha256::new();
        hasher.update(payload_bytes);
        let computed_checksum: [u8; 32] = hasher.finalize().into();

        if stored_checksum != computed_checksum {
            return Err(PackLoadError::ChecksumMismatch);
        }

        // 6. Decode Payload Entries into Staging Buffers
        let count = count as usize;
        if count > payload_bytes.len() / MIN_ENCODED_ENTRY_BYTES {
            return Err(PackLoadError::InvalidPayload {
                message: format!(
                    "Entry count {} cannot fit in a {}-byte payload",
                    count,
                    payload_bytes.len()
                ),
            });
        }
        let mut staged_builder = LexiconStoreBuilder::with_capacity(count);
        let mut staged_count = 0usize;
        let mut staged_trie = Trie::default();
        let mut staged_max_frequency = 0u64;
        let mut payload_cursor = 0;
        let mut regions: Vec<&str> = Vec::new();
        let mut sources: Vec<&str> = Vec::new();

        for _ in 0..count {
            let (word, new_c) = read_str(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;
            let (lemma, new_c) = read_str(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;
            let (normalized, new_c) = read_str(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;
            let (part_of_speech, new_c) = read_str(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;

            if payload_cursor + 8 > payload_bytes.len() {
                return Err(PackLoadError::TruncatedPayload);
            }
            let frequency = u64::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 8]
                    .try_into()
                    .unwrap(),
            );
            payload_cursor += 8;

            let (status, new_c) = read_str(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;

            if payload_cursor + 2 > payload_bytes.len() {
                return Err(PackLoadError::TruncatedPayload);
            }
            let regions_count = u16::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 2;

            regions.clear();
            for _ in 0..regions_count {
                let (reg, new_c) = read_str(payload_bytes, payload_cursor)?;
                payload_cursor = new_c;
                regions.push(reg);
            }

            if payload_cursor + 2 > payload_bytes.len() {
                return Err(PackLoadError::TruncatedPayload);
            }
            let sources_count = u16::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 2;

            sources.clear();
            for _ in 0..sources_count {
                let (src, new_c) = read_str(payload_bytes, payload_cursor)?;
                payload_cursor = new_c;
                sources.push(src);
            }

            if payload_cursor + 20 > payload_bytes.len() {
                return Err(PackLoadError::TruncatedPayload);
            }

            let token_count = u64::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 8]
                    .try_into()
                    .unwrap(),
            );
            payload_cursor += 8;

            let document_count = u64::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 8]
                    .try_into()
                    .unwrap(),
            );
            payload_cursor += 8;

            let zipf_milli = u32::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 4]
                    .try_into()
                    .unwrap(),
            );
            payload_cursor += 4;

            if frequency > staged_max_frequency {
                staged_max_frequency = frequency;
            }
            staged_trie.insert(normalized, frequency);
            // Representation limit: a payload whose text, list or table sizes do not fit
            // the store's u32 offsets is rejected here, before the staged store is adopted,
            // so the existing engine state is untouched.
            staged_builder
                .push(
                    word,
                    normalized,
                    lemma,
                    part_of_speech,
                    frequency,
                    status,
                    regions.iter().copied(),
                    sources.iter().copied(),
                    FrequencyMetadata {
                        token_count,
                        document_count,
                        zipf_milli,
                    },
                )
                .map_err(|e| PackLoadError::InvalidPayload {
                    message: e.to_string(),
                })?;
            staged_count += 1;
        }
        let staged_lexicon = staged_builder.finish();
        debug_assert_eq!(staged_lexicon.len(), staged_count);

        // 7. Decode Bigram Section (Section 2)
        if payload_cursor + 4 > payload_bytes.len() {
            return Err(PackLoadError::TruncatedPayload);
        }
        let bigram_context_count = u32::from_le_bytes(
            payload_bytes[payload_cursor..payload_cursor + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        payload_cursor += 4;

        if bigram_context_count > staged_lexicon.len() {
            return Err(PackLoadError::InvalidPayload {
                message: format!(
                    "Bigram context count {} exceeds lexicon count {}",
                    bigram_context_count,
                    staged_lexicon.len()
                ),
            });
        }
        if bigram_context_count
            > (payload_bytes.len() - payload_cursor) / MIN_ENCODED_BIGRAM_CONTEXT_BYTES
        {
            return Err(PackLoadError::InvalidPayload {
                message: format!(
                    "Bigram context count {} cannot fit in the remaining {} payload bytes",
                    bigram_context_count,
                    payload_bytes.len() - payload_cursor
                ),
            });
        }

        let mut staged_bigram_index: HashMap<usize, Vec<(usize, u64, u32)>> =
            HashMap::with_capacity(bigram_context_count);

        for _ in 0..bigram_context_count {
            if payload_cursor + 6 > payload_bytes.len() {
                return Err(PackLoadError::TruncatedPayload);
            }
            let ctx_idx = u32::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 4;

            if ctx_idx >= staged_lexicon.len() {
                return Err(PackLoadError::InvalidPayload {
                    message: format!(
                        "Context index {} out of lexicon bounds (lexicon count {})",
                        ctx_idx,
                        staged_lexicon.len()
                    ),
                });
            }

            if staged_bigram_index.contains_key(&ctx_idx) {
                return Err(PackLoadError::InvalidPayload {
                    message: format!("Duplicate context index {} in binary pack", ctx_idx),
                });
            }

            let pred_count = u16::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 2;

            if pred_count == 0 {
                return Err(PackLoadError::InvalidPayload {
                    message: format!("Context index {} has zero predictions", ctx_idx),
                });
            }
            if pred_count > MAX_BIGRAM_PREDICTIONS_PER_CONTEXT {
                return Err(PackLoadError::InvalidPayload {
                    message: format!(
                        "Context index {} has prediction count {} exceeding maximum {}",
                        ctx_idx, pred_count, MAX_BIGRAM_PREDICTIONS_PER_CONTEXT
                    ),
                });
            }

            let mut predictions = Vec::with_capacity(pred_count);
            let mut seen_next_indices = std::collections::HashSet::with_capacity(pred_count);

            for _ in 0..pred_count {
                if payload_cursor + 16 > payload_bytes.len() {
                    return Err(PackLoadError::TruncatedPayload);
                }
                let next_idx = u32::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 4]
                        .try_into()
                        .unwrap(),
                ) as usize;
                payload_cursor += 4;

                if next_idx >= staged_lexicon.len() {
                    return Err(PackLoadError::InvalidPayload {
                        message: format!(
                            "Next lexicon index {} out of lexicon bounds (lexicon count {})",
                            next_idx,
                            staged_lexicon.len()
                        ),
                    });
                }

                if !seen_next_indices.insert(next_idx) {
                    return Err(PackLoadError::InvalidPayload {
                        message: format!(
                            "Duplicate next lexicon index {} in context {}",
                            next_idx, ctx_idx
                        ),
                    });
                }

                let count = u64::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 8]
                        .try_into()
                        .unwrap(),
                );
                payload_cursor += 8;

                if count == 0 {
                    return Err(PackLoadError::InvalidPayload {
                        message: format!(
                            "Zero bigram count for prediction index {} in context {}",
                            next_idx, ctx_idx
                        ),
                    });
                }

                let prob = u32::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 4]
                        .try_into()
                        .unwrap(),
                );
                payload_cursor += 4;

                if prob > PROBABILITY_SCALE {
                    return Err(PackLoadError::InvalidPayload {
                        message: format!(
                            "Probability {} exceeds {} for prediction index {} in context {}",
                            prob, PROBABILITY_SCALE, next_idx, ctx_idx
                        ),
                    });
                }

                predictions.push((next_idx, count, prob));
            }

            staged_bigram_index.insert(ctx_idx, predictions);
        }

        // 8. Decode Trigram Section (Section 3)
        if payload_cursor + 4 > payload_bytes.len() {
            return Err(PackLoadError::TruncatedPayload);
        }
        let raw_trigram_context_count = u32::from_le_bytes(
            payload_bytes[payload_cursor..payload_cursor + 4]
                .try_into()
                .unwrap(),
        );
        payload_cursor += 4;

        let trigram_context_count = usize::try_from(raw_trigram_context_count).map_err(|_| {
            PackLoadError::InvalidPayload {
                message: "Invalid trigram context count representation".to_string(),
            }
        })?;

        let max_contexts = staged_lexicon
            .len()
            .checked_mul(staged_lexicon.len())
            .ok_or_else(|| PackLoadError::InvalidPayload {
                message: "Trigram context bound overflow".to_string(),
            })?;

        if trigram_context_count > max_contexts {
            return Err(PackLoadError::InvalidPayload {
                message: format!(
                    "Trigram context count {} exceeds maximum possible pairs {}",
                    trigram_context_count, max_contexts
                ),
            });
        }
        if trigram_context_count
            > (payload_bytes.len() - payload_cursor) / MIN_ENCODED_TRIGRAM_CONTEXT_BYTES
        {
            return Err(PackLoadError::InvalidPayload {
                message: format!(
                    "Trigram context count {} cannot fit in the remaining {} payload bytes",
                    trigram_context_count,
                    payload_bytes.len() - payload_cursor
                ),
            });
        }

        let mut staged_trigram_index: TrigramIndex =
            TrigramIndex::with_capacity(trigram_context_count);

        for _ in 0..trigram_context_count {
            if payload_cursor + 10 > payload_bytes.len() {
                return Err(PackLoadError::TruncatedPayload);
            }
            let prev2_idx = u32::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 4;

            let prev1_idx = u32::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 4;

            if prev2_idx >= staged_lexicon.len() || prev1_idx >= staged_lexicon.len() {
                return Err(PackLoadError::InvalidPayload {
                    message: format!(
                        "Trigram context indices ({}, {}) out of lexicon bounds (count {})",
                        prev2_idx,
                        prev1_idx,
                        staged_lexicon.len()
                    ),
                });
            }

            if staged_trigram_index.contains_key(&(prev2_idx, prev1_idx)) {
                return Err(PackLoadError::InvalidPayload {
                    message: format!(
                        "Duplicate trigram context indices ({}, {}) in binary pack",
                        prev2_idx, prev1_idx
                    ),
                });
            }

            let pred_count = u16::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 2;

            if pred_count == 0 {
                return Err(PackLoadError::InvalidPayload {
                    message: format!(
                        "Trigram context indices ({}, {}) has zero predictions",
                        prev2_idx, prev1_idx
                    ),
                });
            }
            if pred_count > MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT {
                return Err(PackLoadError::InvalidPayload {
                    message: format!(
                        "Trigram context indices ({}, {}) has prediction count {} exceeding maximum {}",
                        prev2_idx, prev1_idx, pred_count, MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT
                    ),
                });
            }

            let mut predictions = Vec::with_capacity(pred_count);
            let mut seen_next_indices = std::collections::HashSet::with_capacity(pred_count);

            for _ in 0..pred_count {
                if payload_cursor + 16 > payload_bytes.len() {
                    return Err(PackLoadError::TruncatedPayload);
                }
                let next_idx = u32::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 4]
                        .try_into()
                        .unwrap(),
                ) as usize;
                payload_cursor += 4;

                if next_idx >= staged_lexicon.len() {
                    return Err(PackLoadError::InvalidPayload {
                        message: format!(
                            "Trigram next lexicon index {} out of lexicon bounds (count {})",
                            next_idx,
                            staged_lexicon.len()
                        ),
                    });
                }

                if !seen_next_indices.insert(next_idx) {
                    return Err(PackLoadError::InvalidPayload {
                        message: format!(
                            "Duplicate next lexicon index {} in trigram context ({}, {})",
                            next_idx, prev2_idx, prev1_idx
                        ),
                    });
                }

                let count = u64::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 8]
                        .try_into()
                        .unwrap(),
                );
                payload_cursor += 8;

                if count == 0 {
                    return Err(PackLoadError::InvalidPayload {
                        message: format!(
                            "Zero trigram count for prediction index {} in context ({}, {})",
                            next_idx, prev2_idx, prev1_idx
                        ),
                    });
                }

                let prob = u32::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 4]
                        .try_into()
                        .unwrap(),
                );
                payload_cursor += 4;

                if prob > PROBABILITY_SCALE {
                    return Err(PackLoadError::InvalidPayload {
                        message: format!(
                            "Probability {} exceeds {} for trigram prediction index {} in context ({}, {})",
                            prob, PROBABILITY_SCALE, next_idx, prev2_idx, prev1_idx
                        ),
                    });
                }

                predictions.push((next_idx, count, prob));
            }

            staged_trigram_index.insert((prev2_idx, prev1_idx), predictions);
        }

        // Verify complete payload consumption
        if payload_cursor != payload_bytes.len() {
            return Err(PackLoadError::InvalidPayload {
                message: format!(
                    "Trailing payload bytes: {} bytes remain",
                    payload_bytes.len() - payload_cursor
                ),
            });
        }

        // Atomically replace engine state upon 100% successful parsing
        staged_trie.build();
        let staged_index = QueryIndex::build(&staged_lexicon);
        let loaded = staged_lexicon.len();
        self.lexicon = staged_lexicon;
        self.trie = staged_trie;
        self.index = staged_index;
        self.max_frequency = staged_max_frequency;
        self.bigram_index = staged_bigram_index;
        self.trigram_index = staged_trigram_index;

        Ok(loaded)
    }

    pub fn contains(&self, word: &str) -> bool {
        let norm = crate::normalization::normalize(word);
        self.trie.contains(&norm)
    }

    /// Read-only estimate of the heap memory owned by each engine structure (see
    /// `crate::memory`). Never changes engine state or behaviour.
    pub fn memory_attribution(&self) -> crate::memory::MemoryAttribution {
        crate::memory::attribute(self)
    }

    /// Predicts next word given previous word context.
    pub fn predict_next(&self, previous_word: &str, limit: usize) -> Vec<NextWordPrediction> {
        self.predict_next_with_policy(previous_word, limit, UnknownContextPolicy::Empty)
    }

    /// Predicts next word given previous word context and fallback policy.
    pub fn predict_next_with_policy(
        &self,
        previous_word: &str,
        limit: usize,
        _policy: UnknownContextPolicy,
    ) -> Vec<NextWordPrediction> {
        if limit == 0 {
            return Vec::new();
        }
        let norm_prev = crate::normalization::normalize(previous_word);
        if norm_prev.is_empty() {
            return Vec::new();
        }

        let ctx_idx = match self.find_lexicon_index(&norm_prev) {
            Some(idx) => idx,
            None => return Vec::new(),
        };

        let preds = match self.bigram_index.get(&ctx_idx) {
            Some(p) => p,
            None => return Vec::new(),
        };

        preds
            .iter()
            .take(limit)
            .map(|&(next_idx, count, prob)| NextWordPrediction {
                word: self.lexicon.word(next_idx).to_string(),
                count,
                probability_millionths: prob,
            })
            .collect()
    }

    /// Predicts next word given two-word context (previous_2, previous_1) using deterministic hard backoff.
    pub fn predict_next_with_context(
        &self,
        previous_2: &str,
        previous_1: &str,
        limit: usize,
    ) -> crate::ranking::ContextPredictionResult {
        if limit == 0 {
            return crate::ranking::ContextPredictionResult {
                source: None,
                predictions: Vec::new(),
            };
        }

        let norm_prev2 = normalize(previous_2);
        let norm_prev1 = normalize(previous_1);

        let p2_idx = self.find_lexicon_index(&norm_prev2);
        let p1_idx = self.find_lexicon_index(&norm_prev1);

        // Check trigram context existence first
        if let (Some(idx2), Some(idx1)) = (p2_idx, p1_idx) {
            if let Some(preds) = self.trigram_index.get(&(idx2, idx1)) {
                let predictions = preds
                    .iter()
                    .take(limit)
                    .map(|&(next_idx, count, prob)| NextWordPrediction {
                        word: self.lexicon.word(next_idx).to_string(),
                        count,
                        probability_millionths: prob,
                    })
                    .collect();

                return crate::ranking::ContextPredictionResult {
                    source: Some(crate::ranking::PredictionSource::Trigram),
                    predictions,
                };
            }
        }

        // Fall back to bigram context for previous_1
        if let Some(idx1) = p1_idx {
            if let Some(preds) = self.bigram_index.get(&idx1) {
                let predictions = preds
                    .iter()
                    .take(limit)
                    .map(|&(next_idx, count, prob)| NextWordPrediction {
                        word: self.lexicon.word(next_idx).to_string(),
                        count,
                        probability_millionths: prob,
                    })
                    .collect();

                return crate::ranking::ContextPredictionResult {
                    source: Some(crate::ranking::PredictionSource::BigramBackoff),
                    predictions,
                };
            }
        }

        crate::ranking::ContextPredictionResult {
            source: None,
            predictions: Vec::new(),
        }
    }

    /// Index of the first entry with this normalized form: the query index's binary search,
    /// which returns exactly what a linear `position` over the lexicon returns.
    fn find_lexicon_index(&self, normalized: &str) -> Option<usize> {
        if normalized.is_empty() {
            return None;
        }
        self.index.find_normalized(&self.lexicon, normalized)
    }

    /// Generates prefix completion candidates.
    pub fn complete(&self, prefix: &str, limit: usize) -> Vec<Suggestion> {
        self.suggest_with_config(prefix, limit, &RankingConfig::default())
    }

    /// Generates spelling and diacritic corrections for a given word.
    pub fn correct(&self, word: &str, limit: usize) -> Vec<Suggestion> {
        self.suggest(word, limit)
    }

    /// Full suggestion pipeline executing exact match -> prefix completion -> diacritic restoration -> weighted edit distance.
    pub fn suggest(&self, query: &str, limit: usize) -> Vec<Suggestion> {
        self.suggest_with_config(query, limit, &RankingConfig::default())
    }

    /// Full suggestion pipeline executing with custom ranking configuration.
    ///
    /// Production path: the candidate index (`crate::index`) supplies a provable superset of
    /// the entries the full scan could accept, and every stage is the same code the
    /// reference full scan runs (`suggest_reference_full_scan`), so results are identical.
    pub fn suggest_with_config(
        &self,
        query: &str,
        limit: usize,
        config: &RankingConfig,
    ) -> Vec<Suggestion> {
        self.suggest_indexed(query, limit, config)
    }

    /// Reference implementation: exact match and prefix completion from the trie (each hit
    /// resolved by a linear search of the lexicon), then every lexicon entry through the
    /// scoring stage. This is the semantic oracle for the indexed path and is deliberately
    /// kept verbatim; it is not part of the public runtime API.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn suggest_reference_full_scan(
        &self,
        query: &str,
        limit: usize,
        config: &RankingConfig,
    ) -> Vec<Suggestion> {
        let norm_query = normalize(query);
        if norm_query.is_empty() {
            return Vec::new();
        }
        let query_stripped = strip_diacritics(&norm_query);
        let mut candidate_ranks: HashMap<String, RankedCandidate> = HashMap::new();

        // 1. Exact match & Prefix completion from Trie
        self.prefix_stage(
            &norm_query,
            &query_stripped,
            &mut candidate_ranks,
            |norm_word| self.lexicon.position_normalized(norm_word),
        );

        // 2. Scan Lexicon for Diacritic Restoration & Edit Distance
        for idx in 0..self.lexicon.len() {
            score_entry(
                &self.lexicon,
                idx,
                &norm_query,
                &query_stripped,
                config,
                &mut candidate_ranks,
            );
        }

        finish_suggestions(candidate_ranks, query, &norm_query, limit, config)
    }

    /// Indexed path: identical stages, but trie hits are resolved by binary search and the
    /// scoring stage only visits the index's candidate superset, in ascending lexicon order
    /// (the order the reference visits them, which matters only for entries sharing a
    /// normalized form).
    fn suggest_indexed(
        &self,
        query: &str,
        limit: usize,
        config: &RankingConfig,
    ) -> Vec<Suggestion> {
        let norm_query = normalize(query);
        if norm_query.is_empty() {
            return Vec::new();
        }
        let query_stripped = strip_diacritics(&norm_query);
        let mut candidate_ranks: HashMap<String, RankedCandidate> = HashMap::new();

        self.prefix_stage(
            &norm_query,
            &query_stripped,
            &mut candidate_ranks,
            |norm_word| self.index.find_normalized(&self.lexicon, norm_word),
        );

        for idx in self
            .index
            .candidates(&query_stripped, MAX_STRIPPED_UNIT_DISTANCE)
        {
            score_entry(
                &self.lexicon,
                idx as usize,
                &norm_query,
                &query_stripped,
                config,
                &mut candidate_ranks,
            );
        }

        finish_suggestions(candidate_ranks, query, &norm_query, limit, config)
    }

    /// Stage 1 of the pipeline: every trie word under the normalized query becomes an Exact
    /// or Completion candidate. `lookup` resolves a normalized trie word to its lexicon entry.
    fn prefix_stage<F>(
        &self,
        norm_query: &str,
        query_stripped: &str,
        candidate_ranks: &mut HashMap<String, RankedCandidate>,
        lookup: F,
    ) where
        F: Fn(&str) -> Option<usize>,
    {
        let prefix_matches = self.trie.find_by_prefix(norm_query);
        for (norm_word, _freq) in prefix_matches {
            let lex_entry = lookup(&norm_word);
            let display_word = lex_entry
                .map(|i| self.lexicon.word(i).to_string())
                .unwrap_or_else(|| norm_word.clone());
            let freq_meta = lex_entry
                .map(|i| self.lexicon.frequency_metadata(i))
                .unwrap_or_default();

            let is_exact = norm_word == norm_query;
            let kind = if is_exact {
                SuggestionKind::Exact
            } else {
                SuggestionKind::Completion
            };

            let prefix_quality = if is_exact {
                100
            } else {
                norm_query.chars().count() as u32
            };

            let is_diac_match = strip_diacritics(&norm_word) == query_stripped;

            let candidate = RankedCandidate {
                word: display_word,
                edit_cost: if is_exact { 0 } else { 1 },
                is_diacritic_match: is_diac_match,
                prefix_quality,
                frequency: freq_meta,
                kind,
            };

            candidate_ranks.insert(norm_word, candidate);
        }
    }

    /// Lexicon indices the scoring stage accepts for `query` (diacritic restoration, or length
    /// within 2 and weighted distance within 2.0), by full scan. Test support for the index
    /// superset invariant.
    #[cfg(test)]
    pub(crate) fn reference_accepted_entries(&self, query: &str) -> Vec<u32> {
        let norm_query = normalize(query);
        if norm_query.is_empty() {
            return Vec::new();
        }
        let query_stripped = strip_diacritics(&norm_query);
        let mut out = Vec::new();
        for i in 0..self.lexicon.len() {
            let candidate_norm = self.lexicon.normalized(i);
            let candidate_stripped = strip_diacritics(candidate_norm);
            if candidate_stripped == query_stripped && candidate_norm != norm_query {
                out.push(i as u32);
                continue;
            }
            let len_diff = (candidate_norm.chars().count() as isize
                - norm_query.chars().count() as isize)
                .abs();
            if len_diff <= 2 && weighted_damerau_levenshtein(&norm_query, candidate_norm) <= 2.0 {
                out.push(i as u32);
            }
        }
        out
    }

    /// The index's candidate superset for `query`. Test support.
    #[cfg(test)]
    pub(crate) fn indexed_candidate_entries(&self, query: &str) -> Vec<u32> {
        let norm_query = normalize(query);
        if norm_query.is_empty() {
            return Vec::new();
        }
        self.index
            .candidates(&strip_diacritics(&norm_query), MAX_STRIPPED_UNIT_DISTANCE)
    }
}

/// Stage 2 of the pipeline for one lexicon entry: diacritic restoration, then the weighted
/// edit-distance fallback. This is the only copy of the scoring semantics; the reference
/// applies it to every entry, the indexed path to the index's candidate superset.
fn score_entry(
    lexicon: &LexiconStore,
    idx: usize,
    norm_query: &str,
    query_stripped: &str,
    config: &RankingConfig,
    candidate_ranks: &mut HashMap<String, RankedCandidate>,
) {
    let candidate_norm = lexicon.normalized(idx);
    let candidate_stripped = strip_diacritics(candidate_norm);
    let is_diac_match = candidate_stripped == query_stripped;

    // Diacritic restoration match
    if is_diac_match && candidate_norm != norm_query {
        let candidate = RankedCandidate {
            word: lexicon.word(idx).to_string(),
            edit_cost: 0, // diacritic restoration is 0 edit distance penalty
            is_diacritic_match: true,
            prefix_quality: 0,
            frequency: lexicon.frequency_metadata(idx),
            kind: SuggestionKind::DiacriticCorrection,
        };
        candidate_ranks.insert(candidate_norm.to_string(), candidate);
        return;
    }

    // Weighted edit distance fallback
    let len_diff =
        (candidate_norm.chars().count() as isize - norm_query.chars().count() as isize).abs();
    if len_diff <= 2 {
        let dist = weighted_damerau_levenshtein(norm_query, candidate_norm);
        if dist <= 2.0 {
            let is_exact = candidate_norm == norm_query;
            let edit_cost = if is_exact {
                0
            } else {
                (dist * 10.0).round() as u32
            };
            let kind = if is_exact {
                SuggestionKind::Exact
            } else if candidate_norm.starts_with(norm_query) {
                SuggestionKind::Completion
            } else {
                SuggestionKind::Correction
            };
            let candidate = RankedCandidate {
                word: lexicon.word(idx).to_string(),
                edit_cost,
                is_diacritic_match: is_diac_match,
                prefix_quality: if candidate_norm.starts_with(norm_query) {
                    norm_query.chars().count() as u32
                } else {
                    0
                },
                frequency: lexicon.frequency_metadata(idx),
                kind,
            };

            candidate_ranks
                .entry(candidate_norm.to_string())
                .and_modify(|existing| {
                    if candidate.cmp_with_config(existing, config) == std::cmp::Ordering::Less {
                        *existing = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }
    }
}

/// Stage 3 of the pipeline: rank, truncate and materialize. The ordering is
/// `RankedCandidate::cmp_with_config`, which ends in the display word; distinct candidates
/// always have distinct display words (they are keyed by normalized form, and the display
/// word determines the normalized form), so the order is a total order independent of the
/// hash map's iteration order and of how the candidates were found.
fn finish_suggestions(
    candidate_ranks: HashMap<String, RankedCandidate>,
    query: &str,
    norm_query: &str,
    limit: usize,
    config: &RankingConfig,
) -> Vec<Suggestion> {
    let mut ranked_list: Vec<RankedCandidate> = candidate_ranks.into_values().collect();
    ranked_list.sort_by(|a, b| a.cmp_with_config(b, config));
    ranked_list.truncate(limit);

    // Convert RankedCandidate to public Suggestion with query-case-preserving display text
    let query_is_lowercase = is_all_lowercase(query);
    ranked_list
        .into_iter()
        .map(|rc| {
            let display_text = if query_is_lowercase && is_title_case_like(&rc.word) {
                rc.word.to_lowercase()
            } else {
                rc.word.clone()
            };

            let score = calculate_score(
                norm_query,
                &display_text,
                rc.frequency.token_count,
                1000,
                rc.edit_cost as f64 / 10.0,
                rc.kind.clone(),
            );
            Suggestion {
                text: display_text,
                score,
                kind: rc.kind.clone(),
                edit_cost: rc.edit_cost,
                zipf_milli: rc.frequency.zipf_milli,
                document_count: rc.frequency.document_count,
                ranking_reason: rc.ranking_reason(config),
            }
        })
        .collect()
}

fn is_title_case_like(s: &str) -> bool {
    let mut alpha_chars = s.chars().filter(|c| c.is_alphabetic());
    match alpha_chars.next() {
        Some(first) if first.is_uppercase() => {
            let mut subsequent_count = 0;
            for c in alpha_chars {
                if !c.is_lowercase() {
                    return false;
                }
                subsequent_count += 1;
            }
            subsequent_count > 0
        }
        _ => false,
    }
}

fn is_all_lowercase(query: &str) -> bool {
    let mut has_alpha = false;
    for c in query.chars() {
        if c.is_alphabetic() {
            has_alpha = true;
            if !c.is_lowercase() {
                return false;
            }
        }
    }
    has_alpha
}

/// Borrows a length-prefixed UTF-8 string from the payload without allocating.
fn read_str(bytes: &[u8], cursor: usize) -> Result<(&str, usize), PackLoadError> {
    if cursor + 2 > bytes.len() {
        return Err(PackLoadError::TruncatedPayload);
    }
    let len = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap()) as usize;
    let start = cursor + 2;
    let end = start + len;
    if end > bytes.len() {
        return Err(PackLoadError::TruncatedPayload);
    }
    let s = std::str::from_utf8(&bytes[start..end]).map_err(|e| PackLoadError::InvalidPayload {
        message: format!("UTF-8 error: {}", e),
    })?;
    Ok((s, end))
}

#[allow(dead_code)]
fn read_string(bytes: &[u8], cursor: usize) -> Result<(String, usize), PackLoadError> {
    if cursor + 2 > bytes.len() {
        return Err(PackLoadError::TruncatedPayload);
    }
    let len = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap()) as usize;
    let start = cursor + 2;
    let end = start + len;
    if end > bytes.len() {
        return Err(PackLoadError::TruncatedPayload);
    }
    let s = std::str::from_utf8(&bytes[start..end])
        .map_err(|e| PackLoadError::InvalidPayload {
            message: format!("UTF-8 error: {}", e),
        })?
        .to_string();
    Ok((s, end))
}
