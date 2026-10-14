//! TRAIN corpus attestation against a synthetic provenance chain in a temporary root.
//!
//! Proves the funnel accounting, the document-count buckets, the corpus scoping and canonical
//! representative rule, and every refusal that keeps the signal honest: a held-out document
//! inside the TRAIN file, a document set that is not the one the language model recorded, a
//! repartitioned corpus, a model built from another corpus, a review pool that overlaps a
//! policy-exclusion queue, and corpus artifacts that changed after the model was built.
//!
//! The central guarantee under test is that attestation cannot be computed over DEVELOPMENT or
//! EVALUATION material. That is enforced by recomputing `train_document_set_sha256`, so the
//! tests tamper with the document set rather than with a flag.

use data_builder_lib::review::{build_train_attestation_report, TRAIN_ATTESTATION_SCHEMA_VERSION};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const MODEL: &str = "test-model";
const CORPUS: &str = "test-corpus";
const SOURCE: &str = "test-source";
const REVISION: &str = "deadbeef";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn doc(partition: &str, corpus: &str, id: &str, canonical: &str, text: &str) -> String {
    format!(
        r#"{{"partition":"{partition}","corpus_id":"{corpus}","document_id":"{id}","duplicate_group_id":"g-{canonical}","is_duplicate":{dup},"canonical_corpus_id":"{corpus}","canonical_document_id":"{canonical}","source_file":"c.txt","source_sha256":"00","text":"{text}"}}"#,
        dup = id != canonical
    )
}

fn pool_entry(target: &str, normalized: &str) -> String {
    format!(
        r#"{{"schema_version":"review-queue-v1","target_type":"entry","target_id":"{target}","display":"{normalized}","normalized":"{normalized}","source_id":"{SOURCE}","source_revision":"{REVISION}"}}"#
    )
}

/// Everything the report reads, assembled so that the language-model manifest truthfully
/// anchors the TRAIN document set unless a test deliberately breaks one link.
struct Fixture {
    root: tempfile::TempDir,
}

#[derive(Default)]
struct Opts {
    /// Extra lines appended to train.jsonl after the canonical set is hashed, so the recorded
    /// document set no longer describes the file.
    extra_train_lines: Vec<String>,
    /// Replaces the recorded document-set hash.
    tamper_document_set_sha: Option<String>,
    /// Replaces the recorded partition manifest hash.
    tamper_partition_sha: Option<String>,
    /// Replaces the recorded canonical-import manifest hash.
    tamper_canonical_sha: Option<String>,
    /// Replaces the recorded train document count.
    tamper_train_document_count: Option<usize>,
    /// Corpus the model claims to have been built from.
    model_corpus_id: Option<String>,
    /// Replaces the model manifest's schema_version.
    model_schema_version: Option<String>,
    /// The source id the registry registers, when it should differ from the one requested.
    registered_source_id: Option<String>,
    /// The version the registry records for the source.
    registered_version: Option<String>,
    decisions: Vec<(String, String)>,
    conflict_members: Vec<String>,
    alphabet_excluded: Vec<String>,
}

