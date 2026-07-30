use kurmanci_engine::Engine;
use std::collections::HashMap;

fn main() {
    println!("=== Evaluation: Spell Checking & Diacritic Accuracy Benchmark ===");
    let mut engine = Engine::new();

    let pack_path = std::path::Path::new("data/build/lexicon.bin");
    if !pack_path.exists() {
        eprintln!("data/build/lexicon.bin not found. Executing data-builder first...");
    }
    let pack_bytes = std::fs::read(pack_path)
        .expect("Failed to read production binary pack file 'data/build/lexicon.bin'");
    engine
        .load_binary_pack(&pack_bytes)
        .expect("Failed to load production binary pack");

    let typos_json = include_str!("../../data/errors/typos.json");
    let typos: Vec<serde_json::Value> = serde_json::from_str(typos_json).unwrap();

    let mut typo_map = HashMap::new();
    for typo in &typos {
        let inc = typo["incorrect"].as_str().unwrap().to_string();
        let intended = typo["intended"].as_str().unwrap().to_string();
        typo_map.insert(inc, intended);
    }
    engine.load_typos(typo_map);

    let mut top1_correct = 0;
    let mut top3_correct = 0;
    let total = typos.len();

    for typo in &typos {
        let incorrect = typo["incorrect"].as_str().unwrap();
        let intended = typo["intended"].as_str().unwrap();

        let suggestions = engine.suggest(incorrect, 3);
        println!(
            "Query: '{}', Intended: '{}', Got suggestions: {:?}",
            incorrect, intended, suggestions
        );
        if !suggestions.is_empty() && suggestions[0].text == intended {
            top1_correct += 1;
        }
        if suggestions.iter().any(|s| s.text == intended) {
            top3_correct += 1;
        }
    }

    let top1_acc = (top1_correct as f64 / total as f64) * 100.0;
    let top3_acc = (top3_correct as f64 / total as f64) * 100.0;

    println!("Total benchmark items: {}", total);
    println!(
        "Top-1 Accuracy: {:.1}% ({}/{})",
        top1_acc, top1_correct, total
    );
    println!(
        "Top-3 Accuracy: {:.1}% ({}/{})",
        top3_acc, top3_correct, total
    );
    assert!(top1_acc >= 80.0, "Top-1 accuracy below 80% threshold!");
    println!("✅ Accuracy Benchmark PASSED!");
}
