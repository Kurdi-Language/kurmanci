use crate::distance::weighted_damerau_levenshtein;
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

pub const MAGIC_BYTES: &[u8; 4] = b"KRM1";
pub const PACK_VERSION: u32 = 3;

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

#[derive(Default)]
pub struct Engine {
    lexicon: Vec<LexiconEntry>,
    trie: Trie,
    max_frequency: u64,
    #[allow(dead_code)]
    typo_map: HashMap<String, String>,
    bigram_index: HashMap<usize, Vec<(usize, u64, u32)>>,
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
    pub fn load_lexicon(&mut self, entries: Vec<LexiconEntry>) {
        for entry in entries {
            if entry.frequency > self.max_frequency {
                self.max_frequency = entry.frequency;
            }
            self.trie.insert(&entry.normalized, entry.frequency);
            self.lexicon.push(entry);
        }
    }

    /// Loads custom typo mappings into the engine.
    pub fn load_typos(&mut self, typos: HashMap<String, String>) {
        self.typo_map = typos;
    }

    /// Loads a compiled binary pack (.bin format v2) with strict header and SHA-256 checksum integrity verification.
    pub fn load_binary_pack(&mut self, bytes: &[u8]) -> Result<usize, String> {
        if bytes.len() < 12 {
            return Err("Binary pack file too short".to_string());
        }

        // 1. Magic Bytes Check
        if &bytes[0..4] != MAGIC_BYTES {
            return Err("Invalid magic bytes in binary pack".to_string());
        }

        // 2. Format Version Check
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != PACK_VERSION {
            return Err(format!("Unsupported format version {}", version));
        }

        let mut cursor = 8;

        // 3. Language Tag Check
        let (lang_tag, new_cursor) = read_string(bytes, cursor)?;
        cursor = new_cursor;
        if lang_tag != "ku-Latn" {
            return Err(format!(
                "Incompatible language tag '{}' (expected 'ku-Latn')",
                lang_tag
            ));
        }

        // 4. Entry Count & Payload Length
        if cursor + 12 > bytes.len() {
            return Err("Truncated binary pack header".to_string());
        }
        let count = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;

        let payload_len = usize::try_from(u64::from_le_bytes(
            bytes[cursor..cursor + 8].try_into().unwrap(),
        ))
        .map_err(|_| "Payload length does not fit this platform")?;
        cursor += 8;

        // 5. Payload Checksum Verification
        if cursor + 32 > bytes.len() {
            return Err("Truncated checksum in binary header".to_string());
        }
        let stored_checksum: [u8; 32] = bytes[cursor..cursor + 32].try_into().unwrap();
        cursor += 32;

        let payload_bytes = &bytes[cursor..];
        if payload_bytes.len() != payload_len {
            return Err(format!(
                "Payload size mismatch: expected {} bytes, found {}",
                payload_len,
                payload_bytes.len()
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(payload_bytes);
        let computed_checksum: [u8; 32] = hasher.finalize().into();

        if stored_checksum != computed_checksum {
            return Err("Binary pack corrupted: payload SHA-256 checksum mismatch".to_string());
        }

        // 6. Decode Payload Entries into Staging Buffers (Version 2)
        let mut staged_lexicon = Vec::with_capacity(count as usize);
        let mut staged_trie = Trie::default();
        let mut staged_max_frequency = 0u64;
        let mut payload_cursor = 0;

        for _ in 0..count {
            let (word, new_c) = read_string(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;
            let (lemma, new_c) = read_string(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;
            let (normalized, new_c) = read_string(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;
            let (part_of_speech, new_c) = read_string(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;

            if payload_cursor + 8 > payload_bytes.len() {
                return Err("Unexpected EOF reading entry frequency".to_string());
            }
            let frequency = u64::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 8]
                    .try_into()
                    .unwrap(),
            );
            payload_cursor += 8;

            let (status, new_c) = read_string(payload_bytes, payload_cursor)?;
            payload_cursor = new_c;

            if payload_cursor + 2 > payload_bytes.len() {
                return Err("Unexpected EOF reading regions count".to_string());
            }
            let regions_count = u16::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 2;

            let mut regions = Vec::with_capacity(regions_count);
            for _ in 0..regions_count {
                let (reg, new_c) = read_string(payload_bytes, payload_cursor)?;
                payload_cursor = new_c;
                regions.push(reg);
            }

            if payload_cursor + 2 > payload_bytes.len() {
                return Err("Unexpected EOF reading sources count".to_string());
            }
            let sources_count = u16::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 2;

            let mut sources = Vec::with_capacity(sources_count);
            for _ in 0..sources_count {
                let (src, new_c) = read_string(payload_bytes, payload_cursor)?;
                payload_cursor = new_c;
                sources.push(src);
            }

            // Decode FrequencyMetadata (Version 2 layout: u64 token_count, u64 doc_count, u32 zipf_milli)
            if payload_cursor + 20 > payload_bytes.len() {
                return Err(
                    "Unexpected EOF reading frequency metadata in v2 binary pack".to_string(),
                );
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

            let entry = LexiconEntry {
                word,
                normalized,
                lemma,
                part_of_speech,
                frequency,
                regions,
                status,
                sources,
                frequency_metadata: FrequencyMetadata {
                    token_count,
                    document_count,
                    zipf_milli,
                },
            };

            if entry.frequency > staged_max_frequency {
                staged_max_frequency = entry.frequency;
            }
            staged_trie.insert(&entry.normalized, entry.frequency);
            staged_lexicon.push(entry);
        }

        // 7. Decode Bigram Section (Version 3 Extension - Lexicon Indices)
        if payload_cursor + 4 > payload_bytes.len() {
            return Err("Unexpected EOF reading bigram context count".to_string());
        }
        let bigram_context_count = u32::from_le_bytes(
            payload_bytes[payload_cursor..payload_cursor + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        payload_cursor += 4;

        if bigram_context_count > staged_lexicon.len() {
            return Err(format!(
                "Bigram context count {} exceeds lexicon count {}",
                bigram_context_count,
                staged_lexicon.len()
            ));
        }

        let mut staged_bigram_index: HashMap<usize, Vec<(usize, u64, u32)>> =
            HashMap::with_capacity(bigram_context_count);

        for _ in 0..bigram_context_count {
            if payload_cursor + 6 > payload_bytes.len() {
                return Err("Unexpected EOF reading bigram context header".to_string());
            }
            let ctx_idx = u32::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 4;

            if ctx_idx >= staged_lexicon.len() {
                return Err(format!(
                    "Context index {} out of lexicon bounds (lexicon count {})",
                    ctx_idx,
                    staged_lexicon.len()
                ));
            }

            if staged_bigram_index.contains_key(&ctx_idx) {
                return Err(format!(
                    "Duplicate context index {} in binary pack",
                    ctx_idx
                ));
            }

            let pred_count = u16::from_le_bytes(
                payload_bytes[payload_cursor..payload_cursor + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            payload_cursor += 2;

            if pred_count == 0 {
                return Err(format!("Context index {} has zero predictions", ctx_idx));
            }
            if pred_count > 16 {
                return Err(format!(
                    "Context index {} has prediction count {} exceeding maximum 16",
                    ctx_idx, pred_count
                ));
            }

            let mut predictions = Vec::with_capacity(pred_count);
            let mut seen_next_indices = std::collections::HashSet::with_capacity(pred_count);

            for _ in 0..pred_count {
                if payload_cursor + 16 > payload_bytes.len() {
                    return Err("Unexpected EOF reading bigram prediction entry".to_string());
                }
                let next_idx = u32::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 4]
                        .try_into()
                        .unwrap(),
                ) as usize;
                payload_cursor += 4;

                if next_idx >= staged_lexicon.len() {
                    return Err(format!(
                        "Next lexicon index {} out of lexicon bounds (lexicon count {})",
                        next_idx,
                        staged_lexicon.len()
                    ));
                }

                if !seen_next_indices.insert(next_idx) {
                    return Err(format!(
                        "Duplicate next lexicon index {} in context {}",
                        next_idx, ctx_idx
                    ));
                }

                let count = u64::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 8]
                        .try_into()
                        .unwrap(),
                );
                payload_cursor += 8;

                if count == 0 {
                    return Err(format!(
                        "Zero bigram count for prediction index {} in context {}",
                        next_idx, ctx_idx
                    ));
                }

                let prob = u32::from_le_bytes(
                    payload_bytes[payload_cursor..payload_cursor + 4]
                        .try_into()
                        .unwrap(),
                );
                payload_cursor += 4;

                if prob > 1_000_000 {
                    return Err(format!(
                        "Probability {} exceeds 1,000,000 for prediction index {} in context {}",
                        prob, next_idx, ctx_idx
                    ));
                }

                predictions.push((next_idx, count, prob));
            }

            staged_bigram_index.insert(ctx_idx, predictions);
        }

        // Verify complete payload consumption
        if payload_cursor != payload_bytes.len() {
            return Err(format!(
                "Trailing payload bytes: {} bytes remain",
                payload_bytes.len() - payload_cursor
            ));
        }

        // Atomically replace engine state upon 100% successful parsing
        let loaded = staged_lexicon.len();
        self.lexicon = staged_lexicon;
        self.trie = staged_trie;
        self.max_frequency = staged_max_frequency;
        self.bigram_index = staged_bigram_index;

        Ok(loaded)
    }

    pub fn contains(&self, word: &str) -> bool {
        let norm = crate::normalization::normalize(word);
        self.trie.contains(&norm)
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

        let ctx_idx = match self.lexicon.iter().position(|e| e.normalized == norm_prev) {
            Some(idx) => idx,
            None => return Vec::new(),
        };

        let preds = match self.bigram_index.get(&ctx_idx) {
            Some(p) => p,
            None => return Vec::new(),
        };

        let mut results: Vec<NextWordPrediction> = preds
            .iter()
            .map(|(next_idx, count, prob)| NextWordPrediction {
                word: self.lexicon[*next_idx].word.clone(),
                count: *count,
                probability_millionths: *prob,
            })
            .collect();

        results.sort_by(|a, b| {
            b.probability_millionths
                .cmp(&a.probability_millionths)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.word.cmp(&b.word))
        });

        results.truncate(limit);
        results
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
    pub fn suggest_with_config(
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
        let prefix_matches = self.trie.find_by_prefix(&norm_query);
        for (norm_word, _freq) in prefix_matches {
            let lex_entry = self.lexicon.iter().find(|e| e.normalized == norm_word);
            let display_word = lex_entry
                .map(|e| e.word.clone())
                .unwrap_or_else(|| norm_word.clone());
            let freq_meta = lex_entry
                .map(|e| e.frequency_metadata.clone())
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

        // 2. Scan Lexicon for Diacritic Restoration & Edit Distance
        for entry in &self.lexicon {
            let candidate_norm = &entry.normalized;
            let candidate_stripped = strip_diacritics(candidate_norm);
            let is_diac_match = candidate_stripped == query_stripped;

            // Diacritic restoration match
            if is_diac_match && candidate_norm != &norm_query {
                let candidate = RankedCandidate {
                    word: entry.word.clone(),
                    edit_cost: 0, // diacritic restoration is 0 edit distance penalty
                    is_diacritic_match: true,
                    prefix_quality: 0,
                    frequency: entry.frequency_metadata.clone(),
                    kind: SuggestionKind::DiacriticCorrection,
                };
                candidate_ranks.insert(candidate_norm.clone(), candidate);
                continue;
            }

            // Weighted edit distance fallback
            let len_diff = (candidate_norm.chars().count() as isize
                - norm_query.chars().count() as isize)
                .abs();
            if len_diff <= 2 {
                let dist = weighted_damerau_levenshtein(&norm_query, candidate_norm);
                if dist <= 2.0 {
                    let is_exact = candidate_norm == &norm_query;
                    let edit_cost = if is_exact {
                        0
                    } else {
                        (dist * 10.0).round() as u32
                    };
                    let kind = if is_exact {
                        SuggestionKind::Exact
                    } else if candidate_norm.starts_with(&norm_query) {
                        SuggestionKind::Completion
                    } else {
                        SuggestionKind::Correction
                    };
                    let candidate = RankedCandidate {
                        word: entry.word.clone(),
                        edit_cost,
                        is_diacritic_match: is_diac_match,
                        prefix_quality: if candidate_norm.starts_with(&norm_query) {
                            norm_query.chars().count() as u32
                        } else {
                            0
                        },
                        frequency: entry.frequency_metadata.clone(),
                        kind,
                    };

                    candidate_ranks
                        .entry(candidate_norm.clone())
                        .and_modify(|existing| {
                            if candidate.cmp_with_config(existing, config)
                                == std::cmp::Ordering::Less
                            {
                                *existing = candidate.clone();
                            }
                        })
                        .or_insert(candidate);
                }
            }
        }

        let mut ranked_list: Vec<RankedCandidate> = candidate_ranks.into_values().collect();
        ranked_list.sort_by(|a, b| a.cmp_with_config(b, config));
        ranked_list.truncate(limit);

        // Convert RankedCandidate to public Suggestion
        ranked_list
            .into_iter()
            .map(|rc| {
                let score = calculate_score(
                    &norm_query,
                    &rc.word,
                    rc.frequency.token_count,
                    1000,
                    rc.edit_cost as f64 / 10.0,
                    rc.kind.clone(),
                );
                Suggestion {
                    text: rc.word.clone(),
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
}

fn read_string(bytes: &[u8], cursor: usize) -> Result<(String, usize), String> {
    if cursor + 2 > bytes.len() {
        return Err("Unexpected EOF reading string length".to_string());
    }
    let len = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap()) as usize;
    let start = cursor + 2;
    let end = start + len;
    if end > bytes.len() {
        return Err("Unexpected EOF reading string content".to_string());
    }
    let s = std::str::from_utf8(&bytes[start..end])
        .map_err(|e| format!("UTF-8 error: {}", e))?
        .to_string();
    Ok((s, end))
}
