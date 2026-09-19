mod commands;

use clap::{Parser, Subcommand};
use kurmanci_engine::{
    KurmanciEngine, PredictionOptions, PredictionSource, SuggestOptions, SuggestionKind,
};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "kurmanci",
    author = "Kurmancî Language Platform Contributors",
    version = env!("CARGO_PKG_VERSION"),
    about = "Offline Kurmancî Language Engine CLI: query any compiled language pack"
)]
struct Cli {
    /// Path to a compiled binary language pack (.bin)
    #[arg(short, long, global = true, default_value = "data/build/lexicon.bin")]
    pack: PathBuf,

    /// Output results as JSON
    #[arg(long, global = true)]
    json: bool,

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

        /// Print diagnostic ranking explanation for predictions
        #[arg(long)]
        explain: bool,
    },
    /// Reports whether a word is in the loaded lexicon
    Known {
        /// Word to look up (normalized before lookup)
        word: String,
    },
    /// Returns spelling corrections for a word
    Correct {
        /// Misspelled or unknown input
        input: String,

        /// Maximum number of corrections to return
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
    },
    /// Returns prefix completions
    Complete {
        /// Prefix to complete
        prefix: String,

        /// Maximum number of completions to return
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
    },
    /// Predicts the next word after a 1- or 2-word context
    Predict {
        /// Context words, e.g. 'ez' or 'navê te'
        #[arg(required = true, num_args = 1..=2)]
        words: Vec<String>,

        /// Maximum number of predictions to return
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
    },
    /// Interactive shell: one command per line (known, suggest, correct, complete, predict)
    Interactive {
        /// Default result limit for the session
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
    },
}

fn load_engine(pack: &PathBuf) -> KurmanciEngine {
    KurmanciEngine::from_pack_file(pack).unwrap_or_else(|e| {
        eprintln!("Error: failed to load binary pack '{:?}': {}", pack, e);
        std::process::exit(1);
    })
}

fn print_loaded_info(engine: &KurmanciEngine, pack: &PathBuf) {
    let info = engine.pack_info();
    eprintln!(
        "[info] Loaded {} lexicon entries (pack tag: {}, format v{}) from {:?}",
        info.entry_count, info.language_tag, info.format_version, pack
    );
}

fn main() {
    let cli = Cli::parse();
    let pack = cli.pack;
    let json = cli.json;

    match cli.command {
        Commands::Suggest {
            query,
            limit,
            explain,
        } => {
            let engine = load_engine(&pack);
            if !json {
                print_loaded_info(&engine, &pack);
            }

            let start = Instant::now();
            let suggestions = engine.suggest(&query, SuggestOptions { limit });
            let elapsed = start.elapsed();

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&suggestions).unwrap_or_default()
                );
            } else {
                println!(
                    "Suggestions for '{}' ({} in {:.2?}):",
                    query,
                    if explain { "explained" } else { "processed" },
                    elapsed
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
                            "  {}. {:<15} [type: {:<20} edit_cost: {}]",
                            i + 1,
                            sug.text,
                            kind_str,
                            sug.edit_cost
                        );
                    }
                }
            }
        }
        Commands::PredictNext {
            words,
            limit,
            explain,
        } => {
            if words.is_empty() || words.len() > 2 {
                eprintln!("Error: predict-next requires exactly 1 or 2 positional context words");
                std::process::exit(1);
            }

            let engine = load_engine(&pack);
            if !json {
                print_loaded_info(&engine, &pack);
            }

            let start = Instant::now();
            let context_label = words.join(" ");
            let ctx_refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();

            let predictions = engine.predict_next(&ctx_refs, PredictionOptions { limit });
            let elapsed = start.elapsed();

            let source_name = predictions
                .first()
                .map(|p| match p.source {
                    PredictionSource::Trigram => "trigram",
                    PredictionSource::BigramBackoff => "bigram-backoff",
                    PredictionSource::Bigram => "bigram",
                    PredictionSource::None => "none",
                })
                .unwrap_or("none");

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
                        println!("  {}. {}", i + 1, pred.text);
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
                            pred.text,
                            pct,
                            pred.count
                        );
                    }
                }
            }
        }
        Commands::Known { word } => {
            let engine = load_engine(&pack);
            print!(
                "{}",
                commands::render(&commands::known(&engine, &word), json)
            );
            if json {
                println!();
            }
        }
        Commands::Correct { input, limit } => {
            let engine = load_engine(&pack);
            print!(
                "{}",
                commands::render(&commands::correct(&engine, &input, limit), json)
            );
            if json {
                println!();
            }
        }
        Commands::Complete { prefix, limit } => {
            let engine = load_engine(&pack);
            print!(
                "{}",
                commands::render(&commands::complete(&engine, &prefix, limit), json)
            );
            if json {
                println!();
            }
        }
        Commands::Predict { words, limit } => {
            let engine = load_engine(&pack);
            print!(
                "{}",
                commands::render(&commands::predict(&engine, &words, limit), json)
            );
            if json {
                println!();
            }
        }
        Commands::Interactive { limit } => {
            let engine = load_engine(&pack);
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            if let Err(e) =
                commands::run_interactive(&engine, limit, json, stdin.lock(), stdout.lock())
            {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
