use kurmanci_engine::{Engine, LexiconEntry, SuggestionKind};
use std::collections::HashMap;

fn setup_test_engine() -> Engine {
    let mut engine = Engine::new();
    let lexicon_jsonl = include_str!("../../data/reviewed/lexicon.jsonl");
    let mut entries = Vec::new();
    for line in lexicon_jsonl.lines() {
        if !line.trim().is_empty() {
            let entry: LexiconEntry = serde_json::from_str(line).unwrap();
            entries.push(entry);
        }
    }
    engine.load_lexicon(entries);

    let mut typos = HashMap::new();
    typos.insert("rojbas".to_string(), "rojbaş".to_string());
    typos.insert("biji".to_string(), "bijî".to_string());
    typos.insert("spaz".to_string(), "spas".to_string());
    typos.insert("hevall".to_string(), "heval".to_string());
    engine.load_typos(typos);

    engine
}

#[test]
fn test_exact_contains() {
    let engine = setup_test_engine();
    assert!(engine.contains("rojbaş"));
    assert!(engine.contains("bijî"));
    assert!(engine.contains("spas"));
    assert!(!engine.contains("nonexistentword123"));
}

#[test]
fn test_prefix_completion() {
    let engine = setup_test_engine();
    let suggestions = engine.suggest("rojb", 5);

    assert!(!suggestions.is_empty());
    assert_eq!(suggestions[0].text, "rojbaş");
    assert_eq!(suggestions[0].kind, SuggestionKind::Completion);
}

#[test]
fn test_diacritic_restoration() {
    let engine = setup_test_engine();

    let suggestions_biji = engine.suggest("biji", 5);
    assert!(!suggestions_biji.is_empty());
    assert_eq!(suggestions_biji[0].text, "bijî");

    let suggestions_rojbas = engine.suggest("rojbas", 5);
    assert!(!suggestions_rojbas.is_empty());
    assert_eq!(suggestions_rojbas[0].text, "rojbaş");
}

#[test]
fn test_typo_correction() {
    let engine = setup_test_engine();

    let suggestions_spaz = engine.suggest("spaz", 5);
    assert!(!suggestions_spaz.is_empty());
    assert!(suggestions_spaz.iter().any(|s| s.text == "spas"));

    let suggestions_hevall = engine.suggest("hevall", 5);
    assert!(!suggestions_hevall.is_empty());
    assert!(suggestions_hevall.iter().any(|s| s.text == "heval"));
}

fn make_entry(word: &str, norm: &str) -> LexiconEntry {
    LexiconEntry {
        word: word.to_string(),
        normalized: norm.to_string(),
        lemma: word.to_string(),
        part_of_speech: "noun_fem".to_string(),
        frequency: 1,
        regions: vec![],
        status: "canonical".to_string(),
        sources: vec![],
        frequency_metadata: kurmanci_engine::FrequencyMetadata::default(),
    }
}

#[test]
fn test_query_case_preserving_suggestion_output() {
    let mut engine = Engine::new();
    let entries = vec![
        make_entry("Kurdî", "kurdî"),
        make_entry("Hêrs", "hêrs"),
        make_entry("Kurmancî", "kurmancî"),
        make_entry("pirtûk", "pirtûk"),
        make_entry("USA", "usa"),
        make_entry("McDonald", "mcdonald"),
    ];
    engine.load_lexicon(entries);

    // 1. Lowercase query + title-case candidate -> lowercase output
    let s_kurdi = engine.suggest("kurdi", 5);
    assert!(!s_kurdi.is_empty());
    assert_eq!(s_kurdi[0].text, "kurdî");

    let s_hers = engine.suggest("hers", 5);
    assert!(!s_hers.is_empty());
    assert_eq!(s_hers[0].text, "hêrs");

    let s_kûrmanci = engine.suggest("kûrmancî", 5);
    assert!(!s_kûrmanci.is_empty());
    assert_eq!(s_kûrmanci[0].text, "kurmancî");

    // 2. Lowercase query + already-lowercase candidate unchanged
    let s_pirtuk = engine.suggest("pirtuk", 5);
    assert!(!s_pirtuk.is_empty());
    assert_eq!(s_pirtuk[0].text, "pirtûk");

    // 3. Lowercase query + ALL-CAPS candidate unchanged
    let s_usa = engine.suggest("usa", 5);
    assert!(!s_usa.is_empty());
    assert_eq!(s_usa[0].text, "USA");

    // 4. Lowercase query + mixed-case candidate unchanged
    let s_mc = engine.suggest("mcdonald", 5);
    assert!(!s_mc.is_empty());
    assert_eq!(s_mc[0].text, "McDonald");

    // 5. Title-case query unchanged
    let s_title = engine.suggest("Kurdi", 5);
    assert!(!s_title.is_empty());
    assert_eq!(s_title[0].text, "Kurdî");

    // 6. Uppercase query unchanged
    let s_upper = engine.suggest("KURDI", 5);
    assert!(!s_upper.is_empty());
    assert_eq!(s_upper[0].text, "Kurdî");
}
