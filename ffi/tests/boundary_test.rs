//! The C boundary must give exactly the core engine's answers for every input, including
//! Unicode edge cases, and must reject malformed UTF-8 with a status rather than reading it.
//! One handle must serve many threads concurrently with identical results.

use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, PredictionOptions, SuggestOptions,
};
use kurmanci_ffi::*;
use std::ffi::CString;
use std::os::raw::c_char;
use std::path::PathBuf;

fn seed_pack_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/build/packs/seed/lexicon.bin");
    std::fs::read(&path).expect("seed pack must be built for FFI tests")
}

struct Handle(*mut kmr_engine);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

fn create(bytes: &[u8]) -> Handle {
    let mut engine: *mut kmr_engine = std::ptr::null_mut();
    let status = unsafe { kmr_engine_create_from_bytes(bytes.as_ptr(), bytes.len(), &mut engine) };
    assert_eq!(status, KMR_OK);
    Handle(engine)
}

fn c_text(ptr: *const c_char) -> String {
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_str()
        .unwrap()
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Answer {
    known: bool,
    suggest: Vec<(String, u32, u32)>,
    correct: Vec<(String, u32, u32)>,
    complete: Vec<(String, u32, u32)>,
    predict: Vec<(String, u64, u32, u32)>,
}

fn kind_code(kind: &kurmanci_engine::SuggestionKind) -> u32 {
    use kurmanci_engine::SuggestionKind::*;
    match kind {
        Exact => KMR_SUGGESTION_EXACT,
        Completion => KMR_SUGGESTION_COMPLETION,
        Correction => KMR_SUGGESTION_CORRECTION,
        DiacriticCorrection => KMR_SUGGESTION_DIACRITIC_CORRECTION,
        NextWord => KMR_SUGGESTION_NEXT_WORD,
    }
}

fn source_code(source: kurmanci_engine::PredictionSource) -> u32 {
    use kurmanci_engine::PredictionSource::*;
    match source {
        Trigram => KMR_PREDICTION_TRIGRAM,
        BigramBackoff => KMR_PREDICTION_BIGRAM_BACKOFF,
        Bigram => KMR_PREDICTION_BIGRAM,
        None => KMR_PREDICTION_NONE,
    }
}

fn rust_answer(engine: &KurmanciEngine, input: &str) -> Answer {
    let conv = |v: Vec<kurmanci_engine::SuggestionResult>| {
        v.into_iter()
            .map(|s| (s.text, kind_code(&s.kind), s.edit_cost))
            .collect()
    };
    Answer {
        known: engine.is_known_word(input),
        suggest: conv(engine.suggest(input, SuggestOptions { limit: 5 })),
        correct: conv(engine.correct(input, CorrectionOptions { limit: 5 })),
        complete: conv(engine.complete(input, CompletionOptions { limit: 5 })),
        predict: engine
            .predict_next(&[input], PredictionOptions { limit: 5 })
            .into_iter()
            .map(|p| {
                (
                    p.text,
                    p.count,
                    p.probability_millionths,
                    source_code(p.source),
                )
            })
            .collect(),
    }
}

unsafe fn c_list(list: *mut kmr_suggestion_list) -> Vec<(String, u32, u32)> {
    let mut len = 0usize;
    assert_eq!(kmr_suggestion_list_len(list, &mut len), KMR_OK);
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let mut item = kmr_suggestion_item {
            text: std::ptr::null(),
            kind: 0,
            edit_cost: 0,
        };
        assert_eq!(kmr_suggestion_list_get(list, i, &mut item), KMR_OK);
        out.push((c_text(item.text), item.kind, item.edit_cost));
    }
    kmr_suggestion_list_destroy(list);
    out
}

