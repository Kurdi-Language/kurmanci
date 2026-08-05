use crate::engine::{Engine, MAGIC_BYTES, PACK_VERSION};
use crate::errors::{EngineError, PackLoadError};
use crate::normalization::normalize;
use crate::ranking::{NextWordPrediction, PredictionSource, SuggestionKind};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const DEFAULT_RESULT_LIMIT: usize = 5;
pub const MAX_RESULT_LIMIT: usize = 50;

fn clamp_limit(limit: usize) -> usize {
    if limit > MAX_RESULT_LIMIT {
        MAX_RESULT_LIMIT
    } else {
        limit
    }
}

/// Metadata extracted directly from a loaded binary language pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackInfo {
    pub language_tag: String,
    pub format_version: u32,
    pub entry_count: usize,
}

/// Options for spelling correction queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionOptions {
    pub limit: usize,
}

impl Default for CorrectionOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_RESULT_LIMIT,
        }
    }
}

/// Options for prefix completion queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionOptions {
    pub limit: usize,
}

impl Default for CompletionOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_RESULT_LIMIT,
        }
    }
}

/// Options for combined suggestion queries (exact, completion, and correction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestOptions {
    pub limit: usize,
}

impl Default for SuggestOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_RESULT_LIMIT,
        }
    }
}

/// Options for next-word prediction queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredictionOptions {
    pub limit: usize,
}

impl Default for PredictionOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_RESULT_LIMIT,
        }
    }
}

/// Structured spelling or completion suggestion item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestionResult {
    pub text: String,
    pub kind: SuggestionKind,
    pub edit_cost: u32,
}

/// Structured next-word prediction item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prediction {
    pub text: String,
    pub count: u64,
    pub probability_millionths: u32,
    pub source: PredictionSource,
}

/// High-level, thread-safe, immutable Kurmancî language engine owning a loaded pack.
#[derive(Debug, Clone)]
pub struct KurmanciEngine {
    inner: Engine,
    info: PackInfo,
}

impl KurmanciEngine {
    /// Loads an engine instance from raw binary pack bytes.
    pub fn from_pack_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        let (lang_tag, version) = parse_pack_header_metadata(bytes)?;
        let mut inner = Engine::new();
        let loaded_count = inner.load_binary_pack(bytes)?;

        let info = PackInfo {
            language_tag: lang_tag,
            format_version: version,
            entry_count: loaded_count,
        };

        Ok(Self { inner, info })
    }

    /// Loads an engine instance from a binary pack file path.
    pub fn from_pack_file(path: impl AsRef<Path>) -> Result<Self, EngineError> {
        let bytes = fs::read(path)?;
        Self::from_pack_bytes(&bytes)
    }

    /// Returns metadata describing the loaded language pack.
    pub fn pack_info(&self) -> &PackInfo {
        &self.info
    }

    /// Returns the total number of unique lexicon entries loaded.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if no entries are loaded.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns true if the exact or NFC-normalized word exists in the lexicon.
    pub fn is_known_word(&self, word: &str) -> bool {
        let normalized = normalize(word);
        self.inner.contains(&normalized)
    }

    /// Returns ranked spelling corrections for an input word.
    pub fn correct(&self, input: &str, options: CorrectionOptions) -> Vec<SuggestionResult> {
        let effective_limit = clamp_limit(options.limit);
        if effective_limit == 0 {
            return Vec::new();
        }
        self.inner
            .correct(input, effective_limit)
            .into_iter()
            .map(|s| SuggestionResult {
                text: s.text,
                kind: s.kind,
                edit_cost: s.edit_cost,
            })
            .collect()
    }

    /// Returns prefix completions for a given input prefix.
    pub fn complete(&self, prefix: &str, options: CompletionOptions) -> Vec<SuggestionResult> {
        let effective_limit = clamp_limit(options.limit);
        if effective_limit == 0 {
            return Vec::new();
        }
        self.inner
            .complete(prefix, effective_limit)
            .into_iter()
            .map(|s| SuggestionResult {
                text: s.text,
                kind: s.kind,
                edit_cost: s.edit_cost,
            })
            .collect()
    }

    /// Returns ranked combined suggestions (exact matches, completions, and corrections).
    pub fn suggest(&self, input: &str, options: SuggestOptions) -> Vec<SuggestionResult> {
        let effective_limit = clamp_limit(options.limit);
        if effective_limit == 0 {
            return Vec::new();
        }
        self.inner
            .suggest(input, effective_limit)
            .into_iter()
            .map(|s| SuggestionResult {
                text: s.text,
                kind: s.kind,
                edit_cost: s.edit_cost,
            })
            .collect()
    }

    /// Predicts next-word candidates based on preceding 1 or 2 context words.
    ///
    /// Context behavior:
    /// - 0 words: returns empty
    /// - 1 word: bigram prediction (`context[0]`)
    /// - 2+ words: uses the final two normalized words for trigram lookup with bigram backoff
    pub fn predict_next(&self, context: &[&str], options: PredictionOptions) -> Vec<Prediction> {
        let effective_limit = clamp_limit(options.limit);
        if effective_limit == 0 || context.is_empty() {
            return Vec::new();
        }

        let normalized_context: Vec<String> = context.iter().map(|w| normalize(w)).collect();

        if normalized_context.len() == 1 {
            let preds = self
                .inner
                .predict_next(&normalized_context[0], effective_limit);
            preds
                .into_iter()
                .map(|p| Prediction {
                    text: p.word,
                    count: p.count,
                    probability_millionths: p.probability_millionths,
                    source: PredictionSource::Bigram,
                })
                .collect()
        } else {
            let len = normalized_context.len();
            let prev2 = &normalized_context[len - 2];
            let prev1 = &normalized_context[len - 1];

            let res = self
                .inner
                .predict_next_with_context(prev2, prev1, effective_limit);

            let source = res.source.unwrap_or(PredictionSource::BigramBackoff);

            res.predictions
                .into_iter()
                .map(|p: NextWordPrediction| Prediction {
                    text: p.word,
                    count: p.count,
                    probability_millionths: p.probability_millionths,
                    source,
                })
                .collect()
        }
    }
}

/// Helper function parsing pack header language tag and format version without fully decoding payload.
fn parse_pack_header_metadata(bytes: &[u8]) -> Result<(String, u32), PackLoadError> {
    if bytes.len() < 12 {
        return Err(PackLoadError::TooShort(bytes.len()));
    }
    if &bytes[0..4] != MAGIC_BYTES {
        return Err(PackLoadError::InvalidMagicBytes);
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != PACK_VERSION {
        return Err(PackLoadError::UnsupportedVersion { found: version });
    }

    let cursor = 8;
    if cursor + 2 > bytes.len() {
        return Err(PackLoadError::TruncatedPayload);
    }
    let tag_len = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap()) as usize;
    let tag_start = cursor + 2;
    let tag_end = tag_start + tag_len;
    if tag_end > bytes.len() {
        return Err(PackLoadError::TruncatedPayload);
    }
    let lang_tag = std::str::from_utf8(&bytes[tag_start..tag_end])
        .map_err(|e| PackLoadError::InvalidPayload {
            message: format!("UTF-8 error reading language tag: {}", e),
        })?
        .to_string();

    if lang_tag != "ku-Latn" {
        return Err(PackLoadError::IncompatibleLanguage { found: lang_tag });
    }

    Ok((lang_tag, version))
}
