//! Regression test: a language model is built from exactly its own corpus's canonical TRAIN
//! documents. With two corpora in the same TRAIN partition, corpus B's distinctive tokens and
//! n-grams must never appear in corpus A's model, changing B alone must leave A's model
//! byte-identical, and changing A must change A's model.

use data_builder_lib::corpus::partition::{partition_corpora, PartitionDocumentRecord};
use data_builder_lib::import_all_corpora;
use data_builder_lib::pack::language_model::{
    build_language_model, load_language_model, LanguageModelBuildManifest, LanguageModelManifest,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Corpus A documents: only the tokens `ez baş im` plus a per-document filler word.
fn corpus_a_text(docs: usize) -> String {
    let mut text = String::new();
    for i in 0..docs {
        writeln!(text, "ez baş im rêza{}.", i).unwrap();
    }
    text
}

/// Corpus B documents: only the tokens `nenas xerîb e` plus a per-document filler word.
fn corpus_b_text(docs: usize) -> String {
    let mut text = String::new();
    for i in 0..docs {
        writeln!(text, "nenas xerîb e rêzb{}.", i).unwrap();
    }
    text
}

fn write_registry(root: &Path, a_text: &str, b_text: &str) {
    fs::create_dir_all(root.join("data/original/corpus-a")).unwrap();
    fs::create_dir_all(root.join("data/original/corpus-b")).unwrap();
    fs::write(root.join("data/original/corpus-a/corpus.txt"), a_text).unwrap();
    fs::write(root.join("data/original/corpus-b/corpus.txt"), b_text).unwrap();
    let entry = |id: &str, name: &str, sha: &str| {
        format!(
            r#"
[[corpora]]
corpus_id = "{id}"
corpus_name = "{name}"
language = "ku-Latn"
license = "CC BY-SA 4.0"
license_spdx = "CC-BY-SA-4.0"
license_url = "https://creativecommons.org/licenses/by-sa/4.0/"
url = "https://example.org/{id}"
version = "1"
description = "{name} test corpus"
attribution = "{name} contributors"
notes = "test"
document_format = "one-document-per-line"

[[corpora.files]]
path = "data/original/{id}/corpus.txt"
sha256 = "{sha}"
"#
        )
    };
    let toml = format!(
        "schema_version = \"corpus-registry-v1\"\n{}{}",
        entry("corpus-a", "Corpus A", &sha256_hex(a_text.as_bytes())),
        entry("corpus-b", "Corpus B", &sha256_hex(b_text.as_bytes())),
    );
    fs::create_dir_all(root.join("data/source-registry")).unwrap();
    fs::write(root.join("data/source-registry/corpora.toml"), toml).unwrap();
}

/// Packs, sources and review environment so that the authoritative vocabulary resolves to
/// {baş, e, ez, im, nenas, xerîb}: both corpora's tokens are eligible for any model, so only
/// corpus scoping can keep corpus B out of corpus A's model.
fn prepare_pack_fixture(root: &Path) {
    fs::create_dir_all(root.join("data/reviewed")).unwrap();
    let mut seed = String::new();
    for (w, pos) in [
        ("ez", "pronoun"),
        ("baş", "adjective"),
        ("im", "verb"),
        ("nenas", "adjective"),
        ("xerîb", "adjective"),
        ("e", "verb"),
    ] {
        seed.push_str(&format!(
            r#"{{"word":"{w}","lemma":"{w}","normalized":"{w}","part_of_speech":"{pos}","frequency":0,"status":"approved","variants":[],"sources":["manual-seed"],"regions":["general"]}}"#
        ));
        seed.push('\n');
    }
    fs::write(root.join("data/reviewed/lexicon.jsonl"), seed).unwrap();
    fs::write(
        root.join("data/source-registry/sources.toml"),
        r#"schema_version = "source-registry-v1"

[[sources]]
source_id = "manual-seed"
source_name = "Kurmancî Manually Reviewed Seed Lexicon"
author = "Kurmancî Language Platform Contributors"
license = "Apache-2.0"
license_url = "https://www.apache.org/licenses/LICENSE-2.0"
url = "https://github.com/Kurdi-Language/kurmanci"
version = "0.1.0"
redistribution = "allowed"
notes = "Seed"

[[sources]]
source_id = "kurdish-hunspell-kmr"
source_name = "KurdishHunspell"
source_type = "hunspell"
language = "ku-Latn"
script = "Latn"
author = "KurdishHunspell Team"
license = "CC-BY-SA-4.0"
license_url = "https://spdx.org/licenses/CC-BY-SA-4.0"
url = "https://github.com/hunspell/kmr"
version = "88131d6878ef7fa3ee114aa554adc385ff85b44c"
redistribution = "allowed"
notes = "Test"
"#,
    )
    .unwrap();
    fs::write(
        root.join("data/pack-policy.toml"),
        r#"schema_version = "pack-policy-v1"
default_pack = "seed"

[packs.seed]
description = "Seed"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.reviewed]
description = "Reviewed"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.experimental-full]
description = "Experimental"
opt_in = true
allow_as_default = false
model_profile = "none"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("data-builder/config")).unwrap();
    fs::write(
        root.join("data-builder/config/ngrams.toml"),
        "[bigram]\nminimum_count = 1\nmaximum_predictions_per_context = 16\n\n[trigram]\nminimum_count = 1\nmaximum_predictions_per_context = 12\n",
    )
    .unwrap();

    // Empty Hunspell review environment (no queue entries, no decisions, validated reports).
    let source_id = "kurdish-hunspell-kmr";
    let dec_dir = root.join(format!("data/review-decisions/{}", source_id));
    fs::create_dir_all(&dec_dir).unwrap();
    fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();
    let queue_dir = root.join(format!("data/review-queues/{}", source_id));
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("artifacts.sha256"), "").unwrap();
    let rep_dir = root.join("data/reports/controlled-lexicon-review");
    fs::create_dir_all(&rep_dir).unwrap();
    let empty_sha = sha256_hex(b"");
    let summary = format!(
        concat!(
            r#"{{"schema_version":"controlled-review-report-v1","source_id":"kurdish-hunspell-kmr","#,
            r#""total_decisions_count":0,"approved_count":0,"approved_with_metadata_change_count":0,"#,
            r#""rejected_from_default_count":0,"experimental_only_count":0,"unresolved_count":0,"#,
            r#""orphan_decisions_count":0,"decision_file_sha256":"{e}","provenance":{{"decisions_sha256":"{e}","#,
            r#""queue_manifest_sha256":"{e}","source_revision":"1.0","imported_lexicon_sha256":"{e}"}}}}"#
        ),
        e = empty_sha
    );
    fs::write(rep_dir.join("summary.json"), &summary).unwrap();
    let mut lines = vec![format!(
        "{}  data/reports/controlled-lexicon-review/summary.json",
        sha256_hex(summary.as_bytes())
    )];
    for name in [
        "approved.jsonl",
        "rejected-from-default.jsonl",
        "experimental-only.jsonl",
        "unresolved.jsonl",
        "orphan-decisions.jsonl",
        "metadata-changes.jsonl",
    ] {
        fs::write(rep_dir.join(name), "").unwrap();
        lines.push(format!(
            "{}  data/reports/controlled-lexicon-review/{}",
            empty_sha, name
        ));
    }
    fs::write(rep_dir.join("artifacts.sha256"), lines.join("\n") + "\n").unwrap();
}

