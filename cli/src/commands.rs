//! Command handlers of the QA CLI: thin adapters over the public `KurmanciEngine` API.
//!
//! Every command produces an `Output` value from the engine's own results; text and JSON
//! renderings are derived from that value, and interactive mode dispatches to the same
//! handlers. Nothing here re-implements suggestion, correction, completion or prediction,
//! and nothing here reads provenance or review data: the CLI only shows what a loaded pack
//! answers.

use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, PredictionOptions, PredictionSource,
    SuggestOptions, SuggestionKind, SuggestionResult,
};
use serde::Serialize;
use std::io::{BufRead, Write};

/// One suggestion, correction or completion candidate as the engine returned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuggestionOutput {
    pub text: String,
    pub kind: &'static str,
    pub edit_cost: u32,
}

/// One next-word prediction as the engine returned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PredictionOutput {
    pub text: String,
    pub source: &'static str,
    pub count: u64,
    pub probability_millionths: u32,
}

/// Result of one command, in the shape the JSON mode emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Output {
    Known {
        input: String,
        known: bool,
    },
    Suggest {
        input: String,
        results: Vec<SuggestionOutput>,
    },
    Correct {
        input: String,
        results: Vec<SuggestionOutput>,
    },
    Complete {
        input: String,
        results: Vec<SuggestionOutput>,
    },
    Predict {
        context: Vec<String>,
        results: Vec<PredictionOutput>,
    },
}

pub fn kind_name(kind: &SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Exact => "exact",
        SuggestionKind::Completion => "completion",
        SuggestionKind::Correction => "correction",
        SuggestionKind::DiacriticCorrection => "diacriticCorrection",
        SuggestionKind::NextWord => "nextWord",
    }
}

pub fn source_name(source: PredictionSource) -> &'static str {
    match source {
        PredictionSource::Trigram => "trigram",
        PredictionSource::BigramBackoff => "bigramBackoff",
        PredictionSource::Bigram => "bigram",
        PredictionSource::None => "none",
    }
}

fn suggestions(results: Vec<SuggestionResult>) -> Vec<SuggestionOutput> {
    results
        .into_iter()
        .map(|s| SuggestionOutput {
            text: s.text,
            kind: kind_name(&s.kind),
            edit_cost: s.edit_cost,
        })
        .collect()
}

pub fn known(engine: &KurmanciEngine, word: &str) -> Output {
    Output::Known {
        input: word.to_string(),
        known: engine.is_known_word(word),
    }
}

pub fn suggest(engine: &KurmanciEngine, input: &str, limit: usize) -> Output {
    Output::Suggest {
        input: input.to_string(),
        results: suggestions(engine.suggest(input, SuggestOptions { limit })),
    }
}

pub fn correct(engine: &KurmanciEngine, input: &str, limit: usize) -> Output {
    Output::Correct {
        input: input.to_string(),
        results: suggestions(engine.correct(input, CorrectionOptions { limit })),
    }
}

pub fn complete(engine: &KurmanciEngine, prefix: &str, limit: usize) -> Output {
    Output::Complete {
        input: prefix.to_string(),
        results: suggestions(engine.complete(prefix, CompletionOptions { limit })),
    }
}

pub fn predict(engine: &KurmanciEngine, context: &[String], limit: usize) -> Output {
    let refs: Vec<&str> = context.iter().map(|s| s.as_str()).collect();
    Output::Predict {
        context: context.to_vec(),
        results: engine
            .predict_next(&refs, PredictionOptions { limit })
            .into_iter()
            .map(|p| PredictionOutput {
                text: p.text,
                source: source_name(p.source),
                count: p.count,
                probability_millionths: p.probability_millionths,
            })
            .collect(),
    }
}

