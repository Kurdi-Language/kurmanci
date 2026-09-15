//! Deterministic pack-schema-4 builder for CLI tests (mirrors the data-builder encoding for
//! the subset it covers, so the pack is a genuine v4 pack). Duplicated from the engine test
//! helpers until they are shared through a common crate.

use sha2::{Digest, Sha256};

pub const PACK_SCHEMA: u32 = 4;
pub const LANGUAGE_TAG: &str = "ku-Latn";

pub fn encode_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// One lexicon entry with the given word used as word, lemma and normalized form.
pub fn encode_entry(word: &str, normalized: &str, frequency: u64, token_count: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_str(&mut buf, word);
    encode_str(&mut buf, word);
    encode_str(&mut buf, normalized);
    encode_str(&mut buf, "noun");
    buf.extend_from_slice(&frequency.to_le_bytes());
    encode_str(&mut buf, "approved");
    buf.extend_from_slice(&1u16.to_le_bytes());
    encode_str(&mut buf, "general");
    buf.extend_from_slice(&1u16.to_le_bytes());
    encode_str(&mut buf, "manual-seed");
    buf.extend_from_slice(&token_count.to_le_bytes());
    buf.extend_from_slice(&(token_count / 2).to_le_bytes());
    buf.extend_from_slice(&6500u32.to_le_bytes());
    buf
}

/// `(context index, [(next index, count, probability millionths)])`
pub type BigramContext = (u32, Vec<(u32, u64, u32)>);
/// `(prev2 index, prev1 index, [(next index, count, probability millionths)])`
pub type TrigramContext = (u32, u32, Vec<(u32, u64, u32)>);

pub fn encode_bigram_section(buf: &mut Vec<u8>, contexts: &[BigramContext]) {
    buf.extend_from_slice(&(contexts.len() as u32).to_le_bytes());
    for (ctx, preds) in contexts {
        buf.extend_from_slice(&ctx.to_le_bytes());
        buf.extend_from_slice(&(preds.len() as u16).to_le_bytes());
        for (next, count, prob) in preds {
            buf.extend_from_slice(&next.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
            buf.extend_from_slice(&prob.to_le_bytes());
        }
    }
}

pub fn encode_trigram_section(buf: &mut Vec<u8>, contexts: &[TrigramContext]) {
    buf.extend_from_slice(&(contexts.len() as u32).to_le_bytes());
    for (p2, p1, preds) in contexts {
        buf.extend_from_slice(&p2.to_le_bytes());
        buf.extend_from_slice(&p1.to_le_bytes());
        buf.extend_from_slice(&(preds.len() as u16).to_le_bytes());
        for (next, count, prob) in preds {
            buf.extend_from_slice(&next.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
            buf.extend_from_slice(&prob.to_le_bytes());
        }
    }
}

/// Wraps a payload in a pack header with the given schema version and language tag and a
/// correct checksum, so that only the fields under test differ from a valid pack.
pub fn wrap_pack(schema: u32, language_tag: &str, entry_count: u32, payload: &[u8]) -> Vec<u8> {
    let mut pack = Vec::new();
    pack.extend_from_slice(b"KRM1");
    pack.extend_from_slice(&schema.to_le_bytes());
    encode_str(&mut pack, language_tag);
    pack.extend_from_slice(&entry_count.to_le_bytes());
    pack.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    let checksum: [u8; 32] = Sha256::digest(payload).into();
    pack.extend_from_slice(&checksum);
    pack.extend_from_slice(payload);
    pack
}

/// The canonical minimal v4 pack: three entries ("roj", "roja", "baş"), one bigram context
/// (`roj` → `roja`, `baş`) and one trigram context (`roj roja` → `baş`).
pub fn minimal_v4_payload() -> (u32, Vec<u8>) {
    let mut payload = Vec::new();
    payload.extend_from_slice(&encode_entry("roj", "roj", 300, 300));
    payload.extend_from_slice(&encode_entry("roja", "roja", 200, 200));
    payload.extend_from_slice(&encode_entry("baş", "baş", 100, 100));
    encode_bigram_section(
        &mut payload,
        &[(0, vec![(1, 20, 666_667), (2, 10, 333_333)])],
    );
    encode_trigram_section(&mut payload, &[(0, 1, vec![(2, 5, 1_000_000)])]);
    (3, payload)
}

pub fn minimal_v4_pack() -> Vec<u8> {
    let (count, payload) = minimal_v4_payload();
    wrap_pack(PACK_SCHEMA, LANGUAGE_TAG, count, &payload)
}
