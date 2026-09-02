//! Unit and Integration Tests for Corpus Quality Analysis Tooling.

use data_builder_lib::corpus::importer::import_corpus;
use data_builder_lib::corpus::partition::partition_corpora;
use data_builder_lib::corpus::quality::{analyze_corpus_quality, classify_technical_noise};
use data_builder_lib::review::validate_review_decisions;
use sha2::{Digest, Sha256};
use std::fs;
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
fn test_technical_noise_classification_rules() {
    assert_eq!(
        classify_technical_noise("https://example.com"),
        "url_email_fragment"
    );
    assert_eq!(
        classify_technical_noise("www.wikipedia.org"),
        "url_email_fragment"
    );
    assert_eq!(
        classify_technical_noise("user@example.com"),
        "url_email_fragment"
    );
    assert_eq!(classify_technical_noise("http"), "url_email_fragment");
    assert_eq!(classify_technical_noise("https"), "url_email_fragment");
    assert_eq!(classify_technical_noise("www"), "url_email_fragment");
    assert_eq!(classify_technical_noise("ftp"), "url_email_fragment");
    assert_eq!(classify_technical_noise("12345"), "pure_numeric");
    assert_eq!(classify_technical_noise("!!!???"), "no_letter_characters");
    assert_eq!(
        classify_technical_noise("<ref name=\"x\">"),
        "mediawiki_structural_remnant"
    );

    // Legitimate Kurmancî words & domain words must NOT be globally classified as technical noise
    assert_eq!(classify_technical_noise("wêne"), "none");
    assert_eq!(classify_technical_noise("şablon"), "none");
    assert_eq!(classify_technical_noise("dosye"), "none");
    assert_eq!(classify_technical_noise("kategorî"), "none");
    assert_eq!(classify_technical_noise("binêre"), "none");
    assert_eq!(classify_technical_noise("girêdanên"), "none");
    assert_eq!(classify_technical_noise("landkreis"), "none");
    assert_eq!(classify_technical_noise("franche"), "none");
    assert_eq!(classify_technical_noise("bourgogne"), "none");
}

#[test]
fn test_corpus_quality_analyzer_pipeline_tempdir() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = concat!(
        "Kurmancî zimanekî dewlemend e category:bajar 12345 wêne şablon\n",
        "Pirtûk xwendin kirin firavîn.\n",
        "Bonjour le monde! French prose is Latin script but not Kurmancî.\n"
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
license_url = "https://example.com"
url = "https://example.com"
version = "1.0.0"
description = "Test corpus for quality testing"
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

    import_corpus("test-corpus", root).expect("Import corpus failed");
    partition_corpora(root).expect("Partition corpora failed");

    let metrics =
        analyze_corpus_quality(root, "test-corpus").expect("Analyze corpus quality failed");

    assert_eq!(metrics.corpus_id, "test-corpus");
    assert!(metrics.total_documents > 0);
    assert!(metrics.total_lexical_tokens > 0);
    assert!(metrics.unique_lexical_tokens > 0);

    // Verify script distribution detects Latin script tokens
    assert!(metrics.script_distribution.latin_tokens > 0);

    // Verify raw source quality counts detect structural remnants (<ref or category:)
    assert!(metrics.source_quality.mediawiki_structural_remnants > 0);

    // Check generated report files in data/reports/corpus-quality/test-corpus/
    let report_dir = root.join("data/reports/corpus-quality/test-corpus");
    assert!(report_dir.join("summary.json").exists());
    assert!(report_dir.join("source-quality-summary.json").exists());
    assert!(report_dir.join("document-quality-summary.json").exists());
    assert!(report_dir.join("top-tokens.jsonl").exists());
    assert!(report_dir.join("top-oov-tokens.jsonl").exists());
    assert!(report_dir.join("artifacts.sha256").exists());
}

#[test]
fn test_corpus_quality_stale_canonical_provenance_rejection() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    let sample_docs = "Kurmancî text xwendin.\n";
    fs::write(&corpus_txt, sample_docs).unwrap();
    let corpus_sha256 = format!("{:x}", Sha256::digest(sample_docs.as_bytes()));

    let corpora_toml_path = root.join("data/source-registry/corpora.toml");
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
    fs::write(&corpora_toml_path, &corpora_toml).unwrap();

    validate_review_decisions("kurdish-hunspell-kmr", root).unwrap();

    import_corpus("test-corpus", root).unwrap();

    // Mutate corpora.toml after importing canonical documents
    let mutated = corpora_toml.replace("1.0.0", "1.0.1");
    fs::write(&corpora_toml_path, mutated).unwrap();

    let err = analyze_corpus_quality(root, "test-corpus").unwrap_err();
    assert!(
        err.contains("Canonical manifest verification failed") || err.contains("mismatch"),
        "analyze_corpus_quality must reject stale canonical manifest: {}",
        err
    );
}

#[test]
fn test_markup_dominated_and_low_content_anomalies() {
    let tmp = tempdir().expect("Failed to create tempdir");
    let root = tmp.path();

    copy_repo_fixtures(root);

    let orig_dir = root.join("data/original/test-corpus");
    fs::create_dir_all(&orig_dir).unwrap();

    let corpus_txt = orig_dir.join("corpus.txt");
    // Document 1: 6 raw tokens, 6 are structural/numeric/URL noise (100% noise >= 50% threshold)
    // Clean lexical tokens: < 5 threshold (low content)
    let sample_docs = "<ref> 12345 67890 11111 22222 http://example.com\n";
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

    let metrics = analyze_corpus_quality(root, "test-corpus").unwrap();

    assert!(
        metrics
            .document_anomalies
            .technical_markup_dominated_documents
            > 0,
        "Markup-heavy raw text must be detected as technical_markup_dominated_documents"
    );
    assert!(
        metrics.document_anomalies.low_content_documents > 0,
        "Low prose content document must be detected as low_content_documents"
    );
}
