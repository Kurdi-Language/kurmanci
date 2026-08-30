//! Test train-partition frequency builder with isolated tempdir and deterministic tiny fixtures.

use data_builder_lib::corpus::importer::import_corpus;
use data_builder_lib::corpus::partition::partition_corpora;
use data_builder_lib::{build_corpus_frequencies, build_corpus_train_frequencies};
use sha2::{Digest, Sha256};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_train_frequency_builder_parity_and_invariants() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    // 1. Prepare registry and source file in tempdir
    let reg_dir = root.join("data/source-registry");
    fs::create_dir_all(&reg_dir).unwrap();

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = concat!(
        "Pêşkeş Kurmancî dil xwendin kirin.\n",
        "Kurmancî xwendin girîn firavîn.\n",
        "Pêşkeş xwendin dil.\n"
    );
    fs::write(&corpus_txt, sample_docs).unwrap();
    let corpus_sha256 = format!("{:x}", Sha256::digest(sample_docs.as_bytes()));

    let corpora_toml = format!(
        r#"schema_version = "corpus-registry-v1"

[[corpora]]
corpus_id = "test-corpus"
corpus_name = "Test Corpus"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://creativecommons.org/publicdomain/zero/1.0/"
url = "https://example.com"
version = "1.0.0"
description = "Test corpus for unit testing"
attribution = "Test"
notes = "Test"
document_format = "one-document-per-line"

[[corpora.files]]
path = "data/original/test-corpus/corpus.txt"
sha256 = "{}"
"#,
        corpus_sha256
    );
    fs::write(reg_dir.join("corpora.toml"), corpora_toml).unwrap();

    // Copy to data/imported/ for legacy build_corpus_frequencies check
    let imported_dir = root.join("data/imported/test-corpus");
    fs::create_dir_all(&imported_dir).unwrap();
    fs::copy(&corpus_txt, imported_dir.join("corpus.txt")).unwrap();

    // 2. Import corpus
    import_corpus("test-corpus", root).expect("Failed to import test corpus");

    // 3. Run partition-corpora
    let part_summary = partition_corpora(root).expect("Failed to partition corpora");
    assert!(part_summary.train_documents > 0);

    // 4. Run existing whole-corpus frequency builder
    let orig_stats = build_corpus_frequencies(root).expect("Failed to build corpus frequencies");

    // 5. Run new train-partition frequency builder
    let train_stats =
        build_corpus_train_frequencies(root).expect("Failed to build train frequencies");

    println!("\n=== TEMPDIR FREQUENCY BUILDER PARITY CHECK ===");
    println!("Whole Corpus Builder (orig):");
    println!("  Total Documents: {}", orig_stats.total_documents);
    println!("  Total Tokens:    {}", orig_stats.total_tokens);
    println!("  Unique Records:  {}", orig_stats.records.len());

    println!("\nTrain-Partition Canonical Builder (new):");
    println!("  Total Documents: {}", train_stats.total_documents);
    println!("  Total Tokens:    {}", train_stats.total_tokens);
    println!("  Unique Records:  {}", train_stats.records.len());

    // Invariants:
    assert!(
        train_stats.total_documents > 0,
        "Train partition documents must be > 0!"
    );
    assert!(
        train_stats.total_tokens > 0,
        "Train partition tokens must be > 0!"
    );
    assert!(
        !train_stats.records.is_empty(),
        "Train partition frequency records must not be empty!"
    );
}

#[test]
fn test_train_frequency_builder_stale_manifest_rejection() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    let reg_dir = root.join("data/source-registry");
    fs::create_dir_all(&reg_dir).unwrap();

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = "Kurmancî text xwendin.\n";
    fs::write(&corpus_txt, sample_docs).unwrap();
    let corpus_sha256 = format!("{:x}", Sha256::digest(sample_docs.as_bytes()));

    let corpora_toml = format!(
        r#"schema_version = "corpus-registry-v1"

[[corpora]]
corpus_id = "test-corpus"
corpus_name = "Test Corpus"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://creativecommons.org/publicdomain/zero/1.0/"
url = "https://example.com"
version = "1.0.0"
description = "Test corpus"
attribution = "Test"
notes = "Test"
document_format = "one-document-per-line"

[[corpora.files]]
path = "data/original/test-corpus/corpus.txt"
sha256 = "{}"
"#,
        corpus_sha256
    );
    fs::write(reg_dir.join("corpora.toml"), corpora_toml).unwrap();

    import_corpus("test-corpus", root).unwrap();
    partition_corpora(root).unwrap();

    // Mutate data/imported-canonical/manifest.json so SHA-256 no longer matches partition manifest
    let canonical_manifest = root.join("data/imported-canonical/manifest.json");
    fs::write(&canonical_manifest, "{\"stale\": true}").unwrap();

    let err = build_corpus_train_frequencies(root).unwrap_err();
    assert!(
        err.contains("SHA-256 mismatch") || err.contains("Canonical manifest verification failed")
    );
}

#[test]
fn test_train_frequency_builder_stale_corpora_toml_rejection() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    let reg_dir = root.join("data/source-registry");
    fs::create_dir_all(&reg_dir).unwrap();

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = "Kurmancî text xwendin.\n";
    fs::write(&corpus_txt, sample_docs).unwrap();
    let corpus_sha256 = format!("{:x}", Sha256::digest(sample_docs.as_bytes()));

    let corpora_toml_path = reg_dir.join("corpora.toml");
    let corpora_toml = format!(
        r#"schema_version = "corpus-registry-v1"

[[corpora]]
corpus_id = "test-corpus"
corpus_name = "Test Corpus"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://creativecommons.org/publicdomain/zero/1.0/"
url = "https://example.com"
version = "1.0.0"
description = "Test corpus"
attribution = "Test"
notes = "Test"
document_format = "one-document-per-line"

[[corpora.files]]
path = "data/original/test-corpus/corpus.txt"
sha256 = "{}"
"#,
        corpus_sha256
    );
    fs::write(&corpora_toml_path, &corpora_toml).unwrap();

    import_corpus("test-corpus", root).unwrap();
    partition_corpora(root).unwrap();

    // Mutate data/source-registry/corpora.toml in a provenance-relevant way after import+partition
    let mutated_corpora_toml = corpora_toml.replace("1.0.0", "1.0.1");
    fs::write(&corpora_toml_path, mutated_corpora_toml).unwrap();

    let err = build_corpus_train_frequencies(root).unwrap_err();
    assert!(
        err.contains("Registry SHA-256 mismatch")
            || err.contains("Canonical manifest verification failed"),
        "Must reject stale corpora.toml input: {}",
        err
    );
}
