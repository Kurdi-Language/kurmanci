use clap::{Parser, Subcommand};
use kurmanci_engine::{Engine, SuggestionKind};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "kurmanci",
    author = "Kurmancî Language Platform Contributors",
    version = "0.1.0",
    about = "Offline Kurmancî Language Engine CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Returns ranked autocomplete and spelling suggestions for a word or prefix
    Suggest {
        /// Input word or prefix (e.g. 'rojb', 'biji', 'spaz')
        query: String,

        /// Maximum number of suggestions to return
        #[arg(short, long, default_value_t = 5)]
        limit: usize,

        /// Path to compiled binary language pack (.bin)
        #[arg(short, long, default_value = "data/build/lexicon.bin")]
        pack: PathBuf,

        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Print diagnostic ranking explanation for each candidate
        #[arg(long)]
        explain: bool,
    },
    /// Predicts context-aware next word given previous word context (1 or 2 words)
    PredictNext {
        /// Previous word context(s), e.g. 'ez' or 'ez baş'
        #[arg(required = true, num_args = 1..=2)]
        words: Vec<String>,

        /// Maximum number of predictions to return
        #[arg(short, long, default_value_t = 5)]
        limit: usize,

        /// Path to compiled binary language pack (.bin)
        #[arg(short, long, default_value = "data/build/lexicon.bin")]
        pack: PathBuf,

        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Print diagnostic ranking explanation for predictions
        #[arg(long)]
        explain: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Suggest {
            query,
            limit,
            pack,
            json,
            explain,
        } => {
            let mut engine = Engine::new();

            let bytes = fs::read(&pack).unwrap_or_else(|e| {
                panic!(
                    "Failed to open binary language pack file '{:?}': {}",
                    pack, e
                )
            });

            let count = engine
                .load_binary_pack(&bytes)
                .unwrap_or_else(|e| panic!("Failed to parse binary pack '{:?}': {}", pack, e));

            if !json {
                eprintln!("[info] Loaded {} lexicon entries from {:?}", count, pack);
            }

            let start = Instant::now();
            let suggestions = engine.suggest(&query, limit);
            let elapsed = start.elapsed();

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&suggestions).unwrap_or_default()
                );
            } else if explain {
                println!(
                    "Suggestions for '{}' (explained in {:.2?}):",
                    query, elapsed
                );
                if suggestions.is_empty() {
                    println!("  (no suggestions found)");
                } else {
                    for (i, sug) in suggestions.iter().enumerate() {
                        println!("  {}. {}", i + 1, sug.text);
                        println!("     edit_cost: {}", sug.edit_cost);
                        println!("     zipf_milli: {}", sug.zipf_milli);
                        println!("     document_count: {}", sug.document_count);
                        println!("     ranking_reason: {}", sug.ranking_reason);
                    }
                }
            } else {
                println!(
                    "Suggestions for '{}' (processed in {:.2?}):",
                    query, elapsed
                );
                if suggestions.is_empty() {
                    println!("  (no suggestions found)");
                } else {
                    for (i, sug) in suggestions.iter().enumerate() {
                        let kind_str = match sug.kind {
                            SuggestionKind::Exact => "exact",
                            SuggestionKind::Completion => "completion",
                            SuggestionKind::Correction => "correction",
                            SuggestionKind::DiacriticCorrection => "diacritic_correction",
                            SuggestionKind::NextWord => "next_word",
                        };
                        println!(
                            "  {}. {:<15} [type: {:<20} score: {:.2}]",
                            i + 1,
                            sug.text,
                            kind_str,
                            sug.score
                        );
                    }
                }
            }
        }
        Commands::PredictNext {
            words,
            limit,
            pack,
            json,
            explain,
        } => {
            if words.is_empty() || words.len() > 2 {
                eprintln!("Error: predict-next requires exactly 1 or 2 positional context words");
                std::process::exit(1);
            }

            let mut engine = Engine::new();

            let bytes = fs::read(&pack).unwrap_or_else(|e| {
                panic!(
                    "Failed to open binary language pack file '{:?}': {}",
                    pack, e
                )
            });

            let count = engine
                .load_binary_pack(&bytes)
                .unwrap_or_else(|e| panic!("Failed to parse binary pack '{:?}': {}", pack, e));

            if !json {
                eprintln!("[info] Loaded {} lexicon entries from {:?}", count, pack);
            }

            let start = Instant::now();
            let context_label = words.join(" ");

            let (predictions, source_name) = if words.len() == 2 {
                let res = engine.predict_next_with_context(&words[0], &words[1], limit);
                let src = match res.source {
                    Some(kurmanci_engine::PredictionSource::Trigram) => "trigram",
                    Some(kurmanci_engine::PredictionSource::BigramBackoff) => "bigram-backoff",
                    None => "none",
                };
                (res.predictions, src.to_string())
            } else {
                let preds = engine.predict_next(&words[0], limit);
                (preds, "bigram".to_string())
            };
            let elapsed = start.elapsed();

            if json {
                let output = serde_json::json!({
                    "context": context_label,
                    "model": source_name,
                    "predictions": predictions,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output).unwrap_or_default()
                );
            } else if explain {
                println!(
                    "Predictions for '{}' [model: {}] (explained in {:.2?}):",
                    context_label, source_name, elapsed
                );
                if predictions.is_empty() {
                    println!("  (no predictions found)");
                } else {
                    for (i, pred) in predictions.iter().enumerate() {
                        let pct = pred.probability_millionths as f64 / 10000.0;
                        println!("  {}. {}", i + 1, pred.word);
                        println!("     count: {}", pred.count);
                        println!(
                            "     probability_millionths: {}",
                            pred.probability_millionths
                        );
                        println!("     percentage: {:.1}%", pct);
                    }
                }
            } else {
                println!(
                    "Predictions for '{}' [model: {}] (processed in {:.2?}):",
                    context_label, source_name, elapsed
                );
                if predictions.is_empty() {
                    println!("  (no predictions found)");
                } else {
                    for (i, pred) in predictions.iter().enumerate() {
                        let pct = pred.probability_millionths as f64 / 10000.0;
                        println!(
                            "  {}. {:<15} (probability: {:.1}%, count: {})",
                            i + 1,
                            pred.word,
                            pct,
                            pred.count
                        );
                    }
                }
            }
        }
    }
}
