//! Integration tests for Milestone 3B: Deterministic Trigram Language Model & Pack v4.

use data_builder_lib::corpus::ngrams::{
    build_corpus_bigrams, build_corpus_ngrams, build_corpus_trigrams, split_into_sentences,
    NgramConfig,
};
use data_builder_lib::{
    compile_binary_pack_with_root, import_corpus, run_next_word_evaluation, SourceLexiconEntry,
};
use kurmanci_engine::{Engine, PredictionSource};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("Failed to find workspace root")
        .to_path_buf()
}

#[test]
fn test_sentence_isolated_trigram_extraction() {
    let text = "Ez baş im, spasiya te dikim. Tu ji ku yî? Ez ji Amedê me.";
    let sents = split_into_sentences(text);
    assert_eq!(sents.len(), 3);
    assert_eq!(sents[0], "Ez baş im, spasiya te dikim.");
    assert_eq!(sents[1], "Tu ji ku yî?");
    assert_eq!(sents[2], "Ez ji Amedê me.");
}

#[test]
fn test_checked_trigram_probabilities_and_pruning() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let _ = import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");

    let stats = build_corpus_trigrams(&root).expect("Trigram build failed");
    for rec in &stats.records {
        assert!(rec.count >= 2);
        assert!(rec.count <= rec.context_count);
        assert!(rec.context_count > 0);
        assert!(rec.probability_millionths <= 1_000_000);
    }
}

#[test]
fn test_binary_pack_v4_trigram_indices_and_engine_prediction() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let _ = import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");
    let _ = build_corpus_ngrams(&root).expect("Ngram build failed");

    let source_path = root.join("data/reviewed/lexicon.jsonl");
    let file = fs::File::open(&source_path).expect("Failed to open lexicon.jsonl");
    let reader = std::io::BufReader::new(file);
    let mut entries = Vec::new();
    for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
        if !line.trim().is_empty() {
            entries.push(serde_json::from_str::<SourceLexiconEntry>(&line).unwrap());
        }
    }

    let pack_bytes =
        compile_binary_pack_with_root(&root, &entries).expect("Compilation to pack v4 failed");
    let mut engine = Engine::new();
    let loaded = engine
        .load_binary_pack(&pack_bytes)
        .expect("Loading pack v4 failed");

    assert_eq!(loaded, entries.len());

    // Test limit == 0 returns source None
    let res_zero = engine.predict_next_with_context("ev", "platform", 0);
    assert_eq!(res_zero.source, None);
    assert!(res_zero.predictions.is_empty());

    // Test two-word context prediction (trigram hit)
    let res_hit = engine.predict_next_with_context("ev", "platform", 5);
    if !res_hit.predictions.is_empty() {
        assert_eq!(res_hit.source, Some(PredictionSource::Trigram));
        assert_eq!(res_hit.predictions[0].word, "ji");
    }

    // Test bigram backoff when prev2 is unknown
    let res_backoff = engine.predict_next_with_context("unknownwordxyz", "ji", 5);
    if !res_backoff.predictions.is_empty() {
        assert_eq!(res_backoff.source, Some(PredictionSource::BigramBackoff));
        assert_eq!(res_backoff.predictions[0].word, "bo");
    }

    // Test unknown context pair
    let res_unknown = engine.predict_next_with_context("unknownword123", "unknownword456", 5);
    assert_eq!(res_unknown.source, None);
    assert!(res_unknown.predictions.is_empty());
}

#[test]
fn test_compiler_engine_v4_roundtrip() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let _ = import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");
    let _ = build_corpus_ngrams(&root).expect("Ngram build failed");

    let source_path = root.join("data/reviewed/lexicon.jsonl");
    let file = fs::File::open(&source_path).expect("Failed to open lexicon.jsonl");
    let reader = std::io::BufReader::new(file);
    let mut entries = Vec::new();
    for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
        if !line.trim().is_empty() {
            entries.push(serde_json::from_str::<SourceLexiconEntry>(&line).unwrap());
        }
    }

    let pack_bytes =
        compile_binary_pack_with_root(&root, &entries).expect("Compilation to pack v4 failed");
    let mut engine = Engine::new();
    let loaded = engine
        .load_binary_pack(&pack_bytes)
        .expect("Every successful compiler output must load into engine");
    assert_eq!(loaded, entries.len());
}