fn c_answer(handle: &Handle, input: &str) -> Answer {
    let c_input = CString::new(input).unwrap();
    unsafe {
        let mut known = false;
        assert_eq!(
            kmr_engine_is_known_word(handle.0, c_input.as_ptr(), &mut known),
            KMR_OK
        );
        let mut list: *mut kmr_suggestion_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_suggest(handle.0, c_input.as_ptr(), 5, &mut list),
            KMR_OK
        );
        let suggest = c_list(list);
        assert_eq!(
            kmr_engine_correct(handle.0, c_input.as_ptr(), 5, &mut list),
            KMR_OK
        );
        let correct = c_list(list);
        assert_eq!(
            kmr_engine_complete(handle.0, c_input.as_ptr(), 5, &mut list),
            KMR_OK
        );
        let complete = c_list(list);
        let ctx = [c_input.as_ptr()];
        let mut preds: *mut kmr_prediction_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_predict_next(handle.0, ctx.as_ptr(), 1, 5, &mut preds),
            KMR_OK
        );
        let mut len = 0usize;
        assert_eq!(kmr_prediction_list_len(preds, &mut len), KMR_OK);
        let mut predict = Vec::with_capacity(len);
        for i in 0..len {
            let mut item = kmr_prediction_item {
                text: std::ptr::null(),
                count: 0,
                probability_millionths: 0,
                source: 0,
            };
            assert_eq!(kmr_prediction_list_get(preds, i, &mut item), KMR_OK);
            predict.push((
                c_text(item.text),
                item.count,
                item.probability_millionths,
                item.source,
            ));
        }
        kmr_prediction_list_destroy(preds);
        Answer {
            known,
            suggest,
            correct,
            complete,
            predict,
        }
    }
}

/// Inputs covering every Kurmancî diacritic in precomposed and decomposed form, casing,
/// invisible and control characters, mixed content, prefixes, typos and empties.
const INPUTS: &[&str] = &[
    "welat",
    "WELAT",
    "Welat",
    "spaz",
    "roj",
    "rojb",
    "rojbaş",
    "rojbas",
    "rojbas\u{0327}",
    "ROJBAŞ",
    "bijî",
    "biji\u{0302}",
    "biji",
    "çav",
    "c\u{0327}av",
    "cav",
    "êvar",
    "e\u{0302}var",
    "pirtûk",
    "pirtu\u{0302}k",
    "pirtuk",
    "şev",
    "s\u{0327}ev",
    "sev",
    "kurdî",
    "kurmancî",
    "ez",
    "li",
    "\u{FEFF}welat",
    "welat\u{200B}",
    "\u{200B}welat",
    "welat\u{0001}",
    "welat\u{00A0}",
    " welat",
    "welat123",
    "roj-baş",
    "roj baş",
    "123",
    "!",
    "",
    "\u{200B}",
    "ê",
    "î",
    "ş",
    "ç",
    "û",
    "xyzqwv",
];

#[test]
fn c_boundary_answers_equal_core_engine_answers_for_unicode_inputs() {
    let bytes = seed_pack_bytes();
    let core = KurmanciEngine::from_pack_bytes(&bytes).unwrap();
    let handle = create(&bytes);
    for input in INPUTS {
        assert_eq!(
            c_answer(&handle, input),
            rust_answer(&core, input),
            "{:?}",
            input
        );
    }
    // Decomposed forms answer as their precomposed word, through the C boundary too.
    for (pre, de) in [
        ("rojbaş", "rojbas\u{0327}"),
        ("bijî", "biji\u{0302}"),
        ("çav", "c\u{0327}av"),
        ("pirtûk", "pirtu\u{0302}k"),
        ("şev", "s\u{0327}ev"),
    ] {
        assert_eq!(c_answer(&handle, de), c_answer(&handle, pre), "{:?}", pre);
        assert!(c_answer(&handle, de).known, "{:?}", pre);
    }
    // Canonical cleaning removes BOM, zero-width space and control characters through the
    // C boundary too; ordinary whitespace and NBSP stay part of the input.
    let clean = c_answer(&handle, "welat");
    assert!(clean.known);
    for decorated in [
        "\u{FEFF}welat",
        "welat\u{200B}",
        "\u{200B}welat",
        "welat\u{0001}",
        "welat\t",
    ] {
        assert_eq!(c_answer(&handle, decorated), clean, "{:?}", decorated);
    }
    assert!(!c_answer(&handle, " welat").known);
    assert!(!c_answer(&handle, "welat\u{00A0}").known);
    unsafe { kmr_engine_destroy(handle.0) };
}

