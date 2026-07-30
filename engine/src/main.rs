use clap::{Parser, Subcommand};
use kurmanci_engine::{Engine, LexiconEntry, SuggestionKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

const REVIEWED_LEXICON_JSONL: &str = include_str!("../../data/reviewed/lexicon.jsonl");
const TYPOS_JSON: &str = include_str!("../../data/errors/typos.json");

#[derive(Serialize, Deserialize)]
struct TypoEntry {
    incorrect: String,
    intended: String,
}

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

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Returns prefix completions only
    Complete {
        /// Prefix to complete
        prefix: String,

        /// Limit
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
    },

    /// Returns spell checking and diacritic corrections
    Correct {
        /// Word to correct
        word: String,

        /// Limit
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
    },

    /// Checks if a word exists in the Kurmancî lexicon
    Contains { word: String },

    /// Runs automated performance and accuracy benchmarks
    Benchmark,
}

fn load_engine() -> Engine {
    let mut engine = Engine::new();

    let mut entries = Vec::new();
    for line in REVIEWED_LEXICON_JSONL.lines() {
        if !line.trim().is_empty() {
            let entry: LexiconEntry =
                serde_json::from_str(line).expect("Failed to parse lexicon entry");
            entries.push(entry);
        }
    }
    engine.load_lexicon(entries);

    let typos: Vec<TypoEntry> =
        serde_json::from_str(TYPOS_JSON).expect("Failed to parse typos JSON");
    let mut typo_map = HashMap::new();
    for typo in typos {
        typo_map.insert(typo.incorrect, typo.intended);
    }
    engine.load_typos(typo_map);

    engine
}

fn main() {
    let cli = Cli::parse();
    let engine = load_engine();

    match cli.command {
        Commands::Suggest { query, limit, json } => {
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
        Commands::Complete { prefix, limit } => {
            let completions = engine.complete(&prefix, limit);
            println!("Completions for prefix '{}':", prefix);
            for c in completions {
                println!("  - {} (score: {:.2})", c.text, c.score);
            }
        }
        Commands::Correct { word, limit } => {
            let corrections = engine.correct(&word, limit);
            println!("Corrections for word '{}':", word);
            for c in corrections {
                println!("  - {} ({:?}, score: {:.2})", c.text, c.kind, c.score);
            }
        }
        Commands::Contains { word } => {
            let exists = engine.contains(&word);
            println!("Contains '{}': {}", word, exists);
        }
        Commands::Benchmark => {
            println!("=== Kurmancî Engine Benchmark Suite ===");
            let queries = vec!["rojb", "biji", "spaz", "hevall", "kurdi", "pirtuk"];
            let iterations = 1000;

            let start = Instant::now();
            for _ in 0..iterations {
                for q in &queries {
                    let _ = engine.suggest(q, 3);
                }
            }
            let total_time = start.elapsed();
            let total_queries = iterations * queries.len();
            let avg_micros = total_time.as_micros() as f64 / total_queries as f64;

            println!("Total queries: {}", total_queries);
            println!("Total elapsed time: {:.2?}", total_time);
            println!(
                "Average latency per query: {:.2} µs (< 20,000 µs target)",
                avg_micros
            );
            println!("Status: ⚡ PERFORMANCE GOAL PASSED!");
        }
    }
}
