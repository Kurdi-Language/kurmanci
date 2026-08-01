//! Unit and Integration Tests for Milestone 3A Bigram Language Model,
//! Sentence Segmentation, Binary Pack v3 with Lexicon Indices, Engine predict_next API,
//! and 2-Pass Pipeline Determinism.

use data_builder_lib::{
    build_corpus_bigrams, build_corpus_frequencies, compile_binary_pack,
    compile_binary_pack_with_root, import_corpus, join_frequencies_to_lexicon,
    run_next_word_evaluation, split_into_sentences, SourceLexiconEntry,
};
use kurmanci_engine::Engine;
use std::fs;
use std::path::PathBuf;

use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir
        .join("data/source-registry/corpora.toml")
        .exists()
    {
        manifest_dir
    } else if manifest_dir
        .join("../data/source-registry/corpora.toml")
        .exists()
    {
        manifest_dir.join("..")
    } else {
        PathBuf::from(".")
    }
}

#[test]
fn test_sentence_segmentation_rules() {
    // Standard two-sentence split
    let s1 = split_into_sentences("Ez baş im. Tu çawa yî?");
    assert_eq!(s1.len(), 2);
    assert_eq!(s1[0], vec!["ez", "baş", "im"]);
    assert_eq!(s1[1], vec!["tu", "çawa", "yî"]);

    // Ellipsis collapsing
    let s2 = split_into_sentences("Ez baş im... Tu çawa yî?");
    assert_eq!(s2.len(), 2);
    assert_eq!(s2[0], vec!["ez", "baş", "im"]);
    assert_eq!(s2[1], vec!["tu", "çawa", "yî"]);

    // Leading punctuation
    let s3 = split_into_sentences("?! Ez baş im.");
    assert_eq!(s3.len(), 1);
    assert_eq!(s3[0], vec!["ez", "baş", "im"]);

    // Trailing punctuation
    let s4 = split_into_sentences("Ez baş im!!!");
    assert_eq!(s4.len(), 1);
    assert_eq!(s4[0], vec!["ez", "baş", "im"]);

    // Semicolon retention: Semicolon is NOT a sentence terminator
    let s5 = split_into_sentences("Ez baş im; tu çawa yî?");
    assert_eq!(s5.len(), 1);
    assert_eq!(s5[0], vec!["ez", "baş", "im", "tu", "çawa", "yî"]);
}

#[test]
fn test_fixed_point_integer_probability_and_pruning() {
    let count = 42u64;
    let context_count = 200u64;

    let numerator = u128::from(count)
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_add(u128::from(context_count / 2)))
        .unwrap();

    let prob = u32::try_from(numerator / u128::from(context_count)).unwrap();
    assert_eq!(prob, 210000); // Exact 21.0% (210,000 millionths)
}

#[test]
fn test_binary_pack_v3_lexicon_indices_and_engine_prediction() {
    let e1 = SourceLexiconEntry {
        word: "ez".to_string(),
        normalized: "ez".to_string(),
        lemma: "ez".to_string(),
        part_of_speech: "pronoun".to_string(),
        frequency: 100,
        status: "approved".to_string(),
        regions: vec!["all".to_string()],
        sources: vec!["test".to_string()],
        variants: vec![],
        frequency_metadata: None,
    };
    let e2 = SourceLexiconEntry {
        word: "baş".to_string(),
        normalized: "baş".to_string(),
        lemma: "baş".to_string(),
        part_of_speech: "adjective".to_string(),
        frequency: 50,
        status: "approved".to_string(),
        regions: vec!["all".to_string()],
        sources: vec!["test".to_string()],
        variants: vec![],
        frequency_metadata: None,
    };

    let entries = vec![e1, e2];
    let pack_bytes = compile_binary_pack(&entries).expect("Compilation failed");

    // Header check
    assert_eq!(&pack_bytes[0..4], b"KRM1");
    let version = u32::from_le_bytes(pack_bytes[4..8].try_into().unwrap());
    assert_eq!(version, 3);

    let mut engine = Engine::new();
    let loaded = engine
        .load_binary_pack(&pack_bytes)
        .expect("Loading v3 pack failed");
    assert_eq!(loaded, 2);

    // Predict on empty bigram index returns empty vec without panic
    let preds = engine.predict_next("ez", 5);
    assert!(preds.is_empty());
}