impl Fixture {
    /// `train_docs` are the records written to train.jsonl. Only this corpus's canonical
    /// representatives of partition "train" are expected to contribute.
    fn new(train_docs: &[String], pool: &[String], opts: Opts) -> Self {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();

        // The source registry: attestation is reported for registered sources only.
        let registered_id = opts
            .registered_source_id
            .clone()
            .unwrap_or_else(|| SOURCE.to_string());
        let registered_version = opts
            .registered_version
            .clone()
            .unwrap_or_else(|| REVISION.to_string());
        write(
            &r.join("data/source-registry/sources.toml"),
            &format!(
                r#"[[sources]]
source_id = "{registered_id}"
source_name = "Fixture source"
author = "Fixture"
license = "Apache-2.0"
license_url = "https://example.invalid/LICENSE"
url = "https://example.invalid/source"
version = "{registered_version}"
redistribution = "allowed"
notes = "Fixture source registered for the attestation test."
"#
            ),
        );

        // Corpus artifacts the model pins by hash.
        let corpora_toml = r.join("data/source-registry/corpora.toml");
        write(&corpora_toml, "# fixture corpus registry\n");
        let canonical_manifest = r.join("data/imported-canonical/manifest.json");
        write(&canonical_manifest, "{\"fixture\":true}\n");

        // Partitioning. train_documents counts every record in the file, canonical or not.
        let partitions = r.join("data/build/corpus-partitions");
        let mut train_body = String::new();
        for d in train_docs {
            train_body.push_str(d);
            train_body.push('\n');
        }
        // The document set the model recorded: this corpus's canonical train representatives.
        let mut set_hasher = Sha256::new();
        let mut canonical_count = 0usize;
        for d in train_docs {
            let v: serde_json::Value = serde_json::from_str(d).unwrap();
            let is_train = v["partition"] == "train";
            let corpus = v["corpus_id"].as_str().unwrap();
            let canonical_corpus = v["canonical_corpus_id"].as_str().unwrap();
            let id = v["document_id"].as_str().unwrap();
            let canonical_id = v["canonical_document_id"].as_str().unwrap();
            if is_train && corpus == CORPUS && corpus == canonical_corpus && id == canonical_id {
                canonical_count += 1;
                set_hasher.update(d.as_bytes());
                set_hasher.update(b"\n");
            }
        }
        let document_set_sha = format!("{:x}", set_hasher.finalize());
        let train_records = train_docs.len();

        let mut file_body = train_body.clone();
        for extra in &opts.extra_train_lines {
            file_body.push_str(extra);
            file_body.push('\n');
        }
        write(&partitions.join("train.jsonl"), &file_body);
        let total_records = train_records + opts.extra_train_lines.len();
        write(
            &partitions.join("manifest.json"),
            &format!("{{\"train_documents\":{}}}\n", total_records),
        );

        let partition_manifest_sha = sha(&fs::read(partitions.join("manifest.json")).unwrap());
        let train_partition_sha = sha(&fs::read(partitions.join("train.jsonl")).unwrap());

        // The language-model manifest: the anchor.
        let model_manifest = format!(
            r#"{{
  "schema_version": "{schema}",
  "model_id": "{model}",
  "corpus_id": "{corpus}",
  "corpus_version": "19700101",
  "corpus_source_artifact_sha256": "aa",
  "corpus_documents_sha256": "bb",
  "corpus_registry_sha256": "{registry}",
  "canonical_manifest_sha256": "{canonical}",
  "partition_manifest_sha256": "{partition}",
  "train_partition_sha256": "{train}",
  "train_document_count": {count},
  "train_document_set_sha256": "{set}"
}}
"#,
            schema = opts
                .model_schema_version
                .clone()
                .unwrap_or_else(|| "language-model-v1".to_string()),
            model = MODEL,
            corpus = opts
                .model_corpus_id
                .clone()
                .unwrap_or_else(|| CORPUS.to_string()),
            registry = sha(&fs::read(&corpora_toml).unwrap()),
            canonical = opts
                .tamper_canonical_sha
                .clone()
                .unwrap_or_else(|| sha(&fs::read(&canonical_manifest).unwrap())),
            partition = opts
                .tamper_partition_sha
                .clone()
                .unwrap_or(partition_manifest_sha),
            train = train_partition_sha,
            count = opts.tamper_train_document_count.unwrap_or(canonical_count),
            set = opts
                .tamper_document_set_sha
                .clone()
                .unwrap_or(document_set_sha),
        );
        write(
            &r.join("data/language-model")
                .join(MODEL)
                .join("manifest.json"),
            &model_manifest,
        );

        // Review artifacts.
        let queue_dir = r.join("data/review-queues").join(SOURCE);
        let mut pool_body = String::new();
        for p in pool {
            pool_body.push_str(p);
            pool_body.push('\n');
        }
        write(&queue_dir.join("hunspell-only.jsonl"), &pool_body);
        let mut conflicts = String::new();
        if !opts.conflict_members.is_empty() {
            conflicts.push_str(&format!(
                "{{\"member_entry_ids\":{}}}\n",
                serde_json::to_string(&opts.conflict_members).unwrap()
            ));
        }
        write(
            &queue_dir.join("metadata-conflict-groups.jsonl"),
            &conflicts,
        );
        if !opts.alphabet_excluded.is_empty() {
            let mut body = String::new();
            for t in &opts.alphabet_excluded {
                body.push_str(&format!("{{\"target_id\":\"{}\"}}\n", t));
            }
            write(&queue_dir.join("alphabet-policy-excluded.jsonl"), &body);
        }
        let mut decisions = String::new();
        for (target, status) in &opts.decisions {
            decisions.push_str(&format!(
                "{{\"target_id\":\"{}\",\"review_status\":\"{}\"}}\n",
                target, status
            ));
        }
        write(
            &r.join("data/review-decisions")
                .join(SOURCE)
                .join("decisions.jsonl"),
            &decisions,
        );

        Fixture { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn run(&self) -> Result<data_builder_lib::review::TrainAttestationReport, String> {
        build_train_attestation_report(self.path(), CORPUS, SOURCE, MODEL)
    }

    fn run_source(
        &self,
        source: &str,
    ) -> Result<data_builder_lib::review::TrainAttestationReport, String> {
        build_train_attestation_report(self.path(), CORPUS, source, MODEL)
    }

    fn run_corpus(
        &self,
        corpus: &str,
    ) -> Result<data_builder_lib::review::TrainAttestationReport, String> {
        build_train_attestation_report(self.path(), corpus, SOURCE, MODEL)
    }
}

