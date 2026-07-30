use crate::distance::weighted_damerau_levenshtein;
use crate::normalization::{normalize, strip_diacritics};
use crate::ranking::{calculate_score, Suggestion, SuggestionKind};
use crate::trie::Trie;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::convert::TryInto;

pub const MAGIC_BYTES: &[u8; 4] = b"KRM1";
pub const PACK_VERSION: u32 = 1;

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
}

#[derive(Default)]
pub struct Engine {
    lexicon: Vec<LexiconEntry>,
    trie: Trie,
    max_frequency: u64,
    typo_map: HashMap<String, String>,
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

    /// Loads a compiled binary pack (.bin format) with strict header and SHA-256 checksum integrity verification.
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

        // 6. Decode Payload Entries
        // 6. Decode Payload Entries into Staging Buffers (Atomic Safety)
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

            let entry = LexiconEntry {
                word,
                normalized,
                lemma,
                part_of_speech,
                frequency,
                regions,
                status,
                sources,
            };

            if entry.frequency > staged_max_frequency {
                staged_max_frequency = entry.frequency;
            }
            staged_trie.insert(&entry.normalized, entry.frequency);
            staged_lexicon.push(entry);
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

        Ok(loaded)
    }

    /// Loads custom typo pairs (e.g. `rojbas -> rojbaş`, `biji -> bijî`).
    pub fn load_typos(&mut self, typos: HashMap<String, String>) {
        self.typo_map = typos;
    }

    /// Returns whether a given word exists in the dictionary.
    pub fn contains(&self, word: &str) -> bool {
        let norm = normalize(word);
        self.trie.contains(&norm)
    }

    /// Generates prefix completion candidates.
    pub fn complete(&self, prefix: &str, limit: usize) -> Vec<Suggestion> {
        let norm_prefix = normalize(prefix);
        if norm_prefix.is_empty() {
            return Vec::new();
        }

        let matches = self.trie.find_by_prefix(&norm_prefix);
        let mut suggestions: Vec<Suggestion> = matches
            .into_iter()
            .map(|(norm_word, freq)| {
                // Retrieve original display word from lexicon if available
                let display_word = self
                    .lexicon
                    .iter()
                    .find(|e| e.normalized == norm_word)
                    .map(|e| e.word.clone())
                    .unwrap_or_else(|| norm_word.clone());

                let kind = if norm_word == norm_prefix {
                    SuggestionKind::Exact
                } else {
                    SuggestionKind::Completion
                };
                let score = calculate_score(
                    &norm_prefix,
                    &norm_word,
                    freq,
                    self.max_frequency,
                    0.0,
                    kind.clone(),
                );
                Suggestion {
                    text: display_word,
                    score,
                    kind,
                }
            })
            .collect();

        suggestions.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        suggestions.truncate(limit);
        suggestions
    }

    /// Generates spelling and diacritic corrections for a given word.
    pub fn correct(&self, word: &str, limit: usize) -> Vec<Suggestion> {
        self.suggest(word, limit)
    }

    /// Full suggestion pipeline executing exact match -> prefix completion -> diacritic restoration -> weighted edit distance.
    pub fn suggest(&self, query: &str, limit: usize) -> Vec<Suggestion> {
        let norm_query = normalize(query);
        if norm_query.is_empty() {
            return Vec::new();
        }

        let query_stripped = strip_diacritics(&norm_query);
        let mut candidates_map: HashMap<String, Suggestion> = HashMap::new();

        // 1. Direct typo dictionary check
        if let Some(intended) = self.typo_map.get(&norm_query) {
            let freq = self
                .lexicon
                .iter()
                .find(|e| &e.normalized == intended)
                .map(|e| e.frequency)
                .unwrap_or(50000);
            let display_text = self
                .lexicon
                .iter()
                .find(|e| &e.normalized == intended)
                .map(|e| e.word.clone())
                .unwrap_or_else(|| intended.clone());

            let score = calculate_score(
                &norm_query,
                intended,
                freq,
                self.max_frequency,
                0.25,
                SuggestionKind::DiacriticCorrection,
            );
            candidates_map.insert(
                intended.clone(),
                Suggestion {
                    text: display_text,
                    score,
                    kind: SuggestionKind::DiacriticCorrection,
                },
            );
        }

        // 2. Exact match & Prefix completion from Trie
        let prefix_matches = self.trie.find_by_prefix(&norm_query);
        for (norm_word, freq) in prefix_matches {
            let display_word = self
                .lexicon
                .iter()
                .find(|e| e.normalized == norm_word)
                .map(|e| e.word.clone())
                .unwrap_or_else(|| norm_word.clone());

            let kind = if norm_word == norm_query {
                SuggestionKind::Exact
            } else {
                SuggestionKind::Completion
            };
            let score = calculate_score(
                &norm_query,
                &norm_word,
                freq,
                self.max_frequency,
                0.0,
                kind.clone(),
            );
            candidates_map.insert(
                norm_word,
                Suggestion {
                    text: display_word,
                    score,
                    kind,
                },
            );
        }

        // 3. Scan Lexicon for Diacritic Restoration & Edit Distance
        for entry in &self.lexicon {
            let candidate_norm = &entry.normalized;
            let candidate_stripped = strip_diacritics(candidate_norm);

            // Diacritic restoration match
            if candidate_stripped == query_stripped && candidate_norm != &norm_query {
                let dist = weighted_damerau_levenshtein(&norm_query, candidate_norm);
                let score = calculate_score(
                    &norm_query,
                    candidate_norm,
                    entry.frequency,
                    self.max_frequency,
                    dist,
                    SuggestionKind::DiacriticCorrection,
                );
                candidates_map
                    .entry(candidate_norm.clone())
                    .and_modify(|existing| {
                        if score > existing.score {
                            existing.score = score;
                            existing.kind = SuggestionKind::DiacriticCorrection;
                        }
                    })
                    .or_insert(Suggestion {
                        text: entry.word.clone(),
                        score,
                        kind: SuggestionKind::DiacriticCorrection,
                    });
                continue;
            }

            // Weighted edit distance fallback (filtering length difference)
            let len_diff = (candidate_norm.chars().count() as isize
                - norm_query.chars().count() as isize)
                .abs();
            if len_diff <= 2 {
                let dist = weighted_damerau_levenshtein(&norm_query, candidate_norm);
                if dist <= 2.0 {
                    let kind = if candidate_norm.starts_with(&norm_query) {
                        SuggestionKind::Completion
                    } else {
                        SuggestionKind::Correction
                    };
                    let score = calculate_score(
                        &norm_query,
                        candidate_norm,
                        entry.frequency,
                        self.max_frequency,
                        dist,
                        kind.clone(),
                    );
                    candidates_map
                        .entry(candidate_norm.clone())
                        .and_modify(|existing| {
                            if score > existing.score {
                                existing.score = score;
                            }
                        })
                        .or_insert(Suggestion {
                            text: entry.word.clone(),
                            score,
                            kind,
                        });
                }
            }
        }

        let mut results: Vec<Suggestion> = candidates_map.into_values().collect();
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
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
