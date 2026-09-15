//! Pack compatibility contract and loader fail-safety.
//!
//! - the committed `fixtures/pack-v4-minimal.bin` is byte-identical to what the schema-4
//!   builder produces today and loads with the expected answers (backward-compatibility
//!   anchor: it must keep loading for as long as schema 4 is supported);
//! - older, newer, wrong-language, truncated and corrupt packs fail with the documented
//!   error and classification, never with a panic;
//! - every single-byte mutation and every truncation of a valid pack fails closed;
//! - counts declared in a corrupt header cannot trigger a huge allocation.

mod common;

use common::*;
use kurmanci_engine::{
    probe_pack_header, CompatibilityTable, Engine, KurmanciEngine, LoadFailureClass, PackLoadError,
    PredictionOptions, SuggestOptions, PACK_SCHEMA_VERSION, SUPPORTED_LANGUAGE_TAG,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

const FIXTURE: &[u8] = include_bytes!("fixtures/pack-v4-minimal.bin");

fn load(bytes: &[u8]) -> Result<usize, PackLoadError> {
    let mut engine = Engine::new();
    engine.load_binary_pack(bytes)
}

/// Loads `bytes` and asserts that the load returned (with any result) instead of panicking.
fn load_without_panic(bytes: &[u8], what: &str) -> Result<usize, PackLoadError> {
    catch_unwind(AssertUnwindSafe(|| load(bytes)))
        .unwrap_or_else(|_| panic!("loader panicked on {}", what))
}

#[test]
fn committed_v4_fixture_matches_builder_and_loads() {
    assert_eq!(
        FIXTURE,
        minimal_v4_pack().as_slice(),
        "fixtures/pack-v4-minimal.bin must stay byte-identical to the schema-4 encoding"
    );
    let engine = KurmanciEngine::from_pack_bytes(FIXTURE).unwrap();
    let info = engine.pack_info();
    assert_eq!(info.format_version, PACK_SCHEMA_VERSION);
    assert_eq!(info.language_tag, SUPPORTED_LANGUAGE_TAG);
    assert_eq!(info.entry_count, 3);
    assert!(engine.is_known_word("roj"));
    assert!(engine.is_known_word("Baş"));
    assert!(!engine.is_known_word("rojb"));
    let completions: Vec<String> = engine
        .suggest("ro", SuggestOptions { limit: 5 })
        .into_iter()
        .map(|s| s.text)
        .collect();
    assert_eq!(completions, vec!["roj", "roja"]);
    let preds: Vec<String> = engine
        .predict_next(&["roj"], PredictionOptions { limit: 5 })
        .into_iter()
        .map(|p| p.text)
        .collect();
    assert_eq!(preds, vec!["roja", "baş"]);
    let tri: Vec<String> = engine
        .predict_next(&["roj", "roja"], PredictionOptions { limit: 5 })
        .into_iter()
        .map(|p| p.text)
        .collect();
    assert_eq!(tri, vec!["baş"]);

    let header = probe_pack_header(FIXTURE).unwrap();
    assert!(header.is_supported);
    assert_eq!(header.entry_count, 3);
    assert_eq!(header.payload_len as usize, minimal_v4_payload().1.len());
}

#[test]
fn compatibility_table_matches_what_the_loader_enforces() {
    let table = CompatibilityTable::current();
    let (count, payload) = minimal_v4_payload();
    for schema in table.supported_pack_schemas {
        assert!(load(&wrap_pack(*schema, table.language_tag, count, &payload)).is_ok());
    }
    assert_eq!(table.language_tag, SUPPORTED_LANGUAGE_TAG);
}

#[test]
fn older_and_newer_schemas_are_rejected_as_unsupported() {
    let (count, payload) = minimal_v4_payload();
    for schema in [
        0,
        1,
        2,
        3,
        PACK_SCHEMA_VERSION + 1,
        PACK_SCHEMA_VERSION + 100,
        u32::MAX,
    ] {
        let pack = wrap_pack(schema, LANGUAGE_TAG, count, &payload);
        let err = load_without_panic(&pack, &format!("schema {}", schema)).unwrap_err();
        assert!(
            matches!(err, PackLoadError::UnsupportedVersion { found } if found == schema),
            "schema {}: {:?}",
            schema,
            err
        );
        assert_eq!(err.failure_class(), LoadFailureClass::UnsupportedSchema);
        let header = probe_pack_header(&pack).unwrap();
        assert_eq!(header.pack_schema_version, schema);
        assert!(!header.is_supported);
    }
}

#[test]
fn wrong_language_is_rejected_even_with_a_valid_payload() {
    let (count, payload) = minimal_v4_payload();
    for tag in [
        "en",
        "ku-Arab",
        "ku-latn",
        "KU-Latn",
        "ku-Latn-x",
        "",
        "tr-Latn",
    ] {
        let pack = wrap_pack(PACK_SCHEMA, tag, count, &payload);
        let err = load_without_panic(&pack, &format!("language {:?}", tag)).unwrap_err();
        assert!(
            matches!(&err, PackLoadError::IncompatibleLanguage { found } if found == tag),
            "language {:?}: {:?}",
            tag,
            err
        );
        assert_eq!(err.failure_class(), LoadFailureClass::WrongLanguage);
        assert!(!probe_pack_header(&pack).unwrap().is_supported);
    }
}

#[test]
fn every_truncation_of_a_valid_pack_fails_closed() {
    let pack = minimal_v4_pack();
    for len in 0..pack.len() {
        let err =
            load_without_panic(&pack[..len], &format!("truncation to {} bytes", len)).unwrap_err();
        assert!(
            matches!(
                err.failure_class(),
                LoadFailureClass::Truncated | LoadFailureClass::Corrupt
            ),
            "truncation to {} bytes: {:?}",
            len,
            err
        );
    }
    assert!(load(&pack).is_ok());
}

#[test]
fn every_single_byte_mutation_of_a_valid_pack_fails_closed() {
    let pack = minimal_v4_pack();
    let mut classes = std::collections::BTreeMap::new();
    for pos in 0..pack.len() {
        for mutation in 0..10u8 {
            let mut bytes = pack.clone();
            bytes[pos] = match mutation {
                0..=7 => bytes[pos] ^ (1 << mutation),
                8 => 0x00,
                _ => 0xFF,
            };
            if bytes == pack {
                continue;
            }
            let what = format!("byte {} mutation {}", pos, mutation);
            let result = load_without_panic(&bytes, &what);
            let err = result.expect_err(&format!("{} must not load", what));
            *classes.entry(err.failure_class()).or_insert(0usize) += 1;
        }
    }
    // Header mutations produce unsupported/wrong-language/truncated/corrupt; payload and
    // checksum mutations produce checksum mismatches. All four classes must be reachable.
    for class in [
        LoadFailureClass::Corrupt,
        LoadFailureClass::Truncated,
        LoadFailureClass::UnsupportedSchema,
        LoadFailureClass::WrongLanguage,
    ] {
        assert!(
            classes.contains_key(&class),
            "no mutation produced {:?}",
            class
        );
    }
}

#[test]
fn appended_bytes_are_rejected() {
    let mut pack = minimal_v4_pack();
    pack.push(0);
    let err = load_without_panic(&pack, "one appended byte").unwrap_err();
    assert_eq!(err.failure_class(), LoadFailureClass::Corrupt);
}

#[test]
fn corrupt_entry_count_cannot_cause_a_huge_allocation() {
    // The header is not covered by the checksum, so a corrupt count reaches the decoder.
    let (_, payload) = minimal_v4_payload();
    for count in [4u32, 1_000_000, u32::MAX / 2, u32::MAX] {
        let pack = wrap_pack(PACK_SCHEMA, LANGUAGE_TAG, count, &payload);
        let err = load_without_panic(&pack, &format!("entry count {}", count)).unwrap_err();
        assert!(
            matches!(
                err.failure_class(),
                LoadFailureClass::Corrupt | LoadFailureClass::Truncated
            ),
            "{:?}",
            err
        );
    }
}

#[test]
fn corrupt_ngram_context_counts_cannot_cause_a_huge_allocation() {
    // Bigram context count larger than the lexicon, and trigram context count larger than
    // the remaining payload can hold, both with a valid checksum.
    let mut payload = Vec::new();
    payload.extend_from_slice(&encode_entry("roj", "roj", 1, 1));
    payload.extend_from_slice(&u32::MAX.to_le_bytes());
    let pack = wrap_pack(PACK_SCHEMA, LANGUAGE_TAG, 1, &payload);
    let err = load_without_panic(&pack, "bigram count u32::MAX").unwrap_err();
    assert_eq!(err.failure_class(), LoadFailureClass::Corrupt, "{:?}", err);

    let mut payload = Vec::new();
    for i in 0..70_000u32 {
        payload.extend_from_slice(&encode_entry(&format!("w{}", i), &format!("w{}", i), 1, 1));
    }
    encode_bigram_section(&mut payload, &[]);
    payload.extend_from_slice(&(u32::MAX - 1).to_le_bytes());
    let pack = wrap_pack(PACK_SCHEMA, LANGUAGE_TAG, 70_000, &payload);
    let err = load_without_panic(&pack, "trigram count near u32::MAX").unwrap_err();
    assert_eq!(err.failure_class(), LoadFailureClass::Corrupt, "{:?}", err);
}

#[test]
fn structural_rules_are_enforced_with_valid_checksums() {
    let (count, mut payload) = minimal_v4_payload();
    // Duplicate bigram context.
    let mut dup = Vec::new();
    dup.extend_from_slice(&encode_entry("roj", "roj", 1, 1));
    dup.extend_from_slice(&encode_entry("roja", "roja", 1, 1));
    encode_bigram_section(
        &mut dup,
        &[(0, vec![(1, 1, 1_000_000)]), (0, vec![(1, 1, 1_000_000)])],
    );
    encode_trigram_section(&mut dup, &[]);
    let err =
        load_without_panic(&wrap_pack(PACK_SCHEMA, LANGUAGE_TAG, 2, &dup), "dup ctx").unwrap_err();
    assert_eq!(err.failure_class(), LoadFailureClass::Corrupt);

    // Probability above the fixed-point scale.
    let mut bad_prob = Vec::new();
    bad_prob.extend_from_slice(&encode_entry("roj", "roj", 1, 1));
    bad_prob.extend_from_slice(&encode_entry("roja", "roja", 1, 1));
    encode_bigram_section(&mut bad_prob, &[(0, vec![(1, 1, 1_000_001)])]);
    encode_trigram_section(&mut bad_prob, &[]);
    let err = load_without_panic(&wrap_pack(PACK_SCHEMA, LANGUAGE_TAG, 2, &bad_prob), "prob")
        .unwrap_err();
    assert_eq!(err.failure_class(), LoadFailureClass::Corrupt);

    // Out-of-range next index.
    let mut bad_idx = Vec::new();
    bad_idx.extend_from_slice(&encode_entry("roj", "roj", 1, 1));
    encode_bigram_section(&mut bad_idx, &[(0, vec![(9, 1, 1_000_000)])]);
    encode_trigram_section(&mut bad_idx, &[]);
    let err =
        load_without_panic(&wrap_pack(PACK_SCHEMA, LANGUAGE_TAG, 1, &bad_idx), "idx").unwrap_err();
    assert_eq!(err.failure_class(), LoadFailureClass::Corrupt);

    // Invalid UTF-8 inside an entry string, with a matching checksum.
    let pos = payload
        .windows(3)
        .position(|w| w == b"roj")
        .expect("word present");
    payload[pos] = 0xFF;
    let err = load_without_panic(
        &wrap_pack(PACK_SCHEMA, LANGUAGE_TAG, count, &payload),
        "utf8",
    )
    .unwrap_err();
    assert_eq!(err.failure_class(), LoadFailureClass::Corrupt);
}

/// The built seed pack, when present, is exercised with the same mutation sweep so that the
/// real encoder's output (all sections populated by the data builder) is covered too.
#[test]
fn built_seed_pack_survives_mutation_sweep_when_present() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/build/packs/seed/lexicon.bin");
    let Ok(pack) = std::fs::read(&path) else {
        eprintln!("skipping: {:?} not built", path);
        return;
    };
    assert!(load(&pack).is_ok());
    for pos in 0..pack.len() {
        let mut bytes = pack.clone();
        bytes[pos] ^= 0x01;
        load_without_panic(&bytes, &format!("seed byte {} flipped", pos)).unwrap_err();
    }
    for len in (0..pack.len()).step_by(7) {
        load_without_panic(&pack[..len], &format!("seed truncated to {}", len)).unwrap_err();
    }
}

/// Generates the committed fixture. Run once when the schema-4 encoding is intentionally
/// changed together with a schema bump: `cargo test -p kurmanci-engine --test
/// pack_compatibility_test -- --ignored write_v4_fixture`.
#[test]
#[ignore]
fn write_v4_fixture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pack-v4-minimal.bin");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, minimal_v4_pack()).unwrap();
}