fn bucket(report: &data_builder_lib::review::TrainAttestationReport, name: &str) -> usize {
    report
        .pending_by_bucket
        .iter()
        .find(|b| b.bucket == name)
        .map(|b| b.entries)
        .unwrap_or_else(|| panic!("bucket {} missing", name))
}

#[test]
fn counts_attestation_over_canonical_train_representatives() {
    // "rojbaş" in three canonical documents, "welat" in one, "tenê" only in a duplicate and
    // only in another corpus's document, so both must come out unattested.
    let train = vec![
        doc("train", CORPUS, "d1", "d1", "rojbaş welat"),
        doc("train", CORPUS, "d2", "d2", "rojbaş"),
        doc("train", CORPUS, "d3", "d3", "rojbaş"),
        doc("train", CORPUS, "d4", "d1", "tenê"),
        doc("train", "other-corpus", "o1", "o1", "tenê"),
    ];
    let pool = vec![
        pool_entry("t-rojbas", "rojbaş"),
        pool_entry("t-welat", "welat"),
        pool_entry("t-tene", "tenê"),
    ];
    let f = Fixture::new(&train, &pool, Opts::default());
    let r = f.run().expect("report");

    assert_eq!(r.schema_version, TRAIN_ATTESTATION_SCHEMA_VERSION);
    assert_eq!(r.corpus.train_records_read, 5);
    assert_eq!(
        r.corpus.train_document_count, 3,
        "canonical representatives of this corpus only"
    );
    assert_eq!(r.funnel.pool_records, 3);
    assert_eq!(r.funnel.pending, 3);
    assert_eq!(r.funnel.pending_train_attested, 2, "rojbaş and welat");
    assert_eq!(
        r.funnel.pending_train_unattested, 1,
        "tenê is only in a duplicate"
    );
    assert_eq!(bucket(&r, "0"), 1);
    assert_eq!(bucket(&r, "1-4"), 2, "welat once, rojbaş three times");
    assert_eq!(bucket(&r, "5-9"), 0);
    assert_eq!(r.source.source_revision, REVISION);
    assert_eq!(r.corpus.tokenizer, "corpus::tokenizer::tokenize_text");
}

#[test]
fn decided_and_conflicting_entries_leave_the_pending_population() {
    let train = vec![
        doc("train", CORPUS, "d1", "d1", "rojbaş welat spas"),
        doc("train", CORPUS, "d2", "d2", "rojbaş welat"),
    ];
    let pool = vec![
        pool_entry("t-rojbas", "rojbaş"),
        pool_entry("t-welat", "welat"),
        pool_entry("t-spas", "spas"),
    ];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            decisions: vec![("t-rojbas".into(), "approved".into())],
            conflict_members: vec!["t-welat".into()],
            ..Default::default()
        },
    );
    let r = f.run().expect("report");
    assert_eq!(r.funnel.excluded_already_decided, 1);
    assert_eq!(r.funnel.excluded_conflict_group_member, 1);
    assert_eq!(r.funnel.pending, 1, "only spas remains");
    assert_eq!(r.funnel.pending_train_attested, 1);
    // The decided entry is reported in its own bucket with its recorded status, verbatim.
    let decided = r
        .decided_by_bucket
        .iter()
        .find(|b| b.bucket == "1-4")
        .expect("decided bucket");
    assert_eq!(decided.decided, 1);
    assert_eq!(decided.by_status.get("approved"), Some(&1));
}

#[test]
fn refuses_a_held_out_document_inside_the_train_file() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            extra_train_lines: vec![doc("development", CORPUS, "d9", "d9", "rojbaş")],
            ..Default::default()
        },
    );
    let err = f.run().expect_err("a development record must be refused");
    assert!(
        err.contains("refuses to read a DEVELOPMENT or EVALUATION document"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_a_document_set_the_model_did_not_record() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            tamper_document_set_sha: Some("00".repeat(32)),
            ..Default::default()
        },
    );
    let err = f.run().expect_err("a foreign document set must be refused");
    assert!(
        err.contains("refuses to attribute attestation to a document set the model did not use"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_a_repartitioned_corpus() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            tamper_partition_sha: Some("11".repeat(32)),
            ..Default::default()
        },
    );
    let err = f.run().expect_err("a changed partitioning must be refused");
    assert!(err.contains("repartitioned"), "unexpected error: {err}");
}

