use kurmanci_engine::Engine;
use sha2::{Digest, Sha256};

fn build_test_pack(entry_count: u32, payload_bytes: &[u8]) -> Vec<u8> {
    let mut pack = Vec::new();
    pack.extend_from_slice(b"KRM1");
    pack.extend_from_slice(&1u32.to_le_bytes());

    let lang = b"ku-Latn";
    pack.extend_from_slice(&(lang.len() as u16).to_le_bytes());
    pack.extend_from_slice(lang);

    pack.extend_from_slice(&entry_count.to_le_bytes());
    pack.extend_from_slice(&(payload_bytes.len() as u64).to_le_bytes());

    let mut hasher = Sha256::new();
    hasher.update(payload_bytes);
    let checksum: [u8; 32] = hasher.finalize().into();
    pack.extend_from_slice(&checksum);

    pack.extend_from_slice(payload_bytes);
    pack
}

fn encode_test_entry(word: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    let encode_str = |b: &mut Vec<u8>, s: &str| {
        let bytes = s.as_bytes();
        b.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        b.extend_from_slice(bytes);
    };

    encode_str(&mut buf, word);
    encode_str(&mut buf, word);
    encode_str(&mut buf, word);
    encode_str(&mut buf, "noun");
    buf.extend_from_slice(&100u64.to_le_bytes());
    encode_str(&mut buf, "verified");
    buf.extend_from_slice(&1u16.to_le_bytes());
    encode_str(&mut buf, "general");
    buf.extend_from_slice(&1u16.to_le_bytes());
    encode_str(&mut buf, "manual-seed");

    buf
}

#[test]
fn test_invalid_magic_bytes() {
    let mut engine = Engine::new();
    let mut bad_bytes = vec![0u8; 61];
    bad_bytes[0..4].copy_from_slice(b"WRNG");
    let res = engine.load_binary_pack(&bad_bytes);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Invalid magic bytes"));
}

#[test]
fn test_unsupported_version() {
    let mut engine = Engine::new();
    let mut bad_bytes = vec![0u8; 61];
    bad_bytes[0..4].copy_from_slice(b"KRM1");
    bad_bytes[4..8].copy_from_slice(&999u32.to_le_bytes());
    let res = engine.load_binary_pack(&bad_bytes);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Unsupported format version"));
}

#[test]
fn test_truncated_header() {
    let mut engine = Engine::new();
    let short_bytes = b"KRM1";
    let res = engine.load_binary_pack(short_bytes);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Binary pack file too short"));
}

#[test]
fn test_checksum_corruption() {
    let pack_path = std::path::Path::new("data/build/lexicon.bin");
    if !pack_path.exists() {
        eprintln!("Skipping test_checksum_corruption: data/build/lexicon.bin not found");
        return;
    }

    let mut engine = Engine::new();
    let pack_bytes = std::fs::read(pack_path).expect("Failed to read binary pack file");
    let mut corrupted = pack_bytes.clone();

    if let Some(last) = corrupted.last_mut() {
        *last ^= 0xFF;
    }

    let res = engine.load_binary_pack(&corrupted);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("checksum mismatch"));
}

#[test]
fn test_one_trailing_byte() {
    let mut engine = Engine::new();
    let mut payload = encode_test_entry("rojbaş");
    payload.push(0xFF);

    let pack = build_test_pack(1, &payload);
    let res = engine.load_binary_pack(&pack);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Trailing payload bytes"));
}

#[test]
fn test_complete_extra_encoded_entry_not_in_entry_count() {
    let mut engine = Engine::new();
    let mut payload = encode_test_entry("rojbaş");
    let extra = encode_test_entry("bijî");
    payload.extend_from_slice(&extra);

    let pack = build_test_pack(1, &payload);
    let res = engine.load_binary_pack(&pack);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Trailing payload bytes"));
}

#[test]
fn test_zero_entries_with_nonempty_payload() {
    let mut engine = Engine::new();
    let payload = encode_test_entry("rojbaş");

    let pack = build_test_pack(0, &payload);
    let res = engine.load_binary_pack(&pack);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Trailing payload bytes"));
}

#[test]
fn test_declared_count_lower_than_actual() {
    let mut engine = Engine::new();
    let mut payload = Vec::new();
    payload.extend_from_slice(&encode_test_entry("one"));
    payload.extend_from_slice(&encode_test_entry("two"));
    payload.extend_from_slice(&encode_test_entry("three"));

    let pack = build_test_pack(2, &payload);
    let res = engine.load_binary_pack(&pack);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Trailing payload bytes"));
}
