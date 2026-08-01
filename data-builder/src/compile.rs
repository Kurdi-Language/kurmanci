//! Deterministic Binary Pack Compiler for Kurmancî Language Engine Packs (Version 4).

use crate::corpus::ngrams::{BigramRecord, TrigramRecord};
use crate::validate::SourceLexiconEntry;
pub use kurmanci_engine::format::{
    MAGIC_BYTES, MAX_BIGRAM_PREDICTIONS_PER_CONTEXT, MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT,
    PACK_VERSION, PROBABILITY_SCALE,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Compiles a set of validated lexicon entries and optional n-grams into a deterministic binary pack byte buffer.
pub fn compile_binary_pack_with_root<P: AsRef<Path>>(
    root_dir: P,
    entries: &[SourceLexiconEntry],
) -> Result<Vec<u8>, String> {
    let root = root_dir.as_ref();
    let mut payload = Vec::new();
    let mut lexicon_index_map: BTreeMap<String, u32> = BTreeMap::new();

    // 1. Build Lexicon Section (Section 1)
    for (i, entry) in entries.iter().enumerate() {
        let idx = u32::try_from(i).map_err(|_| "Entry index exceeds u32::MAX".to_string())?;
        lexicon_index_map.insert(entry.normalized.clone(), idx);

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

        // Encode FrequencyMetadata
        let freq_meta = entry
            .frequency_metadata
            .as_ref()
            .cloned()
            .unwrap_or_default();
        payload.extend_from_slice(&freq_meta.token_count.to_le_bytes());
        payload.extend_from_slice(&freq_meta.document_count.to_le_bytes());
        payload.extend_from_slice(&freq_meta.zipf_milli.to_le_bytes());
    }

    // 2. Build Bigram Section (Section 2 - Version 4 Layout using Lexicon Indices)
    let bigrams_path = root.join("data/build/bigrams.jsonl");
    let mut bigram_groups: BTreeMap<u32, Vec<(u32, u64, u32, String)>> = BTreeMap::new();
    let mut seen_bigram_triples: BTreeSet<(String, String)> = BTreeSet::new();

    if bigrams_path.exists() {
        let file = File::open(&bigrams_path)
            .map_err(|e| format!("Failed to open {:?}: {}", bigrams_path, e))?;
        let reader = BufReader::new(file);

        for (line_idx, line_res) in reader.lines().enumerate() {
            let line = line_res.map_err(|e| format!("Read error in {:?}: {}", bigrams_path, e))?;
            if line.trim().is_empty() {
                continue;
            }

            let rec: BigramRecord = serde_json::from_str(&line)
                .map_err(|e| format!("Line {}: invalid bigram record: {}", line_idx + 1, e))?;

            if rec.context_count == 0 {
                return Err(format!(
                    "Line {}: bigram ({}, {}) has zero context_count",
                    line_idx + 1,
                    rec.previous,
                    rec.next
                ));
            }
            if rec.count == 0 {
                return Err(format!(
                    "Line {}: bigram ({}, {}) has zero count",
                    line_idx + 1,
                    rec.previous,
                    rec.next
                ));
            }
            if rec.count > rec.context_count {
                return Err(format!(
                    "Line {}: bigram ({}, {}) count {} exceeds context_count {}",
                    line_idx + 1,
                    rec.previous,
                    rec.next,
                    rec.count,
                    rec.context_count
                ));
            }
            if rec.probability_millionths > PROBABILITY_SCALE {
                return Err(format!(
                    "Line {}: bigram ({}, {}) probability {} exceeds {}",
                    line_idx + 1,
                    rec.previous,
                    rec.next,
                    rec.probability_millionths,
                    PROBABILITY_SCALE
                ));
            }

            let key = (rec.previous.clone(), rec.next.clone());
            if !seen_bigram_triples.insert(key) {
                return Err(format!(
                    "Line {}: duplicate bigram record ({}, {})",
                    line_idx + 1,
                    rec.previous,
                    rec.next
                ));
            }

            if let (Some(&ctx_idx), Some(&next_idx)) = (
                lexicon_index_map.get(&rec.previous),
                lexicon_index_map.get(&rec.next),
            ) {
                bigram_groups.entry(ctx_idx).or_default().push((
                    next_idx,
                    rec.count,
                    rec.probability_millionths,
                    rec.next,
                ));
            }
        }
    }

    let bigram_context_count = u32::try_from(bigram_groups.len())
        .map_err(|_| "Bigram context count exceeds u32::MAX".to_string())?;
    payload.extend_from_slice(&bigram_context_count.to_le_bytes());

    for (ctx_idx, mut predictions) in bigram_groups {
        predictions.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.3.cmp(&b.3))
        });

        if predictions.len() > MAX_BIGRAM_PREDICTIONS_PER_CONTEXT {
            return Err(format!(
                "Bigram context index {} exceeds maximum allowed predictions ({})",
                ctx_idx, MAX_BIGRAM_PREDICTIONS_PER_CONTEXT
            ));
        }

        let pred_count = u16::try_from(predictions.len())
            .map_err(|_| "Prediction count exceeds u16::MAX".to_string())?;

        payload.extend_from_slice(&ctx_idx.to_le_bytes());
        payload.extend_from_slice(&pred_count.to_le_bytes());

        for (next_idx, count, prob, _word) in predictions {
            payload.extend_from_slice(&next_idx.to_le_bytes());
            payload.extend_from_slice(&count.to_le_bytes());
            payload.extend_from_slice(&prob.to_le_bytes());
        }
    }

    // 3. Build Trigram Section (Section 3 - Version 4 Layout using Lexicon Indices)
    let trigrams_path = root.join("data/build/trigrams.jsonl");
    type TrigramGroupMap = BTreeMap<(u32, u32), Vec<(u32, u64, u32, String)>>;
    let mut trigram_groups: TrigramGroupMap = BTreeMap::new();
    let mut seen_trigram_triples: BTreeSet<(String, String, String)> = BTreeSet::new();

    if trigrams_path.exists() {
        let file = File::open(&trigrams_path)
            .map_err(|e| format!("Failed to open {:?}: {}", trigrams_path, e))?;
        let reader = BufReader::new(file);

        for (line_idx, line_res) in reader.lines().enumerate() {
            let line = line_res.map_err(|e| format!("Read error in {:?}: {}", trigrams_path, e))?;
            if line.trim().is_empty() {
                continue;
            }

            let rec: TrigramRecord = serde_json::from_str(&line)
                .map_err(|e| format!("Line {}: invalid trigram record: {}", line_idx + 1, e))?;

            if rec.context_count == 0 {
                return Err(format!(
                    "Line {}: trigram ({}, {}, {}) has zero context_count",
                    line_idx + 1,
                    rec.previous_2,
                    rec.previous_1,
                    rec.next
                ));
            }
            if rec.count == 0 {
                return Err(format!(
                    "Line {}: trigram ({}, {}, {}) has zero count",
                    line_idx + 1,
                    rec.previous_2,
                    rec.previous_1,
                    rec.next
                ));
            }
            if rec.count > rec.context_count {
                return Err(format!(
                    "Line {}: trigram ({}, {}, {}) count {} exceeds context_count {}",
                    line_idx + 1,
                    rec.previous_2,
                    rec.previous_1,
                    rec.next,
                    rec.count,
                    rec.context_count
                ));
            }
            if rec.probability_millionths > PROBABILITY_SCALE {
                return Err(format!(
                    "Line {}: trigram ({}, {}, {}) probability {} exceeds {}",
                    line_idx + 1,
                    rec.previous_2,
                    rec.previous_1,
                    rec.next,
                    rec.probability_millionths,
                    PROBABILITY_SCALE
                ));
            }

            let key = (
                rec.previous_2.clone(),
                rec.previous_1.clone(),
                rec.next.clone(),
            );
            if !seen_trigram_triples.insert(key) {
                return Err(format!(
                    "Line {}: duplicate trigram record ({}, {}, {})",
                    line_idx + 1,
                    rec.previous_2,
                    rec.previous_1,
                    rec.next
                ));
            }

            if let (Some(&prev2_idx), Some(&prev1_idx), Some(&next_idx)) = (
                lexicon_index_map.get(&rec.previous_2),
                lexicon_index_map.get(&rec.previous_1),
                lexicon_index_map.get(&rec.next),
            ) {
                trigram_groups
                    .entry((prev2_idx, prev1_idx))
                    .or_default()
                    .push((next_idx, rec.count, rec.probability_millionths, rec.next));
            }
        }
    }

    let trigram_context_count = u32::try_from(trigram_groups.len())
        .map_err(|_| "Trigram context count exceeds u32::MAX".to_string())?;
    payload.extend_from_slice(&trigram_context_count.to_le_bytes());

    for ((prev2_idx, prev1_idx), mut predictions) in trigram_groups {
        predictions.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.3.cmp(&b.3))
        });

        if predictions.len() > MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT {
            return Err(format!(
                "Trigram context ({}, {}) exceeds maximum allowed predictions ({})",
                prev2_idx, prev1_idx, MAX_TRIGRAM_PREDICTIONS_PER_CONTEXT
            ));
        }

        let pred_count = u16::try_from(predictions.len())
            .map_err(|_| "Trigram prediction count exceeds u16::MAX".to_string())?;

        payload.extend_from_slice(&prev2_idx.to_le_bytes());
        payload.extend_from_slice(&prev1_idx.to_le_bytes());
        payload.extend_from_slice(&pred_count.to_le_bytes());

        for (next_idx, count, prob, _word) in predictions {
            payload.extend_from_slice(&next_idx.to_le_bytes());
            payload.extend_from_slice(&count.to_le_bytes());
            payload.extend_from_slice(&prob.to_le_bytes());
        }
    }

    // 4. Construct Header
    let language_tag = "ku-Latn";
    let lang_bytes = language_tag.as_bytes();
    let lang_len = u16::try_from(lang_bytes.len())
        .map_err(|_| "Language tag length exceeds u16::MAX".to_string())?;

    let entry_count = u32::try_from(entries.len())
        .map_err(|_| "Lexicon entry count exceeds u32::MAX".to_string())?;

    let payload_len =
        u64::try_from(payload.len()).map_err(|_| "Payload length exceeds u64::MAX".to_string())?;

    let checksum_bytes: [u8; 32] = Sha256::digest(&payload).into();

    let mut header = Vec::new();
    header.extend_from_slice(MAGIC_BYTES);
    header.extend_from_slice(&PACK_VERSION.to_le_bytes());
    header.extend_from_slice(&lang_len.to_le_bytes());
    header.extend_from_slice(lang_bytes);
    header.extend_from_slice(&entry_count.to_le_bytes());
    header.extend_from_slice(&payload_len.to_le_bytes());
    header.extend_from_slice(&checksum_bytes);

    let mut pack = Vec::with_capacity(header.len() + payload.len());
    pack.extend_from_slice(&header);
    pack.extend_from_slice(&payload);

    Ok(pack)
}

