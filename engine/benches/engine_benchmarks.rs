use criterion::{black_box, criterion_group, criterion_main, Criterion};
use kurmanci_engine::{CompletionOptions, CorrectionOptions, KurmanciEngine, PredictionOptions};
use std::path::PathBuf;

fn get_pack_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/build/packs")
        .join(name)
        .join("lexicon.bin")
}

fn bench_engine_operations(c: &mut Criterion) {
    let seed_path = get_pack_path("seed");
    let exp_path = get_pack_path("experimental-full");

    let seed_bytes = std::fs::read(&seed_path).expect("Seed pack file must exist for benchmarks");
    let exp_bytes =
        std::fs::read(&exp_path).expect("Experimental pack file must exist for benchmarks");

    c.bench_function("engine_load_seed_bytes", |b| {
        b.iter(|| KurmanciEngine::from_pack_bytes(black_box(&seed_bytes)).unwrap())
    });

    c.bench_function("engine_load_experimental_bytes", |b| {
        b.iter(|| KurmanciEngine::from_pack_bytes(black_box(&exp_bytes)).unwrap())
    });

    let engine = KurmanciEngine::from_pack_bytes(&seed_bytes).unwrap();

    c.bench_function("is_known_word_seed", |b| {
        b.iter(|| engine.is_known_word(black_box("welat")))
    });

    c.bench_function("correct_seed", |b| {
        b.iter(|| engine.correct(black_box("spaz"), CorrectionOptions { limit: 5 }))
    });

    c.bench_function("complete_seed", |b| {
        b.iter(|| engine.complete(black_box("roj"), CompletionOptions { limit: 5 }))
    });

    c.bench_function("predict_next_seed", |b| {
        b.iter(|| engine.predict_next(black_box(&["ez", "diçim"]), PredictionOptions { limit: 5 }))
    });
}

criterion_group!(benches, bench_engine_operations);
criterion_main!(benches);
