use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, PredictionOptions, SuggestOptions,
};
use std::env;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = env::args().collect();
    let pack_path = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        PathBuf::from("data/build/packs/seed/lexicon.bin")
    };

    println!("Loading Kurmancî language pack from {:?}...", pack_path);

    let engine = match KurmanciEngine::from_pack_file(&pack_path) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Error loading language pack: {}", err);
            std::process::exit(1);
        }
    };

    let info = engine.pack_info();
    println!(
        "Successfully loaded {} entries (tag: {}, format v{})",
        info.entry_count, info.language_tag, info.format_version
    );

    // 1. Word check
    let test_word = "welat";
    println!(
        "\n1. Is '{}' a known word? {}",
        test_word,
        engine.is_known_word(test_word)
    );

    // 2. Spelling correction
    let typo = "spaz";
    let corrections = engine.correct(typo, CorrectionOptions { limit: 3 });
    println!("\n2. Corrections for '{}':", typo);
    for (i, c) in corrections.iter().enumerate() {
        println!("   {}. {} (edit_cost: {})", i + 1, c.text, c.edit_cost);
    }

    // 3. Prefix completion
    let prefix = "roj";
    let completions = engine.complete(prefix, CompletionOptions { limit: 3 });
    println!("\n3. Prefix completions for '{}':", prefix);
    for (i, c) in completions.iter().enumerate() {
        println!("   {}. {}", i + 1, c.text);
    }

    // 4. Combined suggestion
    let query = "biji";
    let suggestions = engine.suggest(query, SuggestOptions { limit: 3 });
    println!("\n4. Combined suggestions for '{}':", query);
    for (i, s) in suggestions.iter().enumerate() {
        println!("   {}. {} [{:?}]", i + 1, s.text, s.kind);
    }

    // 5. Next-word prediction
    let context = ["ez", "diçim"];
    let predictions = engine.predict_next(&context, PredictionOptions { limit: 3 });
    println!("\n5. Predictions following {:?}:", context);
    for (i, p) in predictions.iter().enumerate() {
        let pct = p.probability_millionths as f64 / 10000.0;
        println!(
            "   {}. {} (prob: {:.1}%, source: {:?})",
            i + 1,
            p.text,
            pct,
            p.source
        );
    }
}
