//! Concurrency semantics and Unicode input contract of the public engine API.
//!
//! Concurrency: one loaded `KurmanciEngine` is immutable and serves any number of threads;
//! every query answers exactly what the same query answers single-threaded, no matter how
//! many threads interleave, and the engine may be dropped once all callers are done.
//!
//! Unicode: inputs go through the canonical repository normalization (control characters,
//! U+200B and U+FEFF removed, NFC, lower case), so precomposed and decomposed forms of
//! `ç ê î ş û`, any casing, and words decorated with invisible or control characters query
//! the same word. Ordinary whitespace and NBSP are not removed and therefore make a different
//! input; empty input yields empty results. These tests pin that contract.

mod common;

use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, PredictionOptions, SuggestOptions,
};
use std::path::PathBuf;
use std::sync::Arc;

fn fixture_engine() -> KurmanciEngine {
    KurmanciEngine::from_pack_bytes(&common::minimal_v4_pack()).unwrap()
}

fn seed_engine() -> Option<KurmanciEngine> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/build/packs/seed/lexicon.bin");
    KurmanciEngine::from_pack_file(path).ok()
}

/// A serializable snapshot of every query answer for one input.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Answer {
    known: bool,
    suggest: Vec<(String, u32)>,
    correct: Vec<(String, u32)>,
    complete: Vec<(String, u32)>,
    predict: Vec<(String, u64, u32)>,
}

fn answer(engine: &KurmanciEngine, input: &str) -> Answer {
    let pairs = |v: Vec<kurmanci_engine::SuggestionResult>| {
        v.into_iter().map(|s| (s.text, s.edit_cost)).collect()
    };
    Answer {
        known: engine.is_known_word(input),
        suggest: pairs(engine.suggest(input, SuggestOptions { limit: 5 })),
        correct: pairs(engine.correct(input, CorrectionOptions { limit: 5 })),
        complete: pairs(engine.complete(input, CompletionOptions { limit: 5 })),
        predict: engine
            .predict_next(&[input], PredictionOptions { limit: 5 })
            .into_iter()
            .map(|p| (p.text, p.count, p.probability_millionths))
            .collect(),
    }
}

const INPUTS: &[&str] = &[
    "roj",
    "roja",
    "baş",
    "ro",
    "r",
    "ba",
    "bas",
    "roz",
    "rojb",
    "xyz",
    "",
    "Roj",
    "BAŞ",
    "bas\u{0327}",
    "welat",
    "spaz",
    "rojbas",
    "kurdî",
    "çav",
    "êvar",
    "pirtûk",
    "şev",
];

