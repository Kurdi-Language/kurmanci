//! Integration Tests for Vocabulary Evidence Pipeline & OOV Queue Generation.

use data_builder_lib::corpus::frequency::build_corpus_train_frequencies;
use data_builder_lib::corpus::importer::import_corpus;
use data_builder_lib::corpus::partition::partition_corpora;
use data_builder_lib::corpus::quality::classify_technical_noise;
use data_builder_lib::corpus::vocabulary_evidence::{
    build_vocabulary_evidence, OovCandidateRecord,
};
use data_builder_lib::review::validate_review_decisions;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir)
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn copy_repo_fixtures(root: &Path) {
    let ws_root = get_workspace_root();
    let dirs_to_copy = [
        "data/source-registry",
        "data/raw",
        "data/original",
        "data/imported",
        "data/reviewed",
        "data/reports",
        "data/review-decisions",
        "data/review-queues",
        "data/review-batches",
    ];

    for d in &dirs_to_copy {
        let src = ws_root.join(d);
        if src.exists() {
            copy_dir_all(&src, root.join(d)).unwrap();
        }
    }

    if ws_root.join("data/pack-policy.toml").exists() {
        fs::copy(
            ws_root.join("data/pack-policy.toml"),
            root.join("data/pack-policy.toml"),
        )
        .unwrap();
    }
}

#[test]
fn test_multi_corpus_evidence_isolation() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

    let orig_a = root.join("data/original/corpus-a");
    let orig_b = root.join("data/original/corpus-b");
    fs::create_dir_all(&orig_a).unwrap();
    fs::create_dir_all(&orig_b).unwrap();

    let doc_a = (1..=20)
        .map(|i| format!("alphacandidateonly doc{} text.", i))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let doc_b = (1..=20)
        .map(|i| format!("betacandidateonly doc{} text.", i))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    fs::write(orig_a.join("corpus.txt"), &doc_a).unwrap();
    fs::write(orig_b.join("corpus.txt"), &doc_b).unwrap();

    let sha_a = format!("{:x}", Sha256::digest(doc_a.as_bytes()));
    let sha_b = format!("{:x}", Sha256::digest(doc_b.as_bytes()));

    let corpora_toml = format!(
        r#"schema_version = "corpus-registry-v1"

[[corpora]]
corpus_id = "corpus-a"
corpus_name = "Corpus A"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://example.com"
url = "https://example.com"
version = "1.0.0"
description = "Corpus A"
attribution = "A"
notes = "A"
document_format = "one-document-per-line"

[[corpora.files]]
path = "data/original/corpus-a/corpus.txt"
sha256 = "{}"

[[corpora]]
corpus_id = "corpus-b"
corpus_name = "Corpus B"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://example.com"
url = "https://example.com"
version = "1.0.0"
description = "Corpus B"
attribution = "B"
notes = "B"
document_format = "one-document-per-line"

[[corpora.files]]
path = "data/original/corpus-b/corpus.txt"
sha256 = "{}"
"#,
        sha_a, sha_b
    );
    fs::write(root.join("data/source-registry/corpora.toml"), corpora_toml).unwrap();

    validate_review_decisions("kurdish-hunspell-kmr", root).unwrap();

    import_corpus("corpus-a", root).unwrap();
    import_corpus("corpus-b", root).unwrap();
    partition_corpora(root).unwrap();
    build_corpus_train_frequencies(root).unwrap();

    // Test Corpus A Evidence
    let summary_a = build_vocabulary_evidence(root, "corpus-a").unwrap();
    assert_eq!(summary_a.corpus_id, "corpus-a");

    let queue_a_path =
        root.join("data/reports/vocabulary-evidence/corpus-a/oov-review-queue.jsonl");
    let queue_a_file = File::open(&queue_a_path).unwrap();
    let cands_a: Vec<OovCandidateRecord> = BufReader::new(queue_a_file)
        .lines()
        .map_while(Result::ok)
        .map(|l| serde_json::from_str(&l).unwrap())
        .collect();

    assert!(cands_a.iter().any(|c| c.token == "alphacandidateonly"));
    assert!(!cands_a.iter().any(|c| c.token == "betacandidateonly"));
    for cand in &cands_a {
        assert_eq!(cand.corpus_id, "corpus-a");
    }

    // Test Corpus B Evidence
    let summary_b = build_vocabulary_evidence(root, "corpus-b").unwrap();
    assert_eq!(summary_b.corpus_id, "corpus-b");

    let queue_b_path =
        root.join("data/reports/vocabulary-evidence/corpus-b/oov-review-queue.jsonl");
    let queue_b_file = File::open(&queue_b_path).unwrap();
    let cands_b: Vec<OovCandidateRecord> = BufReader::new(queue_b_file)
        .lines()
        .map_while(Result::ok)
        .map(|l| serde_json::from_str(&l).unwrap())
        .collect();

    assert!(cands_b.iter().any(|c| c.token == "betacandidateonly"));
    assert!(!cands_b.iter().any(|c| c.token == "alphacandidateonly"));
    for cand in &cands_b {
        assert_eq!(cand.corpus_id, "corpus-b");
    }
}