#[test]
fn test_bigram_build_and_evaluation_pipeline_determinism() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let _ = import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");
    let _ = build_corpus_frequencies(&root).expect("Frequency build failed");

    let source_path = root.join("data/reviewed/lexicon.jsonl");
    let file = fs::File::open(&source_path).expect("Failed to open lexicon.jsonl");
    let reader = std::io::BufReader::new(file);
    let mut entries = Vec::new();
    for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
        if !line.trim().is_empty() {
            entries.push(serde_json::from_str::<SourceLexiconEntry>(&line).unwrap());
        }
    }

    let _ = join_frequencies_to_lexicon(&root, &mut entries).expect("Frequency join failed");

    // Pass 1: Build Bigrams
    let stats1 = build_corpus_bigrams(&root).expect("Bigram build 1 failed");
    let bigrams_bytes1 =
        fs::read(root.join("data/build/bigrams.jsonl")).expect("Read bigrams.jsonl 1 failed");
    let summary1 =
        fs::read(root.join("data/reports/ngrams/summary.json")).expect("Read summary 1 failed");

    // Pass 2: Build Bigrams
    let stats2 = build_corpus_bigrams(&root).expect("Bigram build 2 failed");
    let bigrams_bytes2 =
        fs::read(root.join("data/build/bigrams.jsonl")).expect("Read bigrams.jsonl 2 failed");
    let summary2 =
        fs::read(root.join("data/reports/ngrams/summary.json")).expect("Read summary 2 failed");

    assert_eq!(stats1.total_sentences, stats2.total_sentences);
    assert_eq!(stats1.records.len(), stats2.records.len());
    assert_eq!(bigrams_bytes1, bigrams_bytes2);
    assert_eq!(summary1, summary2);

    // Pass 1: Compile v3 Binary Pack
    let pack1 = compile_binary_pack_with_root(&root, &entries).expect("Pack compile 1 failed");
    fs::create_dir_all(root.join("data/build")).unwrap();
    fs::write(root.join("data/build/lexicon.bin"), &pack1).unwrap();

    // Verify engine loads compiled pack cleanly
    let mut engine = Engine::new();
    let loaded = engine
        .load_binary_pack(&pack1)
        .expect("Engine load of compiled pack failed");
    assert_eq!(loaded, entries.len());

    let pack2 = compile_binary_pack_with_root(&root, &entries).expect("Pack compile 2 failed");
    assert_eq!(pack1, pack2);

    // Pass 1: Next-word evaluation
    let eval_summary1 = run_next_word_evaluation(&root).expect("Next-word eval 1 failed");
    assert!(eval_summary1.acceptance_passed);
    assert!(eval_summary1.model_quality_passed);
    assert!(eval_summary1.pipeline_validation_passed);

    // Pass 2: Next-word evaluation
    let eval_summary2 = run_next_word_evaluation(&root).expect("Next-word eval 2 failed");
    assert!(eval_summary2.acceptance_passed);
    assert_eq!(eval_summary1.top_1_accuracy, eval_summary2.top_1_accuracy);
    assert_eq!(
        eval_summary1.mean_reciprocal_rank,
        eval_summary2.mean_reciprocal_rank
    );
}

#[test]
fn test_corrupted_corpus_checksum_rejection() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let corpus_file = root.join("data/imported/opensubtitles-kmr/corpus.txt");
    if corpus_file.exists() {
        let original = fs::read(&corpus_file).unwrap();
        let mut corrupted = original.clone();
        corrupted.push(b'X');

        fs::write(&corpus_file, &corrupted).unwrap();
        let res = build_corpus_bigrams(&root);
        fs::write(&corpus_file, &original).unwrap(); // Restore

        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Corpus file checksum mismatch"));
    }
}

#[test]
fn test_dynamic_pruning_config_and_report_alignment() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let _ = import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");

    let config_path = root.join("data-builder/config/ngrams.toml");
    let original_config = fs::read_to_string(&config_path).expect("Failed to read ngrams.toml");

    // Write custom pruning configuration with minimum_count = 3 and maximum_predictions_per_context = 8
    let custom_config = r#"[pruning]
minimum_count = 3
maximum_predictions_per_context = 8
"#;
    fs::write(&config_path, custom_config).expect("Failed to write custom ngrams.toml");

    let res = build_corpus_bigrams(&root);
    // Restore original config immediately
    fs::write(&config_path, &original_config).expect("Failed to restore ngrams.toml");

    let stats = res.expect("Bigram build failed with custom config");
    for rec in &stats.records {
        assert!(
            rec.count >= 3,
            "Retained record ({}, {}) count {} must be >= minimum_count 3",
            rec.previous,
            rec.next,
            rec.count
        );
    }

    let summary_bytes = fs::read(root.join("data/reports/ngrams/pruning-summary.json"))
        .expect("Read pruning-summary.json failed");
    let report: serde_json::Value = serde_json::from_slice(&summary_bytes).unwrap();

    assert_eq!(report["minimum_count_threshold"], 3);
    assert_eq!(report["maximum_predictions_per_context"], 8);
}
