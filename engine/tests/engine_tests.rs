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
