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
    }
}
