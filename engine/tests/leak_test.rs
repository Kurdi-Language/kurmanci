//! Long-running memory behaviour of a resident engine: a keyboard keeps one engine loaded
//! for hours, so repeated queries must not grow the heap. This test binary installs a
//! counting allocator, loads one engine, runs 100,000 mixed queries and requires the live
//! heap to return exactly to its pre-loop value (the engine keeps no caches and every result
//! is freed by the caller). It is the only test in this file so the counters are not
//! disturbed by other test threads.

mod common;

use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, PredictionOptions, SuggestOptions,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            LIVE.fetch_add(new_size, Ordering::Relaxed);
        }
        p
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

fn run_mixed_queries(engine: &KurmanciEngine, iterations: usize) -> usize {
    let inputs = [
        "roj",
        "roja",
        "baş",
        "ro",
        "bas",
        "roz",
        "rojb",
        "xyz",
        "welat",
        "spaz",
        "kurdî",
        "bas\u{0327}",
        "ÇAV",
        "",
        "\u{FEFF}roj",
    ];
    let mut total = 0usize;
    for i in 0..iterations {
        let input = inputs[i % inputs.len()];
        total += usize::from(engine.is_known_word(input));
        total += engine.suggest(input, SuggestOptions { limit: 5 }).len();
        total += engine.correct(input, CorrectionOptions { limit: 5 }).len();
        total += engine.complete(input, CompletionOptions { limit: 5 }).len();
        total += engine
            .predict_next(&[input], PredictionOptions { limit: 5 })
            .len();
        total += engine
            .predict_next(&["roj", input], PredictionOptions { limit: 5 })
            .len();
    }
    total
}

#[test]
fn hundred_thousand_queries_do_not_grow_the_heap() {
    let mut packs: Vec<(String, Vec<u8>)> = vec![("fixture".into(), common::minimal_v4_pack())];
    let seed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/build/packs/seed/lexicon.bin");
    if let Ok(bytes) = std::fs::read(&seed) {
        packs.push(("seed".into(), bytes));
    }
    for (name, bytes) in packs {
        let engine = KurmanciEngine::from_pack_bytes(&bytes).unwrap();
        drop(bytes);
        // Warm up so that any lazily initialized state is already allocated.
        run_mixed_queries(&engine, 100);
        let before = live();
        let total = run_mixed_queries(&engine, 100_000 / 6);
        let after = live();
        assert!(total > 0);
        assert_eq!(
            after,
            before,
            "{}: live heap grew by {} bytes across the query loop",
            name,
            after as isize - before as isize
        );
        let with_engine = live();
        drop(engine);
        assert!(
            live() < with_engine,
            "{}: dropping the engine must release its heap",
            name
        );
    }
}
