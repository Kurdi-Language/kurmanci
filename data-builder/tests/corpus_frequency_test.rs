//! Integration tests for corpus frequency pipeline, tokenization, import, report generation, and determinism.

use data_builder_lib::corpus::{build_corpus_frequencies, import_corpus, tokenize_text};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

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

// ─── Tokenizer Unit Tests ───────────────────────────────────────────────────

#[test]
fn test_tokenizer_casing_and_normalization() {
    assert_eq!(tokenize_text("Spas"), vec!["spas"]);
    assert_eq!(tokenize_text("SPAS"), vec!["spas"]);
    assert_eq!(tokenize_text("spas!"), vec!["spas"]);
    assert_eq!(tokenize_text("rojbaş!"), vec!["rojbaş"]);
}

#[test]
fn test_tokenizer_kurdish_letter_preservation() {
    assert_eq!(tokenize_text("çav"), vec!["çav"]);
    assert_eq!(tokenize_text("şev"), vec!["şev"]);
    assert_eq!(tokenize_text("dîwar"), vec!["dîwar"]);
    assert_eq!(tokenize_text("êvar"), vec!["êvar"]);
    assert_eq!(tokenize_text("hûn"), vec!["hûn"]);
}

#[test]
fn test_tokenizer_numbers_and_punctuation() {
    assert_eq!(tokenize_text("123"), Vec::<String>::new());
    assert_eq!(tokenize_text("sal 2026"), vec!["sal"]);
    assert_eq!(tokenize_text("rojbaş! heval..."), vec!["rojbaş", "heval"]);
}

// ─── Corpus Integration & Determinism Tests ───────────────────────────────

#[test]
fn test_unknown_corpus_import_rejection() {
    let root = get_workspace_root();
    let res = import_corpus("unknown-nonexistent-corpus", &root);
    assert!(res.is_err(), "Importing unknown corpus must fail");
    assert!(
        res.unwrap_err().contains("Unknown corpus_id"),
        "Error must mention unknown corpus_id"
    );
}

static PIPELINE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_corpus_import_and_frequency_build_pipeline() {
    let _lock = PIPELINE_LOCK.lock().unwrap();
    let root = get_workspace_root();

    // 1. Import corpus
    let summary = import_corpus("opensubtitles-kmr", &root)
        .expect("Corpus import must succeed for registered opensubtitles-kmr");
    assert_eq!(summary.corpus_id, "opensubtitles-kmr");
    assert!(summary.checksum_verification_passed);

    let imported_file = root.join("data/imported/opensubtitles-kmr/corpus.txt");
    assert!(imported_file.exists(), "Imported corpus file must exist");

    // 2. Build frequencies (Pass 1)
    let stats1 = build_corpus_frequencies(&root).expect("Build frequencies pass 1 must succeed");
    assert!(stats1.total_tokens > 0);
    assert!(!stats1.records.is_empty());

    let freq_jsonl = root.join("data/build/frequencies.jsonl");
    assert!(freq_jsonl.exists(), "frequencies.jsonl must exist");

    let report_dir = root.join("data/reports/frequencies");
    let expected_reports = [
        "summary.json",
        "top-100.json",
        "length-distribution.json",
        "character-analysis.json",
        "coverage.json",
        "README.md",
        "artifacts.sha256",
    ];

    for file in &expected_reports {
        assert!(
            report_dir.join(file).exists(),
            "Frequency report file missing: {}",
            file
        );
    }

    // Verify artifacts.sha256 manifest covers data/build/frequencies.jsonl AND all report files
    let manifest_content = fs::read_to_string(report_dir.join("artifacts.sha256"))
        .expect("Failed to read artifacts.sha256");

    let freq_content = fs::read(&freq_jsonl).expect("Failed to read frequencies.jsonl");
    let freq_hash = format!("{:x}", Sha256::digest(freq_content));
    assert!(
        manifest_content.contains("data/build/frequencies.jsonl"),
        "Manifest must explicitly cover data/build/frequencies.jsonl"
    );
    assert!(
        manifest_content.contains(&freq_hash),
        "Manifest must contain valid SHA256 hash for data/build/frequencies.jsonl"
    );

    for file in &expected_reports[..6] {
        let content = fs::read(report_dir.join(file)).expect("Failed to read report file");
        let hash = format!("{:x}", Sha256::digest(&content));
        assert!(
            manifest_content.contains(&hash),
            "Manifest must contain hash for {}",
            file
        );
    }

    // Hash all outputs for Pass 1
    let pass1_freq_hash = format!("{:x}", Sha256::digest(fs::read(&freq_jsonl).unwrap()));
    let pass1_manifest_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(report_dir.join("artifacts.sha256")).unwrap())
    );

    // 3. Build frequencies (Pass 2 - Determinism check)
    let stats2 = build_corpus_frequencies(&root).expect("Build frequencies pass 2 must succeed");
    assert_eq!(stats1.records, stats2.records, "Records must match exactly");

    let pass2_freq_hash = format!("{:x}", Sha256::digest(fs::read(&freq_jsonl).unwrap()));
    let pass2_manifest_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(report_dir.join("artifacts.sha256")).unwrap())
    );

    assert_eq!(
        pass1_freq_hash, pass2_freq_hash,
        "frequencies.jsonl must be byte-for-byte identical across runs"
    );
    assert_eq!(
        pass1_manifest_hash, pass2_manifest_hash,
        "artifacts.sha256 must be byte-for-byte identical across runs"
    );
}