#[test]
fn malformed_utf8_is_rejected_by_every_function_with_a_status() {
    let bytes = seed_pack_bytes();
    let handle = create(&bytes);
    // Lone continuation, truncated multibyte sequence, overlong encoding, UTF-16 surrogate,
    // and a valid prefix followed by an invalid byte.
    let malformed: [&[u8]; 6] = [
        b"\x80\x00",
        b"\xC3\x00",
        b"\xE2\x82\x00",
        b"\xC0\xAF\x00",
        b"\xED\xA0\x80\x00",
        b"welat\xFF\x00",
    ];
    for bad in malformed {
        let ptr = bad.as_ptr() as *const c_char;
        unsafe {
            let mut known = true;
            assert_eq!(
                kmr_engine_is_known_word(handle.0, ptr, &mut known),
                KMR_ERROR_INVALID_ARGUMENT
            );
            assert!(!known);
            for f in [kmr_engine_suggest, kmr_engine_correct, kmr_engine_complete] {
                let mut list: *mut kmr_suggestion_list = 0x1 as *mut kmr_suggestion_list;
                assert_eq!(f(handle.0, ptr, 5, &mut list), KMR_ERROR_INVALID_ARGUMENT);
                assert!(list.is_null());
            }
            let good = CString::new("ez").unwrap();
            let ctx = [good.as_ptr(), ptr];
            let mut preds: *mut kmr_prediction_list = 0x1 as *mut kmr_prediction_list;
            assert_eq!(
                kmr_engine_predict_next(handle.0, ctx.as_ptr(), 2, 5, &mut preds),
                KMR_ERROR_INVALID_ARGUMENT
            );
            assert!(preds.is_null());
            let msg = c_text(kmr_last_error_message());
            assert!(msg.contains("UTF-8"), "{}", msg);
        }
        // A malformed path is also an argument error, not an I/O attempt.
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            unsafe { kmr_engine_create_from_file(ptr, &mut engine) },
            KMR_ERROR_INVALID_ARGUMENT
        );
        assert!(engine.is_null());
    }
    unsafe { kmr_engine_destroy(handle.0) };
}

#[test]
fn one_handle_serves_many_threads_with_identical_answers() {
    let bytes = seed_pack_bytes();
    let core = KurmanciEngine::from_pack_bytes(&bytes).unwrap();
    let handle = std::sync::Arc::new(create(&bytes));
    let baseline: Vec<Answer> = INPUTS.iter().map(|i| rust_answer(&core, i)).collect();
    let threads: Vec<_> = (0..12)
        .map(|t| {
            let handle = std::sync::Arc::clone(&handle);
            let baseline = baseline.clone();
            std::thread::spawn(move || {
                for round in 0..60 {
                    let start = (t * 5 + round) % INPUTS.len();
                    for k in 0..INPUTS.len() {
                        let i = (start + k) % INPUTS.len();
                        assert_eq!(c_answer(&handle, INPUTS[i]), baseline[i], "{:?}", INPUTS[i]);
                    }
                    // Errors are thread-local: a failing call on this thread never disturbs
                    // another thread's last error message or results.
                    let bad: &[u8] = b"\xFF\x00";
                    let mut known = false;
                    unsafe {
                        assert_eq!(
                            kmr_engine_is_known_word(
                                handle.0,
                                bad.as_ptr() as *const c_char,
                                &mut known
                            ),
                            KMR_ERROR_INVALID_ARGUMENT
                        );
                    }
                }
            })
        })
        .collect();
    for t in threads {
        t.join().expect("thread panicked");
    }
    let handle = std::sync::Arc::try_unwrap(handle)
        .ok()
        .expect("all threads done");
    unsafe { kmr_engine_destroy(handle.0) };
}

#[test]
fn destroy_after_all_callers_finished_is_the_documented_rule() {
    // Each thread finishes with its handle use before the join; destroy happens after.
    let bytes = seed_pack_bytes();
    for _ in 0..20 {
        let handle = std::sync::Arc::new(create(&bytes));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let handle = std::sync::Arc::clone(&handle);
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        assert!(c_answer(&handle, "welat").known);
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        let handle = std::sync::Arc::try_unwrap(handle).ok().unwrap();
        unsafe { kmr_engine_destroy(handle.0) };
    }
}
