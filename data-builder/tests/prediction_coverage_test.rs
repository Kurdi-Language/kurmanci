//! Prediction coverage measurement against the committed prediction fixture pack, bound to a
//! synthetic provenance chain (pack manifest → model manifest → partition manifest) in a
//! temporary root. Proves the accounting (positions, sources, targets), the corpus scoping
//! and near-duplicate exclusion, that the tokenization is the model builder's, the limit and
//! partition rules, every provenance refusal, the JSON round trip, and that nothing but
//! numbers and hashes leaves the measurement.
mod common;

use data_builder_lib::corpus::ngrams::{sentence_token_sequences, split_into_sentences};
use data_builder_lib::corpus::tokenizer::tokenize_text;
use data_builder_lib::eval_prediction_coverage::{
    evaluate_prediction_coverage, format_report, PredictionCoverageReport,
    PREDICTION_COVERAGE_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const MODEL: &str = "test-model";
const CORPUS: &str = "test-corpus";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn record(corpus: &str, partition: &str, doc: &str, canonical: &str, text: &str) -> String {
    format!(
        r#"{{"partition":"{partition}","corpus_id":"{corpus}","document_id":"{doc}","duplicate_group_id":"g-{canonical}","is_duplicate":{dup},"canonical_corpus_id":"{corpus}","canonical_document_id":"{canonical}","source_file":"corpus.txt","source_sha256":"00","text":"{text}"}}"#,
        dup = doc != canonical
    )
}

/// A root with the provenance chain: the partition manifest, the model manifest naming the
/// corpus and that partition manifest, the fixture pack with a manifest naming the model and
/// the model manifest's hash, and the development partition with the given records.
struct Fixture {
    root: tempfile::TempDir,
    pack: PathBuf,
}

impl Fixture {
    fn new(records: &[String]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let partitions = r.join("data/build/corpus-partitions");
        fs::create_dir_all(&partitions).unwrap();
        // The manifest declares exactly the records the development partition carries.
        fs::write(
            partitions.join("manifest.json"),
            format!(
                r#"{{"partition_policy_version":"kurmanci-partition-v1","train_documents":1,"development_documents":{},"evaluation_documents":0}}"#,
                records.len()
            ),
        )
        .unwrap();
        let partition_manifest_sha = sha(&fs::read(partitions.join("manifest.json")).unwrap());
        fs::write(
            partitions.join("development.jsonl"),
            records.join("\n") + "\n",
        )
        .unwrap();
        let model_dir = r.join("data/language-model").join(MODEL);
        fs::create_dir_all(&model_dir).unwrap();
        fs::write(
            model_dir.join("manifest.json"),
            format!(
                r#"{{"schema_version":"language-model-v1","model_id":"{MODEL}","corpus_id":"{CORPUS}","partition_manifest_sha256":"{partition_manifest_sha}"}}"#
            ),
        )
        .unwrap();
        let model_manifest_sha = sha(&fs::read(model_dir.join("manifest.json")).unwrap());
        let pack_dir = r.join("data/build/packs/reviewed");
        fs::create_dir_all(&pack_dir).unwrap();
        let pack = pack_dir.join("lexicon.bin");
        fs::copy(
            common::workspace_root().join("integration/apple/fixtures/prediction_test.bin"),
            &pack,
        )
        .unwrap();
        let pack_sha = sha(&fs::read(&pack).unwrap());
        fs::write(
            pack_dir.join("manifest.json"),
            format!(
                r#"{{"schema_version":"language-pack-manifest-v1","pack_id":"reviewed","model_profile":"prediction","binary_sha256":"{pack_sha}","language_model_id":"{MODEL}","language_model_manifest_sha256":"{model_manifest_sha}"}}"#
            ),
        )
        .unwrap();
        Fixture { root, pack }
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    fn write(&self, rel: &str, content: &str) {
        fs::write(self.root().join(rel), content).unwrap();
    }
}

#[test]
fn coverage_accounts_for_every_position_scoped_to_the_models_corpus() {
    // The fixture predicts "ji" after "ez" (bigram); a two-word context ending in "ez" is
    // therefore answered by the deterministic backoff. One near-duplicate record and one
    // record of another corpus must be skipped; one sentence of unknown words must yield no
    // prediction anywhere.
    let f = Fixture::new(&[
        record(CORPUS, "development", "d1", "d1", "Ew ez ji. Ez ji."),
        record(CORPUS, "development", "d2", "d1", "Ew ez ji."),
        record("other-corpus", "development", "o1", "o1", "Ez ji."),
        record(
            CORPUS,
            "development",
            "d3",
            "d3",
            "Xyzqwv zzzzq qqqqz wwwwx.",
        ),
    ]);
    let report = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap();
    assert_eq!(report.schema_version, PREDICTION_COVERAGE_SCHEMA_VERSION);
    let p = &report.provenance;
    assert_eq!(p.model_id, MODEL);
    assert_eq!(p.corpus_id, CORPUS);
    assert_eq!(p.partition, "development");
    assert_eq!(p.partition_declared_documents, 4);
    for h in [
        &p.pack_sha256,
        &p.pack_manifest_sha256,
        &p.model_manifest_sha256,
        &p.partition_manifest_sha256,
        &p.partition_sha256,
    ] {
        assert_eq!(h.len(), 64);
    }
    assert!(p.pack_entry_count > 0);
    assert_eq!(report.documents_read, 4);
    assert_eq!(report.other_corpus_documents_skipped, 1);
    assert_eq!(report.duplicate_documents_skipped, 1);
    assert_eq!(report.documents_evaluated, 2);
    assert_eq!(report.limit, 5);
    // d1: "ew ez ji" (3 tokens) and "ez ji" (2 tokens); d3: 4 tokens.
    assert_eq!(report.sentences, 3);
    assert_eq!(report.tokens, 9);
    let one = &report.one_word_contexts;
    let two = &report.two_word_contexts;
    assert_eq!(one.positions, 3);
    assert_eq!(two.positions, 3);
    assert_eq!(one.bigram_hits + one.zero_results, one.positions);
    assert_eq!(
        two.trigram_hits + two.bigram_backoffs + two.zero_results,
        two.positions
    );
    assert!(one.bigram_hits >= 1, "{one:?}");
    assert!(one.target_in_top_1 >= 1, "{one:?}");
    assert_eq!(two.trigram_hits + two.bigram_backoffs, 1, "{two:?}");
    assert_eq!(two.zero_results, 2, "{two:?}");
    assert!(two.target_in_top_1 >= 1, "{two:?}");
    assert!(one.zero_results >= 1, "{one:?}");
    for c in [one, two] {
        assert!(c.target_in_top_1 <= c.target_in_top_3 && c.target_in_top_3 <= c.target_in_top_5);
        assert!(c.target_in_top_5 <= c.positions);
        for rate in [
            c.trigram_hit_rate,
            c.bigram_backoff_rate,
            c.bigram_hit_rate,
            c.zero_result_rate,
            c.top_1_rate,
            c.top_3_rate,
            c.top_5_rate,
        ] {
            assert!((0.0..=1.0).contains(&rate));
        }
    }
    assert!((two.zero_result_rate - two.zero_results as f64 / two.positions as f64).abs() < 1e-12);

    // JSON round trip; the report and the table carry numbers and hashes only.
    let json = serde_json::to_string_pretty(&report).unwrap();
    let back: PredictionCoverageReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
    let table = format_report(&report);
    for word in ["xyzqwv", "zzzzq", "qqqqz", "Ew ez", "ez ji"] {
        assert!(!json.contains(word), "{word} leaked into the JSON report");
        assert!(!table.contains(word), "{word} leaked into the table");
    }
    assert!(table.contains("two-word") && table.contains("one-word"));

    // Determinism across runs.
    let again = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap();
    assert_eq!(again, report);
}

#[test]
fn tokenization_is_the_model_builders_including_zero_width_and_control_characters() {
    // The shared helper is split_into_sentences followed by tokenize_text, nothing else: a
    // zero-width space inside a word and a control character are treated exactly as the
    // model builder treats them, so the evaluator's context sequences are the trained ones.
    let text = "Ew e\u{200B}z ji.\u{0007} Ez\u{FEFF} ji!";
    let expected: Vec<Vec<String>> = split_into_sentences(text)
        .iter()
        .map(|s| tokenize_text(s))
        .collect();
    assert_eq!(sentence_token_sequences(text), expected);
    // The same text as a JSON string (control characters must be escaped in JSON).
    let json_text = "Ew e\\u200bz ji.\\u0007 Ez\\ufeff ji!";
    let f = Fixture::new(&[record(CORPUS, "development", "d1", "d1", json_text)]);
    let report = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap();
    assert_eq!(
        report.sentences,
        expected.iter().filter(|s| !s.is_empty()).count()
    );
    assert_eq!(report.tokens, expected.iter().map(Vec::len).sum::<usize>());
}

#[test]
fn provenance_and_input_rules_fail_closed() {
    let base = [record(CORPUS, "development", "d1", "d1", "Ew ez ji.")];

    // The limit and partition rules.
    let f = Fixture::new(&base);
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 3).unwrap_err();
    assert!(err.contains("at least 5"), "{err}");
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "train", 5).unwrap_err();
    assert!(err.contains("train partition built the model"), "{err}");

    // A record whose partition field is not the requested one.
    let f = Fixture::new(&[record(CORPUS, "train", "d1", "d1", "Ew ez ji.")]);
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(
        err.contains("says 'train', expected 'development'"),
        "{err}"
    );

    // The local partitioning is not the one the model was built from.
    let f = Fixture::new(&base);
    f.write(
        "data/build/corpus-partitions/manifest.json",
        r#"{"partition_policy_version":"kurmanci-partition-v1","train_documents":2}"#,
    );
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(err.contains("not the partitioning the model"), "{err}");

    // The model manifest is not the one the pack recorded.
    let f = Fixture::new(&base);
    let model_manifest = f
        .root()
        .join("data/language-model")
        .join(MODEL)
        .join("manifest.json");
    let mut edited = fs::read_to_string(&model_manifest).unwrap();
    edited.push('\n');
    fs::write(&model_manifest, edited).unwrap();
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(
        err.contains("differs from the one the pack recorded"),
        "{err}"
    );

    // The pack must be the one its manifest describes: a modified pack (one byte changed,
    // same size) with an unchanged manifest, and a manifest without binary_sha256.
    let f = Fixture::new(&base);
    let mut bytes = fs::read(&f.pack).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    fs::write(&f.pack, bytes).unwrap();
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(
        err.contains("differs from the binary_sha256 its manifest records"),
        "{err}"
    );
    let f = Fixture::new(&base);
    let manifest = f.root().join("data/build/packs/reviewed/manifest.json");
    let without: serde_json::Value = {
        let mut v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("binary_sha256");
        v
    };
    fs::write(&manifest, without.to_string()).unwrap();
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(err.contains("records no binary_sha256"), "{err}");

    // The partition file must carry exactly the record count the model's partition manifest
    // declares: fewer records (truncated) and more records (augmented) are both refused.
    let f = Fixture::new(&base);
    f.write(
        "data/build/corpus-partitions/development.jsonl",
        &(record(CORPUS, "development", "d1", "d1", "Ew ez ji.")
            + "\n"
            + &record(CORPUS, "development", "d2", "d2", "Ez ji.")
            + "\n"),
    );
    // The manifest (pinned by the model) declares 1 record; rewriting it would change its hash.
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(
        err.contains(
            "has 2 records but the partition manifest the model was built from declares 1"
        ),
        "{err}"
    );
    let f = Fixture::new(&[
        record(CORPUS, "development", "d1", "d1", "Ew ez ji."),
        record(CORPUS, "development", "d2", "d2", "Ez ji."),
    ]);
    f.write(
        "data/build/corpus-partitions/development.jsonl",
        &(record(CORPUS, "development", "d1", "d1", "Ew ez ji.") + "\n"),
    );
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(
        err.contains(
            "has 1 records but the partition manifest the model was built from declares 2"
        ),
        "{err}"
    );

    // The model id is validated before any path is derived from it: traversal, absolute and
    // hidden ids are refused even when such a directory exists.
    for bad in ["../escaped-model", "/tmp/escaped-model", ".hidden"] {
        let f = Fixture::new(&base);
        let escaped = f.root().join("escaped-model");
        fs::create_dir_all(&escaped).unwrap();
        fs::copy(
            f.root()
                .join("data/language-model")
                .join(MODEL)
                .join("manifest.json"),
            escaped.join("manifest.json"),
        )
        .unwrap();
        let manifest = f.root().join("data/build/packs/reviewed/manifest.json");
        let mut v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
        v["language_model_id"] = serde_json::Value::String(bad.to_string());
        fs::write(&manifest, v.to_string()).unwrap();
        let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
        assert!(err.contains("Invalid language model id"), "{bad}: {err}");
    }

    // Unsupported manifest schemas are refused before their provenance is interpreted.
    let f = Fixture::new(&base);
    let manifest = f.root().join("data/build/packs/reviewed/manifest.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    v["schema_version"] = serde_json::Value::String("language-pack-manifest-v2".to_string());
    fs::write(&manifest, v.to_string()).unwrap();
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(
        err.contains("has schema Some(\"language-pack-manifest-v2\")"),
        "{err}"
    );
    let f = Fixture::new(&base);
    let model_manifest = f
        .root()
        .join("data/language-model")
        .join(MODEL)
        .join("manifest.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&model_manifest).unwrap()).unwrap();
    v["schema_version"] = serde_json::Value::String("language-model-v9".to_string());
    let edited = v.to_string();
    fs::write(&model_manifest, &edited).unwrap();
    // The pack manifest must still bind to this (edited) model manifest so that the schema
    // check, not the hash check, is the one that fires.
    let pack_manifest = f.root().join("data/build/packs/reviewed/manifest.json");
    let mut pm: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&pack_manifest).unwrap()).unwrap();
    pm["language_model_manifest_sha256"] = serde_json::Value::String(sha(edited.as_bytes()));
    fs::write(&pack_manifest, pm.to_string()).unwrap();
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(
        err.contains("has schema Some(\"language-model-v9\")"),
        "{err}"
    );

    // A pack without a model, a pack without a manifest, a missing partition, malformed input.
    let f = Fixture::new(&base);
    let pack_sha = sha(&fs::read(&f.pack).unwrap());
    f.write(
        "data/build/packs/reviewed/manifest.json",
        &format!(
            r#"{{"schema_version":"language-pack-manifest-v1","pack_id":"seed","model_profile":"none","binary_sha256":"{pack_sha}"}}"#
        ),
    );
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(err.contains("embeds no language model"), "{err}");
    let f = Fixture::new(&base);
    fs::remove_file(f.root().join("data/build/packs/reviewed/manifest.json")).unwrap();
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(err.contains("pack manifest"), "{err}");
    let f = Fixture::new(&base);
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "evaluation", 5).unwrap_err();
    assert!(err.contains("Failed to read partition"), "{err}");
    let f = Fixture::new(&base);
    f.write(
        "data/build/corpus-partitions/development.jsonl",
        "{not json}\n",
    );
    let err = evaluate_prediction_coverage(f.root(), &f.pack, "development", 5).unwrap_err();
    assert!(err.contains("Invalid partition record on line 1"), "{err}");
}