#[test]
fn test_trigram_and_context_eval_determinism() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let _ = import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");
    let _ = build_corpus_ngrams(&root).expect("Ngram build failed");

    let source_path = root.join("data/reviewed/lexicon.jsonl");
    let file = fs::File::open(&source_path).expect("Failed to open lexicon.jsonl");
    let reader = std::io::BufReader::new(file);
    let mut entries = Vec::new();
    for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
        if !line.trim().is_empty() {
            entries.push(serde_json::from_str::<SourceLexiconEntry>(&line).unwrap());
        }
    }

    let pack_bytes1 = compile_binary_pack_with_root(&root, &entries).unwrap();
    let pack_bytes2 = compile_binary_pack_with_root(&root, &entries).unwrap();
    assert_eq!(pack_bytes1, pack_bytes2);

    let pack_file = root.join("data/build/lexicon.bin");
    fs::write(&pack_file, &pack_bytes1).unwrap();

    let eval_summary1 = run_next_word_evaluation(&root).expect("Next-word eval 1 failed");
    eprintln!("DEBUG eval_summary1: {:#?}", eval_summary1);
    assert!(eval_summary1.acceptance_passed);
    assert!(eval_summary1.model_quality_passed);
    assert!(eval_summary1.pipeline_validation_passed);

    let eval_summary2 = run_next_word_evaluation(&root).expect("Next-word eval 2 failed");
    assert_eq!(
        eval_summary1.positive_top_1_accuracy,
        eval_summary2.positive_top_1_accuracy
    );
    assert_eq!(
        eval_summary1.source_selection_accuracy,
        eval_summary2.source_selection_accuracy
    );
}

#[test]
fn test_dynamic_trigram_pruning_config_and_report_alignment() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let _ = import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");

    let config_path = root.join("data-builder/config/ngrams.toml");
    let original_config = fs::read_to_string(&config_path).expect("Failed to read ngrams.toml");

    let custom_config = r#"[bigram]
minimum_count = 2
maximum_predictions_per_context = 16

[trigram]
minimum_count = 3
maximum_predictions_per_context = 6
"#;
    fs::write(&config_path, custom_config).expect("Failed to write custom ngrams.toml");

    let res = build_corpus_trigrams(&root);
    fs::write(&config_path, &original_config).expect("Failed to restore ngrams.toml");

    let stats = res.expect("Trigram build failed with custom config");
    for rec in &stats.records {
        assert!(rec.count >= 3);
    }

    let summary_bytes = fs::read(root.join("data/reports/trigrams/pruning-summary.json"))
        .expect("Read pruning-summary.json failed");
    let report: serde_json::Value = serde_json::from_slice(&summary_bytes).unwrap();

    assert_eq!(report["minimum_count_threshold"], 3);
    assert_eq!(report["maximum_predictions_per_context"], 6);
}

#[test]
fn test_exact_trigram_reports_and_manifest_set() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let _ = import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");
    let _ = build_corpus_trigrams(&root).expect("Trigram build failed");

    let reports_dir = root.join("data/reports/trigrams");
    let expected_files: BTreeSet<String> = [
        "summary.json",
        "top-trigrams.json",
        "context-distribution.json",
        "out-of-vocabulary.json",
        "pruning-summary.json",
        "README.md",
        "artifacts.sha256",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    // 1. Assert exact directory entries set
    let actual_files: BTreeSet<String> = fs::read_dir(&reports_dir)
        .expect("Read reports dir failed")
        .filter_map(|e| {
            e.ok()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
        })
        .collect();

    assert_eq!(
        actual_files, expected_files,
        "Report directory entry set mismatch"
    );

    // 2. Read and parse artifacts.sha256 manifest
    let manifest_bytes = fs::read(reports_dir.join("artifacts.sha256")).unwrap();
    let manifest_str = String::from_utf8(manifest_bytes).unwrap();
    assert!(
        !manifest_str.contains("artifacts.sha256"),
        "Manifest must exclude itself"
    );

    let expected_manifest_paths: BTreeSet<String> = [
        "data/build/trigrams.jsonl",
        "data/reports/trigrams/summary.json",
        "data/reports/trigrams/top-trigrams.json",
        "data/reports/trigrams/context-distribution.json",
        "data/reports/trigrams/out-of-vocabulary.json",
        "data/reports/trigrams/pruning-summary.json",
        "data/reports/trigrams/README.md",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    // 3. Verify every manifest entry hash against disk file contents
    let mut actual_manifest_paths: BTreeSet<String> = BTreeSet::new();
    for line in manifest_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(
            parts.len(),
            2,
            "Manifest line format must be '<hash> <rel_path>'"
        );
        let expected_hash = parts[0];
        let rel_path = parts[1];

        assert!(
            !rel_path.ends_with("artifacts.sha256"),
            "Manifest must exclude itself"
        );

        actual_manifest_paths.insert(rel_path.to_string());

        let target_file = root.join(rel_path);
        assert!(
            target_file.exists(),
            "Manifest references non-existent file {:?}",
            target_file
        );

        let file_bytes = fs::read(&target_file).expect("Failed to read manifest target file");
        let computed_hash = format!("{:x}", Sha256::digest(&file_bytes));

        assert_eq!(
            computed_hash, expected_hash,
            "SHA-256 mismatch for manifest entry {}",
            rel_path
        );
    }

    assert_eq!(
        actual_manifest_paths, expected_manifest_paths,
        "Manifest relative path set mismatch"
    );
}

