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
    assert!(rev_engine.len() >= seed_engine.len());
    assert_eq!(rev_engine.len(), rev_engine.pack_info().entry_count);

    let exp_engine = KurmanciEngine::from_pack_file(&exp_path)
        .expect("Failed to load experimental-full pack via public API");
    assert!(exp_engine.len() > rev_engine.len());
    assert_eq!(exp_engine.len(), exp_engine.pack_info().entry_count);
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

    // 4. Incompatible language tag
    let mut bad_lang = vec![0u8; 32];
    bad_lang[0..4].copy_from_slice(b"KRM1");
    bad_lang[4..8].copy_from_slice(&4u32.to_le_bytes());
    bad_lang[8..10].copy_from_slice(&2u16.to_le_bytes());
    bad_lang[10..12].copy_from_slice(b"en");
    let res_lang = KurmanciEngine::from_pack_bytes(&bad_lang);
    assert!(matches!(
        res_lang,
        Err(EngineError::PackLoad(PackLoadError::IncompatibleLanguage {
            found
        })) if found == "en"
    ));

    // 5. Checksum mismatch on corrupted seed pack bytes
    let seed_path = get_pack_path("seed");
    let mut corrupted = std::fs::read(&seed_path).expect("seed pack must exist");
    if let Some(last) = corrupted.last_mut() {
        *last ^= 0xFF;
    }
    let res_chk = KurmanciEngine::from_pack_bytes(&corrupted);
    assert!(matches!(
        res_chk,
        Err(EngineError::PackLoad(PackLoadError::ChecksumMismatch))
    ));

    // 6. Truncated payload
    let mut trunc_pack = vec![0u8; 32];
    trunc_pack[0..4].copy_from_slice(b"KRM1");
    trunc_pack[4..8].copy_from_slice(&4u32.to_le_bytes());
    trunc_pack[8..10].copy_from_slice(&7u16.to_le_bytes());
    trunc_pack[10..17].copy_from_slice(b"ku-Latn");
    let res_trunc = KurmanciEngine::from_pack_bytes(&trunc_pack);
    assert!(matches!(
        res_trunc,
        Err(EngineError::PackLoad(PackLoadError::TruncatedPayload))
            | Err(EngineError::PackLoad(PackLoadError::InvalidPayload { .. }))
    ));

    // 7. Missing pack file
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
fn test_public_api_unicode_nfc_nfd_equivalence() {
    let engine =
        KurmanciEngine::from_pack_file(get_pack_path("seed")).expect("Failed to load seed pack");

    let nfc_prefix = "rojbaş";
    let nfd_prefix = "rojbas\u{0327}";
    assert_eq!(
        engine.complete(nfc_prefix, CompletionOptions::default()),
        engine.complete(nfd_prefix, CompletionOptions::default())
    );

    let nfc_suggest = "şeq";
    let nfd_suggest = "s\u{0327}eq";
    assert_eq!(
        engine.suggest(nfc_suggest, SuggestOptions::default()),
        engine.suggest(nfd_suggest, SuggestOptions::default())
    );

    let nfc_correct = "bijî";
    let nfd_correct = "biji\u{0302}";
    assert_eq!(
        engine.correct(nfc_correct, CorrectionOptions::default()),
        engine.correct(nfd_correct, CorrectionOptions::default())
    );

    let nfc_ctx = ["ez", "diçim"];
    let nfd_ctx = ["ez", "dic\u{0327}im"];
    assert_eq!(
        engine.predict_next(&nfc_ctx, PredictionOptions::default()),
        engine.predict_next(&nfd_ctx, PredictionOptions::default())
    );
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
    let seed_path = get_pack_path("seed");
    let engine = KurmanciEngine::from_pack_file(&seed_path)
        .expect("seed pack must exist; CI builds controlled packs before tests");

    // 0 context words -> empty
    let p0 = engine.predict_next(&[], PredictionOptions { limit: 5 });
    assert!(p0.is_empty());

    // 1 context word -> bigram prediction (empty in model_profile=none, but query executes infallibly)
    let p1 = engine.predict_next(&["ez"], PredictionOptions { limit: 5 });
    assert_eq!(p1, Vec::new());

    // 2 context words -> trigram/backoff prediction
    let p2 = engine.predict_next(&["ez", "diçim"], PredictionOptions { limit: 5 });
    assert_eq!(p2, Vec::new());

    // 3 context words -> uses final two words ("ez", "diçim")
    let p3 = engine.predict_next(&["min", "ez", "diçim"], PredictionOptions { limit: 5 });
    assert_eq!(p2, p3);
}