#[test]
fn test_train_jsonl_mutation_rejection() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = (1..=20)
        .map(|i| format!("testword{} text.", i))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&corpus_txt, &sample_docs).unwrap();
    let corpus_sha256 = format!("{:x}", Sha256::digest(sample_docs.as_bytes()));

    let corpora_toml = format!(
        r#"schema_version = "corpus-registry-v1"

[[corpora]]
corpus_id = "test-corpus"
corpus_name = "Test Corpus"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://example.com"
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
    fs::write(root.join("data/source-registry/corpora.toml"), corpora_toml).unwrap();

    validate_review_decisions("kurdish-hunspell-kmr", root).unwrap();

    import_corpus("test-corpus", root).unwrap();
    partition_corpora(root).unwrap();
    build_corpus_train_frequencies(root).unwrap();

    // Mutate train.jsonl directly without updating manifest
    let train_path = root.join("data/build/corpus-partitions/train.jsonl");
    let current_train = fs::read_to_string(&train_path).unwrap();
    let mutated_train = current_train + "\n// mutated\n";
    fs::write(&train_path, mutated_train).unwrap();

    let err = build_vocabulary_evidence(root, "test-corpus").unwrap_err();
    assert!(
        err.contains("train_partition_sha256 mismatch"),
        "Pipeline must reject train.jsonl content mutation: {}",
        err
    );
}

#[test]
fn test_frequency_manifest_policy_mismatch_rejection() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = (1..=20)
        .map(|i| format!("testword{} text.", i))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&corpus_txt, &sample_docs).unwrap();
    let corpus_sha256 = format!("{:x}", Sha256::digest(sample_docs.as_bytes()));

    let corpora_toml = format!(
        r#"schema_version = "corpus-registry-v1"

[[corpora]]
corpus_id = "test-corpus"
corpus_name = "Test Corpus"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://example.com"
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
    fs::write(root.join("data/source-registry/corpora.toml"), corpora_toml).unwrap();

    validate_review_decisions("kurdish-hunspell-kmr", root).unwrap();

    import_corpus("test-corpus", root).unwrap();
    partition_corpora(root).unwrap();
    build_corpus_train_frequencies(root).unwrap();

    // Mutate frequency_manifest.json partition_policy_version
    let freq_manifest_path = root.join("data/build/frequency_manifest.json");
    let content = fs::read_to_string(&freq_manifest_path).unwrap();
    let mutated = content.replace("kurmanci-partition-v1", "stale-partition-v0");
    fs::write(&freq_manifest_path, mutated).unwrap();

    let err = build_vocabulary_evidence(root, "test-corpus").unwrap_err();
    assert!(
        err.contains("partition_policy_version mismatch"),
        "Pipeline must reject wrong partition_policy_version: {}",
        err
    );
}