#[test]
fn refuses_a_changed_train_document_count() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            tamper_train_document_count: Some(99),
            ..Default::default()
        },
    );
    let err = f.run().expect_err("a count mismatch must be refused");
    assert!(
        err.contains("canonical TRAIN documents"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_corpus_artifacts_that_changed_after_the_model_was_built() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            tamper_canonical_sha: Some("22".repeat(32)),
            ..Default::default()
        },
    );
    let err = f
        .run()
        .expect_err("a changed canonical manifest must be refused");
    assert!(
        err.contains("corpus state has changed since the model was built"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_a_model_built_from_another_corpus() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            model_corpus_id: Some("some-other-corpus".into()),
            ..Default::default()
        },
    );
    let err = f
        .run_corpus(CORPUS)
        .expect_err("a model from another corpus must be refused");
    assert!(
        err.contains("evidence for its own corpus only"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_a_pool_that_overlaps_a_policy_exclusion_queue() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            alphabet_excluded: vec!["t-rojbas".into()],
            ..Default::default()
        },
    );
    let err = f.run().expect_err("an overlapping pool must be refused");
    assert!(
        err.contains("policy-exclusion queue"),
        "unexpected error: {err}"
    );
}

#[test]
fn report_round_trips_as_json_and_carries_no_lexical_form() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş welat")];
    let pool = vec![
        pool_entry("t-rojbas", "rojbaş"),
        pool_entry("t-welat", "welat"),
    ];
    let f = Fixture::new(&train, &pool, Opts::default());
    let r = f.run().expect("report");
    let json = serde_json::to_string(&r).unwrap();
    let back: data_builder_lib::review::TrainAttestationReport =
        serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
    // Counts, identities and hashes only: no word from the corpus or the pool may appear.
    for form in ["rojbaş", "welat"] {
        assert!(
            !json.contains(form),
            "the report leaked the lexical form {form}"
        );
    }
}

#[test]
fn missing_partitions_are_reported_as_an_unacquired_corpus() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root
            .path()
            .join("data/language-model")
            .join(MODEL)
            .join("manifest.json"),
        "{}",
    );
    let err = build_train_attestation_report(root.path(), CORPUS, SOURCE, MODEL)
        .expect_err("a root without partitions must be refused");
    assert!(
        err.contains("Invalid language model manifest")
            || err.contains("corpus partitions missing"),
        "unexpected error: {err}"
    );
    let _unused: PathBuf = root.path().to_path_buf();
}

#[test]
fn refuses_an_unsupported_language_model_schema() {
    // An otherwise valid manifest: only its schema is one this report does not understand.
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            model_schema_version: Some("language-model-v99".into()),
            ..Default::default()
        },
    );
    let err = f
        .run()
        .expect_err("an unsupported manifest schema must be refused");
    assert!(
        err.contains("refusing to interpret its corpus and partition provenance"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_a_source_that_is_not_in_the_registry() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            registered_source_id: Some("some-other-source".into()),
            ..Default::default()
        },
    );
    let err = f.run().expect_err("an unregistered source must be refused");
    assert!(err.contains("is not registered"), "unexpected error: {err}");
}

#[test]
fn refuses_a_source_id_that_is_not_a_plain_identifier() {
    // The id reaches no filesystem path: it is rejected on shape before any path is built.
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(&train, &pool, Opts::default());
    for bad in ["../escape", "a/b", "", ".hidden"] {
        let err = f.run_source(bad).unwrap_err_or_else_message();
        assert!(
            err.contains("Invalid source id"),
            "source id {bad:?} was not rejected on shape: {err}"
        );
    }
}

#[test]
fn refuses_a_pool_revision_that_is_not_the_registered_version() {
    let train = vec![doc("train", CORPUS, "d1", "d1", "rojbaş")];
    let pool = vec![pool_entry("t-rojbas", "rojbaş")];
    let f = Fixture::new(
        &train,
        &pool,
        Opts {
            registered_version: Some("0123456789abcdef".into()),
            ..Default::default()
        },
    );
    let err = f
        .run()
        .expect_err("a pool at another revision than the registry records must be refused");
    assert!(
        err.contains("the registry records"),
        "unexpected error: {err}"
    );
}

/// Small helper so the loop above reads as one assertion per bad id.
trait ErrMessage {
    fn unwrap_err_or_else_message(self) -> String;
}

impl ErrMessage for Result<data_builder_lib::review::TrainAttestationReport, String> {
    fn unwrap_err_or_else_message(self) -> String {
        match self {
            Ok(_) => panic!("expected a refusal, got a report"),
            Err(e) => e,
        }
    }
}