/// Human-readable rendering of an output.
pub fn render_text(output: &Output) -> String {
    let mut s = String::new();
    match output {
        Output::Known { input, known } => {
            s.push_str(&format!(
                "{}: {}\n",
                input,
                if *known { "known" } else { "unknown" }
            ));
        }
        Output::Suggest { input, results }
        | Output::Correct { input, results }
        | Output::Complete { input, results } => {
            let label = match output {
                Output::Suggest { .. } => "Suggestions",
                Output::Correct { .. } => "Corrections",
                _ => "Completions",
            };
            s.push_str(&format!("{} for '{}':\n", label, input));
            if results.is_empty() {
                s.push_str("  (none)\n");
            }
            for (i, r) in results.iter().enumerate() {
                s.push_str(&format!(
                    "  {}. {:<16} [{:<20} edit_cost: {}]\n",
                    i + 1,
                    r.text,
                    r.kind,
                    r.edit_cost
                ));
            }
        }
        Output::Predict { context, results } => {
            s.push_str(&format!("Predictions after '{}':\n", context.join(" ")));
            if results.is_empty() {
                s.push_str("  (none)\n");
            }
            for (i, r) in results.iter().enumerate() {
                s.push_str(&format!(
                    "  {}. {:<16} [{:<14} count: {:<8} probability: {:.1}%]\n",
                    i + 1,
                    r.text,
                    r.source,
                    r.count,
                    r.probability_millionths as f64 / 10_000.0
                ));
            }
        }
    }
    s
}

/// Deterministic JSON rendering of an output (pretty-printed, stable key order).
pub fn render_json(output: &Output) -> String {
    serde_json::to_string_pretty(output).expect("outputs are always serializable")
}

pub fn render(output: &Output, json: bool) -> String {
    if json {
        render_json(output)
    } else {
        render_text(output)
    }
}

const INTERACTIVE_HELP: &str = "commands:\n  \
    known WORD            is WORD in the lexicon\n  \
    suggest WORD          ranked suggestions (exact, completion, correction)\n  \
    correct WORD          spelling corrections\n  \
    complete PREFIX       prefix completions\n  \
    predict WORD [WORD]   next-word predictions for a 1- or 2-word context\n  \
    limit N               set the result limit (current value shown by 'limit')\n  \
    json on|off           switch between JSON and text output\n  \
    help                  this text\n  \
    quit                  exit\n";

/// Runs the interactive shell: one command per line, dispatched to the handlers above.
/// Returns the number of commands executed. Unknown commands and usage errors are reported
/// on `out` and do not end the session; end of input or `quit` does.
pub fn run_interactive<R: BufRead, W: Write>(
    engine: &KurmanciEngine,
    mut limit: usize,
    mut json: bool,
    input: R,
    mut out: W,
) -> std::io::Result<usize> {
    let mut executed = 0usize;
    writeln!(
        out,
        "kurmanci interactive ({} entries loaded). Type 'help' for commands, 'quit' to exit.",
        engine.len()
    )?;
    for line in input.lines() {
        let line = line?;
        let mut parts = line.split_whitespace();
        let Some(command) = parts.next() else {
            continue;
        };
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();
        let output = match command {
            "quit" | "exit" => break,
            "help" | "?" => {
                out.write_all(INTERACTIVE_HELP.as_bytes())?;
                continue;
            }
            "limit" => {
                match args.first().map(|a| a.parse::<usize>()) {
                    Some(Ok(n)) => limit = n,
                    Some(Err(_)) => writeln!(out, "error: limit expects a number")?,
                    None => {}
                }
                writeln!(out, "limit: {}", limit)?;
                continue;
            }
            "json" => {
                match args.first().map(|s| s.as_str()) {
                    Some("on") => json = true,
                    Some("off") => json = false,
                    _ => writeln!(out, "error: json expects 'on' or 'off'")?,
                }
                writeln!(out, "json: {}", if json { "on" } else { "off" })?;
                continue;
            }
            "known" | "suggest" | "correct" | "complete" => {
                let Some(word) = args.first() else {
                    writeln!(out, "error: {} expects one word", command)?;
                    continue;
                };
                match command {
                    "known" => known(engine, word),
                    "suggest" => suggest(engine, word, limit),
                    "correct" => correct(engine, word, limit),
                    _ => complete(engine, word, limit),
                }
            }
            "predict" => {
                if args.is_empty() || args.len() > 2 {
                    writeln!(out, "error: predict expects one or two context words")?;
                    continue;
                }
                predict(engine, &args, limit)
            }
            other => {
                writeln!(
                    out,
                    "error: unknown command '{}' (type 'help' for the list)",
                    other
                )?;
                continue;
            }
        };
        executed += 1;
        out.write_all(render(&output, json).as_bytes())?;
        if json {
            out.write_all(b"\n")?;
        }
        out.flush()?;
    }
    Ok(executed)
}
