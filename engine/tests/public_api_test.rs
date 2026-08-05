use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, EngineError, KurmanciEngine, PackInfo, PackLoadError,
    PredictionOptions, SuggestOptions,
};
use std::path::PathBuf;

fn get_pack_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/build/packs")
        .join(name)
        .join("lexicon.bin")
}

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn test_engine_thread_safety_send_sync() {
    assert_send_sync::<KurmanciEngine>();
    assert_send_sync::<PackInfo>();
    assert_send_sync::<EngineError>();
}

#[test]
fn test_public_api_pack_loading_and_metadata() {
    let seed_path = get_pack_path("seed");
    let rev_path = get_pack_path("reviewed");
    let exp_path = get_pack_path("experimental-full");

    let seed_engine = KurmanciEngine::from_pack_file(&seed_path)
        .expect("Failed to load seed pack via public API");
    assert_eq!(seed_engine.len(), 33);
    assert!(!seed_engine.is_empty());
    assert_eq!(seed_engine.pack_info().language_tag, "ku-Latn");
    assert_eq!(seed_engine.pack_info().format_version, 4);
    assert_eq!(seed_engine.pack_info().entry_count, 33);

    let rev_engine = KurmanciEngine::from_pack_file(&rev_path)
        .expect("Failed to load reviewed pack via public API");
    assert_eq!(rev_engine.len(), 35);
    assert_eq!(rev_engine.pack_info().entry_count, 35);

    let exp_engine = KurmanciEngine::from_pack_file(&exp_path)
        .expect("Failed to load experimental-full pack via public API");
    assert_eq!(exp_engine.len(), 41496);
    assert_eq!(exp_engine.pack_info().entry_count, 41496);
}

#[test]
fn test_public_api_malformed_pack_rejection() {
    // 1. Short bytes
    let res1 = KurmanciEngine::from_pack_bytes(&[1, 2, 3]);
    assert!(matches!(
        res1,
        Err(EngineError::PackLoad(PackLoadError::TooShort(3)))
    ));

    // 2. Invalid magic bytes
    let mut bad_magic = vec![0u8; 32];
    bad_magic[0..4].copy_from_slice(b"BADM");
    let res2 = KurmanciEngine::from_pack_bytes(&bad_magic);
    assert!(matches!(
        res2,
        Err(EngineError::PackLoad(PackLoadError::InvalidMagicBytes))
    ));

    // 3. Unsupported format version (using valid magic bytes b"KRM1")
    let mut bad_version = vec![0u8; 32];
    bad_version[0..4].copy_from_slice(b"KRM1");
    bad_version[4..8].copy_from_slice(&99u32.to_le_bytes());
    let res3 = KurmanciEngine::from_pack_bytes(&bad_version);
    assert!(matches!(
        res3,
        Err(EngineError::PackLoad(PackLoadError::UnsupportedVersion {
            found: 99
        }))
    ));

    // 4. Missing pack file
    let missing_path = PathBuf::from("non_existent_pack.bin");
    let res4 = KurmanciEngine::from_pack_file(&missing_path);
    assert!(matches!(res4, Err(EngineError::Io(_))));
}

#[test]
fn test_public_api_is_known_word_and_nfc_normalization() {
    let engine =
        KurmanciEngine::from_pack_file(get_pack_path("seed")).expect("Failed to load seed pack");

    // "welat" is in seed
    assert!(engine.is_known_word("welat"));
    assert!(engine.is_known_word("WELAT"));
    assert!(engine.is_known_word("Welat"));

    // NFD decomposed unicode string for "kurmancî" -> NFC normalized match
    let nfd_kurmanci = "kurmanci\u{0302}";
    assert!(engine.is_known_word(nfd_kurmanci));

    // Unknown word
    assert!(!engine.is_known_word("nonexistentword123"));
}

#[test]
fn test_public_api_correct_complete_suggest_infallible() {
    let engine = KurmanciEngine::from_pack_file(get_pack_path("reviewed"))
        .expect("Failed to load reviewed pack");

    // Correct
    let corrections = engine.correct("spaz", CorrectionOptions { limit: 5 });
    assert!(!corrections.is_empty());
    assert!(corrections.iter().any(|c| c.text == "spas"));

    // Complete
    let completions = engine.complete("roj", CompletionOptions { limit: 5 });
    assert!(!completions.is_empty());
    assert!(completions
        .iter()
        .any(|c| c.text == "roja" || c.text == "rojbaş"));

    // Suggest
    let suggestions = engine.suggest("şeq", SuggestOptions { limit: 5 });
    assert!(!suggestions.is_empty());
    assert_eq!(suggestions[0].text, "şeq");
}

#[test]
fn test_public_api_limit_clamping() {
    let engine =
        KurmanciEngine::from_pack_file(get_pack_path("seed")).expect("Failed to load seed pack");

    // Limit 0 returns empty
    let zero_res = engine.suggest("roj", SuggestOptions { limit: 0 });
    assert!(zero_res.is_empty());

    // Limit > 50 is clamped to 50
    let large_res = engine.complete("a", CompletionOptions { limit: 100 });
    assert!(large_res.len() <= 50);
}

#[test]
fn test_public_api_predict_next_context_rules() {
    let pack_path = PathBuf::from("data/build/lexicon.bin");
    if !pack_path.exists() {
        eprintln!(
            "Skipping test_public_api_predict_next_context_rules: data/build/lexicon.bin not found"
        );
        return;
    }

    let engine = KurmanciEngine::from_pack_file(&pack_path).expect("Failed to load compiled pack");

    // 0 context words -> empty
    let p0 = engine.predict_next(&[], PredictionOptions { limit: 5 });
    assert!(p0.is_empty());

    // 1 context word -> bigram prediction
    let p1 = engine.predict_next(&["ez"], PredictionOptions { limit: 5 });
    assert!(!p1.is_empty());

    // 2 context words -> trigram with bigram backoff
    let p2 = engine.predict_next(&["ez", "diçim"], PredictionOptions { limit: 5 });
    assert!(!p2.is_empty());

    // 3 context words -> uses final two words ("ez", "diçim")
    let p3 = engine.predict_next(&["min", "ez", "diçim"], PredictionOptions { limit: 5 });
    assert_eq!(p2, p3);
}