#[test]
fn test_corrupted_trigram_corpus_checksum_rejection() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let corpus_file = root.join("data/imported/opensubtitles-kmr/corpus.txt");
    if corpus_file.exists() {
        let original = fs::read_to_string(&corpus_file).unwrap();
        fs::write(&corpus_file, format!("{}\nCorrupted line", original)).unwrap();

        let res = build_corpus_trigrams(&root);
        fs::write(&corpus_file, &original).unwrap(); // Restore

        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Corpus file checksum mismatch"));
    }
}

#[test]
fn test_missing_registered_corpus_directory_fails() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let imported_dir = root.join("data/imported/opensubtitles-kmr");
    let backup_dir = root.join("data/imported/opensubtitles-kmr-backup");

    if imported_dir.exists() {
        fs::rename(&imported_dir, &backup_dir).unwrap();
    }

    let bigram_res = build_corpus_bigrams(&root);
    let trigram_res = build_corpus_trigrams(&root);

    if backup_dir.exists() {
        fs::rename(&backup_dir, &imported_dir).unwrap();
    }

    assert!(bigram_res.is_err());
    assert!(bigram_res
        .unwrap_err()
        .contains("Imported corpus directory missing"));

    assert!(trigram_res.is_err());
    assert!(trigram_res
        .unwrap_err()
        .contains("Imported corpus directory missing"));
}

#[test]
fn test_exceeding_max_predictions_limit_fails() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let config_path = root.join("data-builder/config/ngrams.toml");
    let original = fs::read_to_string(&config_path).unwrap();

    // Test Bigram 17
    let cfg_bigram_17 = "[bigram]\nminimum_count = 1\nmaximum_predictions_per_context = 17\n";
    fs::write(&config_path, cfg_bigram_17).unwrap();
    let bigram_res = build_corpus_bigrams(&root);

    // Test Trigram 13
    let cfg_trigram_13 = "[trigram]\nminimum_count = 1\nmaximum_predictions_per_context = 13\n";
    fs::write(&config_path, cfg_trigram_13).unwrap();
    let trigram_res = build_corpus_trigrams(&root);

    fs::write(&config_path, &original).unwrap();

    assert!(bigram_res.is_err());
    assert!(bigram_res
        .unwrap_err()
        .contains("bigram.maximum_predictions_per_context must be between 1 and 16"));

    assert!(trigram_res.is_err());
    assert!(trigram_res
        .unwrap_err()
        .contains("trigram.maximum_predictions_per_context must be between 1 and 12"));
}

#[test]
fn test_malformed_config_fails_load() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let config_path = root.join("data-builder/config/ngrams.toml");
    let original = fs::read_to_string(&config_path).unwrap();

    let malformed = "invalid_toml = [[[unclosed";
    fs::write(&config_path, malformed).unwrap();
    let load_res = NgramConfig::load(&root);

    fs::write(&config_path, &original).unwrap();

    assert!(load_res.is_err());
    assert!(load_res
        .unwrap_err()
        .contains("Invalid n-gram configuration"));
}