/// Canonical TRAIN representatives per corpus in the current partition.
fn canonical_train_documents(root: &Path) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let text = fs::read_to_string(root.join("data/build/corpus-partitions/train.jsonl")).unwrap();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let r: PartitionDocumentRecord = serde_json::from_str(line).unwrap();
        if r.corpus_id == r.canonical_corpus_id && r.document_id == r.canonical_document_id {
            *counts.entry(r.corpus_id).or_insert(0) += 1;
        }
    }
    counts
}

fn import_and_partition(root: &Path) {
    import_all_corpora(root).unwrap();
    partition_corpora(root).unwrap();
}

fn content_hashes(manifest: &LanguageModelManifest) -> BTreeMap<String, String> {
    manifest
        .files
        .iter()
        .map(|f| (f.path.clone(), f.sha256.clone()))
        .collect()
}

fn read_build_manifest(root: &Path, corpus_id: &str) -> LanguageModelBuildManifest {
    serde_json::from_slice(
        &fs::read(
            root.join("data/build/language-model")
                .join(corpus_id)
                .join("build-manifest.json"),
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn test_language_model_is_scoped_to_its_own_corpus() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    let a_text = corpus_a_text(30);
    let b_text = corpus_b_text(30);
    write_registry(root, &a_text, &b_text);
    prepare_pack_fixture(root);
    import_and_partition(root);

    let train_docs = canonical_train_documents(root);
    let a_train = train_docs["corpus-a"];
    let b_train = train_docs["corpus-b"];
    assert!(
        a_train > 0 && b_train > 0,
        "both corpora must have TRAIN documents: {:?}",
        train_docs
    );

    // Corpus A's model contains only corpus A statistics.
    let manifest_a = build_language_model(root, "corpus-a", 1, 1).unwrap();
    assert_eq!(manifest_a.model_id, "corpus-a-1");
    assert_eq!(manifest_a.contributing_corpora, vec!["corpus-a"]);
    assert_eq!(manifest_a.train_document_count, a_train);
    assert_eq!(manifest_a.licensing.corpus_name, "Corpus A");
    assert_eq!(manifest_a.licensing.license_spdx, "CC-BY-SA-4.0");
    let model_a = load_language_model(root, "corpus-a-1").unwrap();
    let a_words: BTreeSet<&str> = model_a.unigrams.keys().map(|s| s.as_str()).collect();
    assert!(a_words.contains("ez") && a_words.contains("baş") && a_words.contains("im"));
    for foreign in ["nenas", "xerîb", "e"] {
        assert!(
            !a_words.contains(foreign),
            "corpus B token {:?} leaked into corpus A unigrams",
            foreign
        );
        assert!(
            !model_a
                .bigrams
                .iter()
                .any(|b| b.previous == foreign || b.next == foreign),
            "corpus B token {:?} leaked into corpus A bigrams",
            foreign
        );
        assert!(
            !model_a
                .trigrams
                .iter()
                .any(|t| t.previous_2 == foreign || t.previous_1 == foreign || t.next == foreign),
            "corpus B token {:?} leaked into corpus A trigrams",
            foreign
        );
    }
    assert!(model_a
        .bigrams
        .iter()
        .any(|b| b.previous == "ez" && b.next == "baş"));
    // The unigram document counts are corpus A's TRAIN documents, not the whole partition.
    assert_eq!(model_a.unigrams["ez"].document_count as usize, a_train);
    let build_a = read_build_manifest(root, "corpus-a");
    assert_eq!(build_a.train_document_count, a_train);
    assert_eq!(build_a.partition_train_documents, a_train + b_train);
    assert_eq!(
        build_a.train_document_set_sha256,
        manifest_a.train_document_set_sha256
    );
    assert_eq!(
        build_a.train_bigrams_sha256,
        manifest_a.train_bigrams_sha256
    );
    assert_eq!(
        sha256_hex(
            &fs::read(root.join("data/build/language-model/corpus-a/build-manifest.json")).unwrap()
        ),
        manifest_a.build_manifest_sha256
    );

    // Corpus B's model, built from the same partition, is B-only in the same way.
    let manifest_b = build_language_model(root, "corpus-b", 1, 1).unwrap();
    assert_eq!(manifest_b.contributing_corpora, vec!["corpus-b"]);
    assert_eq!(manifest_b.train_document_count, b_train);
    let model_b = load_language_model(root, "corpus-b-1").unwrap();
    let b_words: BTreeSet<&str> = model_b.unigrams.keys().map(|s| s.as_str()).collect();
    assert!(b_words.contains("nenas") && !b_words.contains("ez"));
    assert_ne!(
        manifest_a.train_document_set_sha256,
        manifest_b.train_document_set_sha256
    );

    // Changing corpus B alone leaves corpus A's statistics byte-identical.
    let hashes_a_before = content_hashes(&manifest_a);
    let b_text_2 = corpus_b_text(30) + "nenas xerîb e ez.\nxerîb nenas e baş.\n";
    write_registry(root, &a_text, &b_text_2);
    import_and_partition(root);
    let manifest_a_2 = build_language_model(root, "corpus-a", 1, 1).unwrap();
    assert_eq!(content_hashes(&manifest_a_2), hashes_a_before);
    assert_eq!(
        manifest_a_2.train_document_set_sha256,
        manifest_a.train_document_set_sha256
    );
    assert_eq!(
        manifest_a_2.train_frequencies_sha256,
        manifest_a.train_frequencies_sha256
    );
    assert_eq!(
        manifest_a_2.train_bigrams_sha256,
        manifest_a.train_bigrams_sha256
    );
    assert_eq!(
        manifest_a_2.train_trigrams_sha256,
        manifest_a.train_trigrams_sha256
    );
    assert_eq!(manifest_a_2.train_document_count, a_train);

    // Changing corpus A changes corpus A's model.
    let a_text_2 = corpus_a_text(40);
    write_registry(root, &a_text_2, &b_text_2);
    import_and_partition(root);
    let a_train_2 = canonical_train_documents(root)["corpus-a"];
    assert!(
        a_train_2 > a_train,
        "the added corpus A documents must reach TRAIN"
    );
    let manifest_a_3 = build_language_model(root, "corpus-a", 1, 1).unwrap();
    assert_eq!(manifest_a_3.train_document_count, a_train_2);
    assert_ne!(
        manifest_a_3.train_document_set_sha256,
        manifest_a.train_document_set_sha256
    );
    assert_ne!(
        manifest_a_3.train_frequencies_sha256,
        manifest_a.train_frequencies_sha256
    );
    assert_ne!(
        content_hashes(&manifest_a_3)["unigrams.tsv"],
        hashes_a_before["unigrams.tsv"]
    );
    let model_a_3 = load_language_model(root, "corpus-a-1").unwrap();
    assert_eq!(model_a_3.unigrams["ez"].document_count as usize, a_train_2);
    assert!(!model_a_3.unigrams.contains_key("nenas"));
}

#[test]
fn test_language_model_requires_train_documents_of_the_requested_corpus() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    // Corpus B has no documents at all: registered, imported, but nothing in TRAIN.
    write_registry(root, &corpus_a_text(30), "");
    prepare_pack_fixture(root);
    import_and_partition(root);
    let err = build_language_model(root, "corpus-b", 1, 1).unwrap_err();
    assert!(err.contains("no canonical TRAIN documents"), "{err}");
    // A registered but unknown corpus id is rejected outright.
    assert!(build_language_model(root, "corpus-c", 1, 1).is_err());
}