fn write_string(buf: &mut Vec<u8>, s: &str) -> Result<(), String> {
    let bytes = s.as_bytes();
    let len = u16::try_from(bytes.len())
        .map_err(|_| format!("String length {} exceeds u16::MAX", bytes.len()))?;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(bytes);
    Ok(())
}

/// Convenience wrapper compiling binary pack for current workspace.
pub fn compile_binary_pack(entries: &[SourceLexiconEntry]) -> Result<Vec<u8>, String> {
    compile_binary_pack_with_root(".", entries)
}

/// Calculates SHA-256 hex string for given bytes.
pub fn calculate_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Release manifest metadata struct.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// Writes compiled binary pack and release manifest to target directory.
pub fn write_artifacts<P: AsRef<Path>>(
    build_dir: P,
    pack_bytes: &[u8],
    manifest: &ReleaseManifest,
) -> Result<(), String> {
    let dir = build_dir.as_ref();
    std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create build dir: {e}"))?;

    let pack_path = dir.join("lexicon.bin");
    std::fs::write(&pack_path, pack_bytes)
        .map_err(|e| format!("Failed to write binary pack: {e}"))?;

    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|e| format!("Failed to serialize manifest: {e}"))?;
    std::fs::write(&manifest_path, manifest_bytes)
        .map_err(|e| format!("Failed to write manifest: {e}"))?;

    Ok(())
}
