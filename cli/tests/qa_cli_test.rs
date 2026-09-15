//! The QA CLI is an adapter over the public engine API: every command's JSON output must
//! equal what the engine returns directly for the same pack and input, text mode must be
//! readable, load and usage failures must exit non-zero with a message on stderr, and the
//! interactive shell must dispatch to the same handlers.

mod common;

use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, PredictionOptions, PredictionSource,
    SuggestOptions, SuggestionKind,
};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn fixture_pack() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lexicon.bin");
    std::fs::write(&path, common::minimal_v4_pack()).unwrap();
    (dir, path)
}

fn run(args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kurmanci"));
    cmd.args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn kind_name(kind: &SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Exact => "exact",
        SuggestionKind::Completion => "completion",
        SuggestionKind::Correction => "correction",
        SuggestionKind::DiacriticCorrection => "diacriticCorrection",
        SuggestionKind::NextWord => "nextWord",
    }
}

fn source_name(source: PredictionSource) -> &'static str {
    match source {
        PredictionSource::Trigram => "trigram",
        PredictionSource::BigramBackoff => "bigramBackoff",
        PredictionSource::Bigram => "bigram",
        PredictionSource::None => "none",
    }
}

fn expected_suggestions(results: Vec<kurmanci_engine::SuggestionResult>) -> Value {
    Value::Array(
        results
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "text": s.text,
                    "kind": kind_name(&s.kind),
                    "edit_cost": s.edit_cost,
                })
            })
            .collect(),
    )
}

#[test]
fn json_output_equals_direct_engine_results() {
    let (_dir, pack) = fixture_pack();
    let engine = KurmanciEngine::from_pack_file(&pack).unwrap();
    let pack_str = pack.to_str().unwrap();

    // known: exact, case-folded Kurmancî input, and unknown
    for (word, expected) in [
        ("roj", true),
        ("Baş", true),
        ("rojb", false),
        ("xyz", false),
    ] {
        let (code, out, _) = run(&["--pack", pack_str, "--json", "known", word], None);
        assert_eq!(code, 0);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["command"], "known");
        assert_eq!(v["input"], word);
        assert_eq!(v["known"], expected);
        assert_eq!(v["known"], engine.is_known_word(word));
    }

    // suggest / correct / complete: results must equal the engine's, in order
    for (cmd, input, limit) in [
        ("suggest", "ro", 5),
        ("suggest", "baş", 5),
        ("correct", "bas", 5),
        ("correct", "rojx", 2),
        ("complete", "ro", 5),
        ("complete", "roj", 1),
        ("complete", "zzz", 5),
    ] {
        let limit_str = limit.to_string();
        let (code, out, _) = run(
            &[
                "--pack", pack_str, "--json", cmd, input, "--limit", &limit_str,
            ],
            None,
        );
        assert_eq!(code, 0, "{} {}", cmd, input);
        let v: Value = serde_json::from_str(&out).unwrap();
        if cmd == "suggest" {
            // The pre-existing `suggest --json` output (a plain array of the engine's
            // SuggestionResult values) is kept unchanged for existing consumers.
            let expected =
                serde_json::to_value(engine.suggest(input, SuggestOptions { limit })).unwrap();
            assert_eq!(v, expected, "{} {}", cmd, input);
            continue;
        }
        assert_eq!(v["command"], cmd);
        assert_eq!(v["input"], input);
        let expected = match cmd {
            "correct" => expected_suggestions(engine.correct(input, CorrectionOptions { limit })),
            _ => expected_suggestions(engine.complete(input, CompletionOptions { limit })),
        };
        assert_eq!(v["results"], expected, "{} {}", cmd, input);
    }
    // The fixture pins one concrete answer so the comparison is not vacuous.
    let (_, out, _) = run(&["--pack", pack_str, "--json", "complete", "ro"], None);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["results"][0]["text"], "roj");
    assert_eq!(v["results"][0]["kind"], "completion");
    assert_eq!(v["results"][1]["text"], "roja");

    // predict: bigram, trigram, backoff and zero-result contexts
    for context in [
        vec!["roj"],
        vec!["roj", "roja"],
        vec!["nope", "roj"],
        vec!["nope", "zzz"],
        vec!["Roj"],
    ] {
        let mut args = vec!["--pack", pack_str, "--json", "predict"];
        args.extend(context.iter());
        let (code, out, _) = run(&args, None);
        assert_eq!(code, 0, "predict {:?}", context);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["command"], "predict");
        assert_eq!(v["context"], serde_json::json!(context));
        let expected: Vec<Value> = engine
            .predict_next(&context, PredictionOptions { limit: 5 })
            .into_iter()
            .map(|p| {
                serde_json::json!({
                    "text": p.text,
                    "source": source_name(p.source),
                    "count": p.count,
                    "probability_millionths": p.probability_millionths,
                })
            })
            .collect();
        assert_eq!(
            v["results"],
            Value::Array(expected),
            "predict {:?}",
            context
        );
    }
    let (_, out, _) = run(&["--pack", pack_str, "--json", "predict", "roj"], None);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["results"][0]["text"], "roja");
    assert_eq!(v["results"][0]["source"], "bigram");
    assert_eq!(v["results"][0]["count"], 20);
    let (_, out, _) = run(
        &["--pack", pack_str, "--json", "predict", "roj", "roja"],
        None,
    );
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["results"][0]["text"], "baş");
    assert_eq!(v["results"][0]["source"], "trigram");
    let (_, out, _) = run(
        &["--pack", pack_str, "--json", "predict", "nope", "roj"],
        None,
    );
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["results"][0]["source"], "bigramBackoff");
}

