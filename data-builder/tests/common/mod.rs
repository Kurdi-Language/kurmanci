//! Test-only corpus fixture. The corpus pipeline (import, inventory, partition, frequencies,
//! n-grams, next-word evaluation) is exercised on an isolated temporary root that carries its
//! own registry with exactly one tracked corpus, `test-corpus`, whose text is
//! `tests/fixtures/synthetic-corpus.txt`. The text is bytes for exercising deterministic code
//! paths: it makes no linguistic claim and never reaches the production registry, frequency
//! inputs, review data, packs, language models or release provenance.
#![allow(dead_code)]

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub const TEST_CORPUS_ID: &str = "test-corpus";

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

pub fn fixture_corpus_text() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic-corpus.txt"),
    )
    .expect("tests/fixtures/synthetic-corpus.txt")
}

fn copy_file(ws: &Path, root: &Path, rel: &str) {
    let src = ws.join(rel);
    if src.is_file() {
        let dst = root.join(rel);
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::copy(&src, &dst).unwrap();
    }
}

/// An isolated root: the repository's seed lexicon, source registry, pack policy, n-gram
/// configuration and next-word evaluation cases, plus a corpus registry that lists only
/// `test-corpus` (tracked, one file) and the fixture text under `data/original/` and the
/// legacy `data/imported/` location the frequency and n-gram builders read.
pub fn synthetic_corpus_root() -> TempDir {
    let ws = workspace_root();
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    for rel in [
        "data/source-registry/sources.toml",
        "data/reviewed/lexicon.jsonl",
        "data/pack-policy.toml",
        "data-builder/config/ngrams.toml",
        "data-builder/config/builder.toml",
        "evaluation/next-word/cases.jsonl",
        "evaluation/next-word/trigram-cases.jsonl",
    ] {
        copy_file(&ws, root, rel);
    }
    let text = fixture_corpus_text();
    let sha = format!("{:x}", Sha256::digest(text.as_bytes()));
    let registry = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpora.toml"),
    )
    .expect("tests/fixtures/corpora.toml");
    assert!(
        registry.contains(&format!("sha256 = \"{}\"", sha)),
        "tests/fixtures/corpora.toml must pin the SHA-256 of synthetic-corpus.txt ({})",
        sha
    );
    for dir in ["data/original", "data/imported"] {
        let d = root.join(dir).join(TEST_CORPUS_ID);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("corpus.txt"), &text).unwrap();
    }
    fs::write(root.join("data/source-registry/corpora.toml"), registry).unwrap();
    tmp
}