#[test]
fn test_tokenizer_url_flow_excludes_protocol_markers() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = (1..=20)
        .map(|i| {
            format!(
                "https://example.com www.ku.org wêne şablon dosye testtoken{} text.",
                i
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&corpus_txt, &sample_docs).unwrap();
    let corpus_sha256 = format!("{:x}", Sha256::digest(sample_docs.as_bytes()));

    let corpora_toml = format!(
        r#"schema_version = "corpus-registry-v1"

[[corpora]]
corpus_id = "test-corpus"
corpus_name = "Test Corpus"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://example.com"
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
    fs::write(root.join("data/source-registry/corpora.toml"), corpora_toml).unwrap();

    validate_review_decisions("kurdish-hunspell-kmr", root).unwrap();

    import_corpus("test-corpus", root).unwrap();
    partition_corpora(root).unwrap();
    build_corpus_train_frequencies(root).unwrap();

    let _summary = build_vocabulary_evidence(root, "test-corpus").unwrap();

    let queue_path =
        root.join("data/reports/vocabulary-evidence/test-corpus/oov-review-queue.jsonl");
    let queue_file = File::open(&queue_path).unwrap();
    let queue: Vec<OovCandidateRecord> = BufReader::new(queue_file)
        .lines()
        .map_while(Result::ok)
        .map(|l| serde_json::from_str(&l).unwrap())
        .collect();

    // Protocol markers must NOT be in eligible review queue
    assert!(!queue.iter().any(|c| c.token == "https"));
    assert!(!queue.iter().any(|c| c.token == "http"));
    assert!(!queue.iter().any(|c| c.token == "www"));

    // Synthetic OOV tokens MUST be in eligible review queue
    assert!(queue.iter().any(|c| c.token.starts_with("testtoken")));

    // Ordinary lexical words (wêne, şablon, dosye, kategorî, binêre, category, file, references) are NOT classified as technical noise
    assert_eq!(classify_technical_noise("wêne"), "none");
    assert_eq!(classify_technical_noise("şablon"), "none");
    assert_eq!(classify_technical_noise("dosye"), "none");
    assert_eq!(classify_technical_noise("kategorî"), "none");
    assert_eq!(classify_technical_noise("binêre"), "none");
    assert_eq!(classify_technical_noise("category"), "none");
    assert_eq!(classify_technical_noise("file"), "none");
    assert_eq!(classify_technical_noise("references"), "none");
}

#[test]
fn test_frequencies_jsonl_mutation_rejection() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = (1..=20)
        .map(|i| format!("testword{} text.", i))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&corpus_txt, &sample_docs).unwrap();
    let corpus_sha256 = format!("{:x}", Sha256::digest(sample_docs.as_bytes()));

    let corpora_toml = format!(
        r#"schema_version = "corpus-registry-v1"

[[corpora]]
corpus_id = "test-corpus"
corpus_name = "Test Corpus"
language = "ku-Latn"
license = "CC0-1.0"
license_url = "https://example.com"
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
    fs::write(root.join("data/source-registry/corpora.toml"), corpora_toml).unwrap();

    validate_review_decisions("kurdish-hunspell-kmr", root).unwrap();

    import_corpus("test-corpus", root).unwrap();
    partition_corpora(root).unwrap();
    build_corpus_train_frequencies(root).unwrap();

    // Mutate frequencies.jsonl directly without updating manifest
    let freq_path = root.join("data/build/frequencies.jsonl");
    let current_freq = fs::read_to_string(&freq_path).unwrap();
    let mutated_freq = current_freq + "\n// mutated\n";
    fs::write(&freq_path, mutated_freq).unwrap();

    let err = build_vocabulary_evidence(root, "test-corpus").unwrap_err();
    assert!(
        err.contains("frequencies_sha256 mismatch"),
        "Pipeline must reject frequencies.jsonl content mutation: {}",
        err
    );
}

#[test]
fn test_fingerprint_determinism_and_mutation_sensitivity() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

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
license_url = "https://example.com"
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
    fs::write(root.join("data/source-registry/corpora.toml"), corpora_toml).unwrap();

    validate_review_decisions("kurdish-hunspell-kmr", root).unwrap();

    import_corpus("test-corpus", root).unwrap();
    partition_corpora(root).unwrap();
    build_corpus_train_frequencies(root).unwrap();

    let summary1 = build_vocabulary_evidence(root, "test-corpus").unwrap();
    let summary2 = build_vocabulary_evidence(root, "test-corpus").unwrap();

    assert_eq!(
        summary1.provenance.experimental_lexicon_fingerprint,
        summary2.provenance.experimental_lexicon_fingerprint,
        "Identical inputs must produce identical fingerprint"
    );

    // Mutate entry word in data/reviewed/lexicon.jsonl
    let seed_file = root.join("data/reviewed/lexicon.jsonl");
    let content = fs::read_to_string(&seed_file).unwrap();
    let mutated_content = content.replacen("\"word\": \"bo\"", "\"word\": \"bomutated\"", 1);
    fs::write(&seed_file, mutated_content).unwrap();

    let summary3 = build_vocabulary_evidence(root, "test-corpus").unwrap();
    assert_ne!(
        summary1.provenance.experimental_lexicon_fingerprint,
        summary3.provenance.experimental_lexicon_fingerprint,
        "Lexical membership identity change must alter the experimental lexicon fingerprint"
    );
}

#[test]
fn test_stale_partition_canonical_manifest_rejection() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

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
license_url = "https://example.com"
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
    fs::write(root.join("data/source-registry/corpora.toml"), corpora_toml).unwrap();

    validate_review_decisions("kurdish-hunspell-kmr", root).unwrap();

    import_corpus("test-corpus", root).unwrap();
    partition_corpora(root).unwrap();
    build_corpus_train_frequencies(root).unwrap();

    // Re-partition or mutate partition manifest without rebuilding frequencies
    let part_manifest_path = root.join("data/build/corpus-partitions/manifest.json");
    let content = fs::read_to_string(&part_manifest_path).unwrap();
    let mutated = content.replace("kurmanci-partition-v1", "stale-partitions-v0");
    fs::write(&part_manifest_path, mutated).unwrap();

    let err = build_vocabulary_evidence(root, "test-corpus").unwrap_err();
    assert!(
        err.contains("partition_manifest_sha256 mismatch") || err.contains("mismatch"),
        "Pipeline must reject stale partition manifest: {}",
        err
    );
}