#[test]
fn test_provenance_ignores_unregistered_and_stale_files() {
    let _lock = PIPELINE_LOCK.lock().unwrap();
    let root = get_workspace_root();

    // Ensure corpus is imported
    import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");

    // 1. Create an unregistered corpus directory
    let unregistered_dir = root.join("data/imported/unregistered-corpus");
    fs::create_dir_all(&unregistered_dir).unwrap();
    let unregistered_file = unregistered_dir.join("notes.txt");
    fs::write(&unregistered_file, "unregistered_token_text_12345").unwrap();

    // 2. Create an undeclared stale file inside registered corpus folder
    let stale_file = root.join("data/imported/opensubtitles-kmr/stale_file.txt");
    fs::write(&stale_file, "stale_token_text_67890").unwrap();

    // 3. Run build_corpus_frequencies
    let stats = build_corpus_frequencies(&root).expect("Build frequencies failed");

    // Clean up injected test files
    let _ = fs::remove_file(&stale_file);
    let _ = fs::remove_dir_all(&unregistered_dir);

    // 4. Assert that neither unregistered nor stale tokens were included
    let has_unregistered = stats
        .records
        .iter()
        .any(|r| r.word.contains("unregistered"));
    let has_stale = stats.records.iter().any(|r| r.word.contains("stale"));

    assert!(
        !has_unregistered,
        "Unregistered corpus directory files must be IGNORED by build-frequencies"
    );
    assert!(
        !has_stale,
        "Undeclared stale files in corpus directory must be IGNORED by build-frequencies"
    );
}

#[test]
fn test_length_distribution_median_length_calculation() {
    let _lock = PIPELINE_LOCK.lock().unwrap();
    let root = get_workspace_root();

    import_corpus("opensubtitles-kmr", &root).expect("Corpus import failed");
    build_corpus_frequencies(&root).expect("Build frequencies failed");

    let report_path = root.join("data/reports/frequencies/length-distribution.json");
    let content =
        fs::read_to_string(&report_path).expect("Failed to read length-distribution.json");
    let json: serde_json::Value =
        serde_json::from_str(&content).expect("Failed to parse length-distribution.json");

    let min_length = json["min_length"].as_u64().unwrap();
    let max_length = json["max_length"].as_u64().unwrap();
    let median_length = json["median_length"].as_u64().unwrap();

    assert!(
        min_length > 0,
        "min_length must be positive for non-empty corpus"
    );
    assert!(max_length >= min_length, "max_length must be >= min_length");
    assert!(
        median_length >= min_length && median_length <= max_length,
        "median_length must be bounded between min_length ({}) and max_length ({}), got {}",
        min_length,
        max_length,
        median_length
    );
}
