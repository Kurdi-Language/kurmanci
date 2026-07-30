use kurmanci_engine::Engine;

#[test]
fn test_binary_pack_loading() {
    let pack_path = std::path::Path::new("data/build/lexicon.bin");
    if !pack_path.exists() {
        eprintln!("Skipping test_binary_pack_loading: data/build/lexicon.bin not found");
        return;
    }

    let pack_bytes = std::fs::read(pack_path).expect("Failed to read binary pack file");
    let mut engine = Engine::new();

    let count = engine
        .load_binary_pack(&pack_bytes)
        .expect("Failed to load binary pack");
    assert!(count >= 32);
    assert!(engine.contains("rojbaş"));
    assert!(engine.contains("bijî"));

    let suggestions = engine.suggest("rojb", 3);
    assert!(!suggestions.is_empty());
    assert_eq!(suggestions[0].text, "rojbaş");
}