#[test]
fn json_output_is_deterministic_and_pack_flag_position_is_flexible() {
    let (_dir, pack) = fixture_pack();
    let pack_str = pack.to_str().unwrap();
    let (_, a, _) = run(&["--pack", pack_str, "--json", "complete", "ro"], None);
    let (_, b, _) = run(&["complete", "ro", "--json", "--pack", pack_str], None);
    let (_, c, _) = run(&["--json", "complete", "--pack", pack_str, "ro"], None);
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert!(a.contains("\"command\": \"complete\""));
}

#[test]
fn text_mode_is_readable() {
    let (_dir, pack) = fixture_pack();
    let pack_str = pack.to_str().unwrap();
    let (code, out, _) = run(&["--pack", pack_str, "known", "roj"], None);
    assert_eq!(code, 0);
    assert_eq!(out, "roj: known\n");
    let (_, out, _) = run(&["--pack", pack_str, "complete", "ro"], None);
    assert!(out.starts_with("Completions for 'ro':\n  1. roj"));
    let (_, out, _) = run(&["--pack", pack_str, "predict", "roj", "roja"], None);
    assert!(out.starts_with("Predictions after 'roj roja':\n  1. baş"));
    assert!(out.contains("trigram"));
    let (_, out, _) = run(&["--pack", pack_str, "correct", "zzzzzz"], None);
    assert!(out.contains("(none)"));
}

#[test]
fn legacy_commands_keep_working_with_global_pack_flag() {
    let (_dir, pack) = fixture_pack();
    let pack_str = pack.to_str().unwrap();
    let (code, out, _) = run(&["suggest", "ro", "--pack", pack_str], None);
    assert_eq!(code, 0);
    assert!(out.contains("Suggestions for 'ro'"));
    let (code, out, _) = run(
        &["predict-next", "roj", "--explain", "--pack", pack_str],
        None,
    );
    assert_eq!(code, 0);
    assert!(out.contains("[model: bigram]"));
    let (code, out, _) = run(
        &["--json", "predict-next", "roj", "roja", "--pack", pack_str],
        None,
    );
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["model"], "trigram");
}

#[test]
fn load_and_usage_failures_exit_non_zero_with_stderr() {
    let (dir, _pack) = fixture_pack();
    let missing = dir.path().join("missing.bin");
    let (code, out, err) = run(&["--pack", missing.to_str().unwrap(), "known", "roj"], None);
    assert_ne!(code, 0);
    assert!(out.is_empty());
    assert!(err.contains("failed to load binary pack"));

    let garbage = dir.path().join("garbage.bin");
    std::fs::write(&garbage, b"KRM1 this is not a pack at all, not even close").unwrap();
    let (code, _, err) = run(&["--pack", garbage.to_str().unwrap(), "known", "roj"], None);
    assert_ne!(code, 0);
    assert!(err.contains("failed to load binary pack"));

    let (code, _, err) = run(&["known"], None);
    assert_ne!(code, 0);
    assert!(!err.is_empty());
    let (code, _, _) = run(&["predict", "a", "b", "c"], None);
    assert_ne!(code, 0);
}

#[test]
fn interactive_mode_dispatches_to_the_same_handlers() {
    let (_dir, pack) = fixture_pack();
    let pack_str = pack.to_str().unwrap();
    let script = "known roj\n\
                  bogus\n\
                  complete\n\
                  limit 1\n\
                  complete ro\n\
                  json on\n\
                  predict roj roja\n\
                  json off\n\
                  predict nope roj\n\
                  quit\n\
                  known baş\n";
    let (code, out, _) = run(&["--pack", pack_str, "interactive"], Some(script));
    assert_eq!(code, 0);
    assert!(out.contains("roj: known\n"));
    assert!(out.contains("unknown command 'bogus'"));
    assert!(out.contains("complete expects one word"));
    assert!(out.contains("limit: 1\n"));
    // limit 1 applied: only the first completion
    assert!(out.contains("Completions for 'ro':\n  1. roj"));
    assert!(!out.contains("2. roja"));
    // json on: the JSON block equals the subcommand's JSON output
    let (_, expected_json, _) = run(
        &[
            "--pack", pack_str, "--json", "predict", "roj", "roja", "--limit", "1",
        ],
        None,
    );
    assert!(out.contains(expected_json.trim_end()));
    assert!(out.contains("Predictions after 'nope roj':"));
    // quit stops the session: the trailing command never runs
    assert!(!out.contains("baş: known"));

    // End of input without quit also ends the session cleanly.
    let (code, out, _) = run(&["--pack", pack_str, "interactive"], Some("known roja\n"));
    assert_eq!(code, 0);
    assert!(out.contains("roja: known\n"));
}