#[test]
fn concurrent_queries_answer_exactly_the_single_threaded_results() {
    let mut engines: Vec<Arc<KurmanciEngine>> = vec![Arc::new(fixture_engine())];
    if let Some(seed) = seed_engine() {
        engines.push(Arc::new(seed));
    }
    for engine in engines {
        let baseline: Vec<Answer> = INPUTS.iter().map(|i| answer(&engine, i)).collect();
        let threads: Vec<_> = (0..16)
            .map(|t| {
                let engine = Arc::clone(&engine);
                let baseline = baseline.clone();
                std::thread::spawn(move || {
                    for round in 0..250 {
                        // Rotate the starting input per thread and round so threads
                        // interleave different operations.
                        let start = (t * 7 + round) % INPUTS.len();
                        for k in 0..INPUTS.len() {
                            let i = (start + k) % INPUTS.len();
                            assert_eq!(answer(&engine, INPUTS[i]), baseline[i], "{:?}", INPUTS[i]);
                        }
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().expect("query thread panicked");
        }
        // All callers are done: the last reference drops the engine.
        assert_eq!(Arc::strong_count(&engine), 1);
        drop(engine);
    }
}

#[test]
fn engine_is_send_and_sync_and_survives_being_moved_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<KurmanciEngine>();
    let engine = fixture_engine();
    let expected = answer(&engine, "ro");
    let handle = std::thread::spawn(move || {
        let moved = engine;
        assert_eq!(answer(&moved, "ro"), expected);
        moved
    });
    let back = handle.join().unwrap();
    assert!(back.is_known_word("roj"));
}

#[test]
fn precomposed_decomposed_and_cased_forms_query_the_same_word() {
    // Fixture: baş (ş = U+015F; decomposed s + U+0327).
    let engine = fixture_engine();
    let forms = ["baş", "bas\u{0327}", "BAŞ", "Baş", "BAS\u{0327}"];
    let reference = answer(&engine, "baş");
    for form in forms {
        assert_eq!(answer(&engine, form), reference, "{:?}", form);
    }
    assert!(reference.known);

    // Seed pack, when built: every Kurmancî diacritic through its decomposed form.
    let Some(engine) = seed_engine() else {
        eprintln!("skipping seed-pack diacritic checks: pack not built");
        return;
    };
    let cases: [(&str, &str); 5] = [
        ("çav", "c\u{0327}av"),       // ç = c + cedilla
        ("êvar", "e\u{0302}var"),     // ê = e + circumflex (prefix query)
        ("bijî", "biji\u{0302}"),     // î = i + circumflex
        ("şev", "s\u{0327}ev"),       // ş = s + cedilla
        ("pirtûk", "pirtu\u{0302}k"), // û = u + circumflex
    ];
    for (precomposed, decomposed) in cases {
        assert_eq!(
            answer(&engine, decomposed),
            answer(&engine, precomposed),
            "{:?}",
            precomposed
        );
        assert_eq!(
            answer(&engine, &precomposed.to_uppercase()),
            answer(&engine, precomposed),
            "uppercase {:?}",
            precomposed
        );
    }
    assert!(engine.is_known_word("c\u{0327}av"));
    assert!(engine.is_known_word("ÇAV"));
}

#[test]
fn invisible_and_control_characters_are_removed_by_canonical_normalization() {
    let engine = fixture_engine();
    let base = "roj";
    let reference = answer(&engine, base);
    assert!(reference.known);
    // Canonical cleaning removes these, so the decorated input is the same word.
    for decorated in [
        format!("\u{FEFF}{}", base), // BOM
        format!("{}\u{200B}", base), // zero-width space
        format!("\u{200B}{}", base),
        format!("{}\u{0001}", base), // control
        format!("{}\u{0000}", base), // NUL (possible in Rust strings; never through C)
        format!("{}\t", base),       // tab is a control character
        format!("\u{FEFF}{}\u{200B}\u{0002}", base),
    ] {
        assert_eq!(answer(&engine, &decorated), reference, "{:?}", decorated);
    }
    // Ordinary whitespace and NBSP are not part of the cleaning rule: different input.
    for spaced in [format!(" {}", base), format!("{}\u{00A0}", base)] {
        assert!(!engine.is_known_word(&spaced), "{:?}", spaced);
        let _ = answer(&engine, &spaced);
    }
}

#[test]
fn empty_and_whitespace_inputs_yield_empty_results() {
    let engine = fixture_engine();
    for input in ["", " ", "\t", "\u{200B}"] {
        assert!(!engine.is_known_word(input));
        assert!(engine
            .predict_next(&[input], PredictionOptions { limit: 5 })
            .is_empty());
    }
    assert!(engine.suggest("", SuggestOptions { limit: 5 }).is_empty());
    assert!(engine
        .complete("", CompletionOptions { limit: 5 })
        .is_empty());
    assert!(engine
        .correct("", CorrectionOptions { limit: 5 })
        .is_empty());
    assert!(engine
        .predict_next(&[], PredictionOptions { limit: 5 })
        .is_empty());
}

#[test]
fn mixed_ascii_and_kurmanci_input_is_handled_consistently() {
    let engine = fixture_engine();
    for input in ["roj123", "roj-baş", "roj baş", "123", "rojbaş!", "ROJ_baş"] {
        assert!(!engine.is_known_word(input), "{:?}", input);
        let a = answer(&engine, input);
        // Deterministic: the same input always gives the same answer.
        assert_eq!(a, answer(&engine, input));
    }
}
