//! Integration tests for committed language models and pack model profiles.

use data_builder_lib::pack::language_model::{
    authoritative_vocabulary_fingerprint, load_language_model, write_language_model,
    LanguageModelContent, LanguageModelLicensing, LanguageModelManifest,
    LANGUAGE_MODEL_SCHEMA_VERSION,
};
use data_builder_lib::pack::manifest::{
    validate_all_pack_manifests, DataLicenseEntry, PackManifest,
};
use data_builder_lib::pack::{build_pack, PackPolicyConfig};
use data_builder_lib::validate::FrequencyMetadata;
use kurmanci_engine::Engine;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_policy(dir: &Path, seed_profile: &str, seed_model: Option<&str>) {
    let model_line = seed_model
        .map(|m| format!("language_model = \"{}\"\n", m))
        .unwrap_or_default();
    fs::create_dir_all(dir.join("data")).unwrap();
    fs::write(
        dir.join("data/pack-policy.toml"),
        format!(
            r#"schema_version = "pack-policy-v1"
default_pack = "seed"

[packs.seed]
description = "Manually reviewed seed lexicon only"
opt_in = false
allow_as_default = true
model_profile = "{profile}"
{model_line}
[packs.reviewed]
description = "Manual seed plus explicitly approved external entries"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.experimental-full]
description = "Manual seed plus mechanically valid imported entries"
opt_in = true
allow_as_default = false
model_profile = "none"
"#,
            profile = seed_profile,
            model_line = model_line,
        ),
    )
    .unwrap();
}

/// Minimal review environment so that all three authoritative packs resolve: an empty
/// Hunspell decision file, one unreviewed queue entry ("nenas", which therefore appears in
/// `experimental-full` but not in `seed`/`reviewed`), and a validated review report set.
/// The authoritative union vocabulary is thus exactly {baş, bext, ez, nenas}.
pub fn prepare_review_environment(dir: &Path) {
    let source_id = "kurdish-hunspell-kmr";
    let dec_dir = dir.join(format!("data/review-decisions/{}", source_id));
    fs::create_dir_all(&dec_dir).unwrap();
    fs::write(dec_dir.join("decisions.jsonl"), "").unwrap();

    let queue_dir = dir.join(format!("data/review-queues/{}", source_id));
    fs::create_dir_all(&queue_dir).unwrap();
    let queue_record = concat!(
        r#"{"schema_version":"review-queue-v1","rule_id":"HUNSPELL_ONLY_V1","rule_version":"1","#,
        r#""target_type":"entry","target_id":"1111111111111111111111111111111111111111111111111111111111111111","#,
        r#""display":"nenas","normalized":"nenas","source_id":"kurdish-hunspell-kmr","#,
        r#""source_revision":"88131d6878ef7fa3ee114aa554adc385ff85b44c","source_lines":[1],"flags":"","#,
        r#""morphology":[],"part_of_speech":"noun","reason_codes":["HUNSPELL_ONLY"],"#,
        r#""suggested_action":"retain","generated_status":"unreviewed","effective_review_status":"unreviewed","#,
        r#""decision_entry_id":null,"queue_categories":["hunspell-only"]}"#,
        "\n"
    );
    fs::write(queue_dir.join("hunspell-only.jsonl"), queue_record).unwrap();
    fs::write(
        queue_dir.join("artifacts.sha256"),
        format!(
            "{}  data/review-queues/{}/hunspell-only.jsonl\n",
            sha256_hex(queue_record.as_bytes()),
            source_id
        ),
    )
    .unwrap();

    let rep_dir = dir.join("data/reports/controlled-lexicon-review");
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
    let mut artifact_lines = vec![format!(
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
        artifact_lines.push(format!(
            "{}  data/reports/controlled-lexicon-review/{}",
            empty_sha, name
        ));
    }
    fs::write(
        rep_dir.join("artifacts.sha256"),
        artifact_lines.join("\n") + "\n",
    )
    .unwrap();
}

fn prepare_fixture(dir: &Path, seed_profile: &str, seed_model: Option<&str>) {
    let seed_dir = dir.join("data/reviewed");
    fs::create_dir_all(&seed_dir).unwrap();
    fs::write(
        seed_dir.join("lexicon.jsonl"),
        r#"{"word":"ez","lemma":"ez","normalized":"ez","part_of_speech":"pronoun","frequency":0,"status":"approved","variants":[],"sources":["manual-seed"],"regions":["general"]}
{"word":"baş","lemma":"baş","normalized":"baş","part_of_speech":"adjective","frequency":0,"status":"approved","variants":[],"sources":["manual-seed"],"regions":["general"]}
{"word":"bext","lemma":"bext","normalized":"bext","part_of_speech":"noun","frequency":0,"status":"approved","variants":[],"sources":["manual-seed"],"regions":["general"]}
"#,
    )
    .unwrap();

    let reg_dir = dir.join("data/source-registry");
    fs::create_dir_all(&reg_dir).unwrap();
    fs::write(
        reg_dir.join("sources.toml"),
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

    prepare_review_environment(dir);
    write_policy(dir, seed_profile, seed_model);
}

fn test_manifest(model_id: &str) -> LanguageModelManifest {
    LanguageModelManifest {
        schema_version: LANGUAGE_MODEL_SCHEMA_VERSION.into(),
        model_id: model_id.into(),
        corpus_id: "test-corpus".into(),
        corpus_version: "1".into(),
        contributing_corpora: vec!["test-corpus".into()],
        corpus_source_artifact_sha256: None,
        corpus_documents_sha256: "0".repeat(64),
        corpus_registry_sha256: "0".repeat(64),
        canonical_manifest_sha256: "0".repeat(64),
        partition_manifest_sha256: "0".repeat(64),
        train_partition_sha256: "1".repeat(64),
        train_document_count: 3,
        train_document_set_sha256: "2".repeat(64),
        train_frequencies_sha256: "0".repeat(64),
        train_bigrams_sha256: "0".repeat(64),
        train_trigrams_sha256: "0".repeat(64),
        build_manifest_sha256: "0".repeat(64),
        ngram_config_sha256: "0".repeat(64),
        licensing: LanguageModelLicensing {
            corpus_name: "Test Corpus".into(),
            license: "CC BY-SA 4.0".into(),
            license_spdx: "CC-BY-SA-4.0".into(),
            license_url: "https://creativecommons.org/licenses/by-sa/4.0/".into(),
            attribution: "Test contributors".into(),
            source_url: "https://example.org/corpus".into(),
            redistribution_determination: "pending-review".into(),
        },
        vocabulary_fingerprint: String::new(),
        vocabulary_size: 0,
        unigram_count: 0,
        bigram_count: 0,
        trigram_count: 0,
        bigram_min_count: 2,
        trigram_min_count: 3,
        files: Vec::new(),
    }
}

/// Vocabulary (sorted): 0 = "baş", 1 = "bext", 2 = "ez", 3 = "nenas" (experimental-full only).
fn write_model(dir: &Path, model_id: &str) {
    let content = LanguageModelContent {
        vocabulary: vec!["baş".into(), "bext".into(), "ez".into(), "nenas".into()],
        unigrams: vec![
            (
                0,
                FrequencyMetadata {
                    token_count: 40,
                    document_count: 20,
                    zipf_milli: 7000,
                },
            ),
            (
                3,
                FrequencyMetadata {
                    token_count: 5,
                    document_count: 5,
                    zipf_milli: 6000,
                },
            ),
        ],
        bigrams: vec![(2, 0, 10, 10, 1_000_000)],
        trigrams: vec![(2, 0, 1, 4, 4, 1_000_000)],
    };
    write_language_model(dir, test_manifest(model_id), &content).unwrap();
}

/// Overwrites one model file and re-pins its hash in `manifest.json` and `artifacts.sha256`,
/// so that only the semantic content changes (hash checks stay green).
fn rewrite_model_file(dir: &Path, model_id: &str, name: &str, content: &str) {
    let model_dir = dir.join("data/language-model").join(model_id);
    fs::write(model_dir.join(name), content).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(model_dir.join("manifest.json")).unwrap()).unwrap();
    for f in manifest["files"].as_array_mut().unwrap() {
        let path = f["path"].as_str().unwrap().to_string();
        f["sha256"] =
            serde_json::Value::String(sha256_hex(&fs::read(model_dir.join(&path)).unwrap()));
    }
    fs::write(
        model_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap() + "\n",
    )
    .unwrap();
    let mut lines = Vec::new();
    for entry in fs::read_dir(&model_dir).unwrap() {
        let file_name = entry.unwrap().file_name().to_string_lossy().to_string();
        if file_name == "artifacts.sha256" {
            continue;
        }
        lines.push(format!(
            "{}  {}",
            sha256_hex(&fs::read(model_dir.join(&file_name)).unwrap()),
            file_name
        ));
    }
    lines.sort();
    fs::write(model_dir.join("artifacts.sha256"), lines.join("\n") + "\n").unwrap();
}

/// Rewrites a built pack manifest and re-pins `artifacts.sha256`, so that only the manifest
/// semantics change (the validator's hash checks stay green).
fn edit_pack_manifest(dir: &Path, pack_id: &str, edit: impl FnOnce(&mut PackManifest)) {
    let pack_dir = dir.join("data/build/packs").join(pack_id);
    let mut manifest: PackManifest =
        serde_json::from_slice(&fs::read(pack_dir.join("manifest.json")).unwrap()).unwrap();
    edit(&mut manifest);
    fs::write(
        pack_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let mut lines = Vec::new();
    for name in [
        "lexicon.bin",
        "manifest.json",
        "collision-report.jsonl",
        "attribution.txt",
    ] {
        lines.push(format!(
            "{}  data/build/packs/{}/{}",
            sha256_hex(&fs::read(pack_dir.join(name)).unwrap()),
            pack_id,
            name
        ));
    }
    fs::write(pack_dir.join("artifacts.sha256"), lines.join("\n") + "\n").unwrap();
}

fn build_all_packs(dir: &Path) {
    for pack_id in ["seed", "reviewed", "experimental-full"] {
        build_pack(pack_id, dir).unwrap();
    }
}

#[test]
fn test_policy_profile_rules() {
    let temp = tempdir().unwrap();
    write_policy(temp.path(), "frequency", None);
    assert!(PackPolicyConfig::load_from_file(temp.path().join("data/pack-policy.toml")).is_err());
    write_policy(temp.path(), "none", Some("m"));
    assert!(PackPolicyConfig::load_from_file(temp.path().join("data/pack-policy.toml")).is_err());
    write_policy(temp.path(), "bigram", Some("m"));
    assert!(PackPolicyConfig::load_from_file(temp.path().join("data/pack-policy.toml")).is_err());
    write_policy(temp.path(), "ngram", Some("m"));
    let cfg = PackPolicyConfig::load_from_file(temp.path().join("data/pack-policy.toml")).unwrap();
    assert!(cfg.packs["seed"].uses_frequencies());
    assert!(cfg.packs["seed"].uses_ngrams());
    assert!(!cfg.packs["reviewed"].uses_frequencies());
}

#[test]
fn test_model_directory_is_numeric_and_round_trips() {
    let temp = tempdir().unwrap();
    prepare_fixture(temp.path(), "none", None);
    write_model(temp.path(), "test-model");
    let dir = temp.path().join("data/language-model/test-model");
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    for n in [
        "vocabulary.txt",
        "unigrams.tsv",
        "bigrams.tsv",
        "trigrams.tsv",
        "manifest.json",
        "artifacts.sha256",
    ] {
        assert!(names.contains(&n.to_string()), "{} missing", n);
    }
    // n-gram tables contain only digits and tabs: no corpus word sequences in readable form.
    for n in ["bigrams.tsv", "trigrams.tsv", "unigrams.tsv"] {
        let text = fs::read_to_string(dir.join(n)).unwrap();
        assert!(
            text.chars()
                .all(|c| c.is_ascii_digit() || c == '\t' || c == '\n'),
            "{} must be purely numeric",
            n
        );
    }
    let model = load_language_model(temp.path(), "test-model").unwrap();
    assert_eq!(model.manifest.vocabulary_size, 4);
    assert_eq!(
        model.manifest.vocabulary_fingerprint,
        authoritative_vocabulary_fingerprint(temp.path()).unwrap()
    );
    assert_eq!(model.unigrams["baş"].zipf_milli, 7000);
    assert_eq!(model.bigrams.len(), 1);
    assert_eq!(model.bigrams[0].previous, "ez");
    assert_eq!(model.bigrams[0].next, "baş");
    assert_eq!(model.trigrams[0].next, "bext");
    assert_eq!(
        model.manifest.licensing.redistribution_determination,
        "pending-review"
    );
    assert_eq!(model.manifest.contributing_corpora, vec!["test-corpus"]);
}

#[test]
fn test_ngram_profile_pack_carries_model_and_predicts() {
    let temp = tempdir().unwrap();
    prepare_fixture(temp.path(), "ngram", Some("test-model"));
    write_model(temp.path(), "test-model");
    let model = load_language_model(temp.path(), "test-model").unwrap();

    let manifest = build_pack("seed", temp.path()).unwrap();
    assert_eq!(manifest.model_profile, "ngram");
    assert_eq!(manifest.frequency_entry_count, 1); // only "baş" is in the seed pack vocabulary
    assert_eq!(manifest.bigram_count, 1);
    assert_eq!(manifest.trigram_count, 1);
    assert_eq!(manifest.language_model_id.as_deref(), Some("test-model"));
    assert_eq!(
        manifest.language_model_manifest_sha256.as_deref(),
        Some(model.manifest_sha256.as_str())
    );
    let prov = manifest.language_model_provenance.as_ref().unwrap();
    assert_eq!(prov.corpus_id, "test-corpus");
    assert_eq!(prov.contributing_corpora, vec!["test-corpus"]);
    assert_eq!(prov.license, "CC BY-SA 4.0");
    assert_eq!(prov.license_spdx, "CC-BY-SA-4.0");
    assert_eq!(prov.train_document_set_sha256, "2".repeat(64));
    assert_eq!(prov.redistribution_determination, "pending-review");
    // The licence entry carries the SPDX identifier, never the human-readable name.
    let model_entries: Vec<&DataLicenseEntry> = manifest
        .data_licenses
        .iter()
        .filter(|l| l.source_id.starts_with("language-model:"))
        .collect();
    assert_eq!(model_entries.len(), 1);
    assert_eq!(model_entries[0].source_id, "language-model:test-model");
    assert_eq!(model_entries[0].spdx, "CC-BY-SA-4.0");
    let attribution =
        fs::read_to_string(temp.path().join("data/build/packs/seed/attribution.txt")).unwrap();
    assert!(attribution.contains("=== Source: language-model:test-model ==="));
    assert!(attribution.contains("Test contributors"));
    assert!(attribution.contains("SPDX CC-BY-SA-4.0"));
    assert!(attribution.contains("pending-review"));

    let bin = fs::read(temp.path().join("data/build/packs/seed/lexicon.bin")).unwrap();
    let mut engine = Engine::new();
    engine.load_binary_pack(&bin).unwrap();
    assert!(!engine.predict_next("ez", 5).is_empty());

    let m2 = build_pack("seed", temp.path()).unwrap();
    assert_eq!(m2.binary_sha256, manifest.binary_sha256);
}

#[test]
fn test_prediction_profile_has_ngrams_but_no_frequencies() {
    let temp = tempdir().unwrap();
    prepare_fixture(temp.path(), "prediction", Some("test-model"));
    write_model(temp.path(), "test-model");

    let manifest = build_pack("seed", temp.path()).unwrap();
    assert_eq!(manifest.model_profile, "prediction");
    assert_eq!(manifest.frequency_entry_count, 0);
    assert_eq!(manifest.bigram_count, 1);
    assert_eq!(manifest.trigram_count, 1);
    assert!(manifest.language_model_provenance.is_some());

    let bin = fs::read(temp.path().join("data/build/packs/seed/lexicon.bin")).unwrap();
    let mut engine = Engine::new();
    engine.load_binary_pack(&bin).unwrap();
    assert!(!engine.predict_next("ez", 5).is_empty());

    // Suggestion ranking is identical to a lexicon-only pack (frequencies are not encoded).
    let with_model: Vec<String> = engine
        .suggest("ba", 5)
        .into_iter()
        .map(|s| s.text)
        .collect();
    write_policy(temp.path(), "none", None);
    build_pack("seed", temp.path()).unwrap();
    let bin_none = fs::read(temp.path().join("data/build/packs/seed/lexicon.bin")).unwrap();
    let mut engine_none = Engine::new();
    engine_none.load_binary_pack(&bin_none).unwrap();
    let without_model: Vec<String> = engine_none
        .suggest("ba", 5)
        .into_iter()
        .map(|s| s.text)
        .collect();
    assert_eq!(with_model, without_model);
}

#[test]
fn test_frequency_profile_excludes_ngrams() {
    let temp = tempdir().unwrap();
    prepare_fixture(temp.path(), "frequency", Some("test-model"));
    write_model(temp.path(), "test-model");

    let manifest = build_pack("seed", temp.path()).unwrap();
    assert_eq!(manifest.model_profile, "frequency");
    assert_eq!(manifest.frequency_entry_count, 1);
    assert_eq!(manifest.bigram_count, 0);
    assert_eq!(manifest.trigram_count, 0);

    let bin = fs::read(temp.path().join("data/build/packs/seed/lexicon.bin")).unwrap();
    let mut engine = Engine::new();
    engine.load_binary_pack(&bin).unwrap();
    assert!(engine.predict_next("ez", 5).is_empty());
}

#[test]
fn test_none_profile_ignores_model_and_tampered_model_fails_closed() {
    let temp = tempdir().unwrap();
    prepare_fixture(temp.path(), "none", None);
    write_model(temp.path(), "test-model");
    let manifest = build_pack("seed", temp.path()).unwrap();
    assert_eq!(manifest.frequency_entry_count, 0);
    assert!(manifest.language_model_id.is_none());
    assert!(manifest.language_model_provenance.is_none());
    assert!(!manifest
        .data_licenses
        .iter()
        .any(|l| l.source_id.starts_with("language-model:")));

    // Tampering with a model file must fail the load and the build.
    write_policy(temp.path(), "ngram", Some("test-model"));
    fs::write(
        temp.path()
            .join("data/language-model/test-model/unigrams.tsv"),
        "0\t999\t999\t9000\n3\t5\t5\t6000\n",
    )
    .unwrap();
    assert!(load_language_model(temp.path(), "test-model").is_err());
    assert!(build_pack("seed", temp.path()).is_err());

    // A missing model is an error, never a silent fallback.
    write_policy(temp.path(), "ngram", Some("absent-model"));
    assert!(build_pack("seed", temp.path()).is_err());
}

#[test]
fn test_loader_enforces_content_invariants_with_valid_hashes() {
    // Every case re-pins the file hashes, so the only thing that can fail is the semantic
    // invariant itself: the loader must enforce what the writer enforces.
    let cases: &[(&str, &str, &str)] = &[
        (
            "vocabulary.txt",
            "baş\nbext\nez nenas\nnenas\n",
            "single non-empty tokens",
        ),
        (
            "vocabulary.txt",
            "baş\nbext\n\nez\n",
            "single non-empty tokens",
        ),
        (
            "vocabulary.txt",
            "baş\nbext\nez\u{0}\nnenas\n",
            "single non-empty tokens",
        ),
        (
            "vocabulary.txt",
            "baş\nbext\nnenas\nez\n",
            "sorted and unique",
        ),
        ("vocabulary.txt", "baş\nbext\nez\nez\n", "sorted and unique"),
        (
            "unigrams.tsv",
            "0\t40\t20\t7000\n0\t5\t5\t6000\n",
            "sorted by unique id",
        ),
        (
            "unigrams.tsv",
            "0\t40\t20\t7000\n9\t5\t5\t6000\n",
            "outside the vocabulary",
        ),
        (
            "unigrams.tsv",
            "0\t5\t6\t7000\n3\t5\t5\t6000\n",
            "impossible counts",
        ),
        (
            "unigrams.tsv",
            "0\t0\t0\t7000\n3\t5\t5\t6000\n",
            "impossible counts",
        ),
        (
            "bigrams.tsv",
            "2\t0\t10\t10\t1000000\n2\t0\t10\t10\t1000000\n",
            "sorted by unique id pair",
        ),
        (
            "bigrams.tsv",
            "2\t7\t10\t10\t1000000\n",
            "outside the vocabulary",
        ),
        (
            "bigrams.tsv",
            "2\t0\t10\t10\t999999\n",
            "does not match count",
        ),
        ("bigrams.tsv", "2\t0\t11\t10\t1000000\n", "Invalid counts"),
        ("bigrams.tsv", "2\t0\t0\t10\t0\n", "Invalid counts"),
        (
            "trigrams.tsv",
            "2\t0\t1\t4\t4\t1000000\n2\t0\t1\t4\t4\t1000000\n",
            "sorted by unique id triple",
        ),
        (
            "trigrams.tsv",
            "2\t0\t1\t4\t4\t500000\n",
            "does not match count",
        ),
        (
            "trigrams.tsv",
            "2\t0\t4\t4\t4\t1000000\n",
            "outside the vocabulary",
        ),
    ];
    for (name, content, expected) in cases {
        let temp = tempdir().unwrap();
        prepare_fixture(temp.path(), "prediction", Some("test-model"));
        write_model(temp.path(), "test-model");
        load_language_model(temp.path(), "test-model").unwrap();
        rewrite_model_file(temp.path(), "test-model", name, content);
        let err = load_language_model(temp.path(), "test-model")
            .expect_err(&format!("{} = {:?} must be rejected", name, content));
        assert!(
            err.contains(expected),
            "{} = {:?}: expected {:?} in error, got {}",
            name,
            content,
            expected,
            err
        );
        assert!(build_pack("seed", temp.path()).is_err());
    }
}

#[test]
fn test_loader_rejects_manifest_claiming_other_contributing_corpora() {
    let temp = tempdir().unwrap();
    prepare_fixture(temp.path(), "none", None);
    write_model(temp.path(), "test-model");
    let model_dir = temp.path().join("data/language-model/test-model");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(model_dir.join("manifest.json")).unwrap()).unwrap();
    manifest["contributing_corpora"] = serde_json::json!(["test-corpus", "other-corpus"]);
    fs::write(
        model_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap() + "\n",
    )
    .unwrap();
    rewrite_model_file(
        temp.path(),
        "test-model",
        "vocabulary.txt",
        "baş\nbext\nez\nnenas\n",
    );
    let err = load_language_model(temp.path(), "test-model").unwrap_err();
    assert!(err.contains("contributing corpora"), "{err}");
}

#[test]
fn test_stale_model_fails_closed_when_authoritative_vocabulary_changes() {
    let temp = tempdir().unwrap();
    prepare_fixture(temp.path(), "prediction", Some("test-model"));
    write_model(temp.path(), "test-model");
    load_language_model(temp.path(), "test-model").unwrap();
    build_pack("seed", temp.path()).unwrap();

    // A new approved seed word changes the authoritative union vocabulary: the committed
    // model no longer describes the packs and must be regenerated, never compiled silently.
    let seed_path = temp.path().join("data/reviewed/lexicon.jsonl");
    let mut seed = fs::read_to_string(&seed_path).unwrap();
    seed.push_str(r#"{"word":"roj","lemma":"roj","normalized":"roj","part_of_speech":"noun","frequency":0,"status":"approved","variants":[],"sources":["manual-seed"],"regions":["general"]}"#);
    seed.push('\n');
    fs::write(&seed_path, seed).unwrap();

    let err = load_language_model(temp.path(), "test-model").unwrap_err();
    assert!(err.contains("regenerate"), "{err}");
    assert!(build_pack("seed", temp.path()).is_err());
}

#[test]
fn test_validator_requires_exact_model_provenance_and_license_entries() {
    let temp = tempdir().unwrap();
    prepare_fixture(temp.path(), "prediction", Some("test-model"));
    write_model(temp.path(), "test-model");
    build_all_packs(temp.path());
    validate_all_pack_manifests(temp.path()).unwrap();

    // Model-backed pack: provenance must equal what the committed model yields.
    edit_pack_manifest(temp.path(), "seed", |m| {
        m.language_model_provenance.as_mut().unwrap().license = "Public Domain".into();
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("language_model_provenance"), "{err}");

    build_all_packs(temp.path());
    edit_pack_manifest(temp.path(), "seed", |m| {
        m.language_model_provenance
            .as_mut()
            .unwrap()
            .redistribution_determination = "approved".into();
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("language_model_provenance"), "{err}");

    build_all_packs(temp.path());
    edit_pack_manifest(temp.path(), "seed", |m| m.language_model_provenance = None);
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("lacks language_model_provenance"), "{err}");

    // Model-backed pack: exactly one matching licence entry.
    build_all_packs(temp.path());
    edit_pack_manifest(temp.path(), "seed", |m| {
        m.data_licenses
            .retain(|l| !l.source_id.starts_with("language-model:"))
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(
        err.contains("lacks the 'language-model:test-model'"),
        "{err}"
    );

    build_all_packs(temp.path());
    edit_pack_manifest(temp.path(), "seed", |m| {
        m.data_licenses.push(DataLicenseEntry {
            source_id: "language-model:test-model".into(),
            spdx: "CC-BY-SA-4.0".into(),
        })
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("exactly one"), "{err}");

    build_all_packs(temp.path());
    edit_pack_manifest(temp.path(), "seed", |m| {
        for l in m.data_licenses.iter_mut() {
            if l.source_id == "language-model:test-model" {
                l.spdx = "CC BY-SA 4.0".into();
            }
        }
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("contradicts"), "{err}");

    // Model-backed pack: the pinned model manifest hash must match the committed model.
    build_all_packs(temp.path());
    edit_pack_manifest(temp.path(), "seed", |m| {
        m.language_model_manifest_sha256 = Some("0".repeat(64))
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("language_model_manifest_sha256"), "{err}");

    // `none` packs: no model reference, provenance or licence entry may be present.
    build_all_packs(temp.path());
    edit_pack_manifest(temp.path(), "reviewed", |m| {
        m.language_model_id = Some("test-model".into())
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("must not reference a language model"), "{err}");

    build_all_packs(temp.path());
    let seed_manifest: PackManifest = serde_json::from_slice(
        &fs::read(temp.path().join("data/build/packs/seed/manifest.json")).unwrap(),
    )
    .unwrap();
    let provenance = seed_manifest.language_model_provenance.clone().unwrap();
    edit_pack_manifest(temp.path(), "reviewed", |m| {
        m.language_model_provenance = Some(provenance)
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("must not reference a language model"), "{err}");

    build_all_packs(temp.path());
    edit_pack_manifest(temp.path(), "experimental-full", |m| {
        m.data_licenses.push(DataLicenseEntry {
            source_id: "language-model:test-model".into(),
            spdx: "CC-BY-SA-4.0".into(),
        })
    });
    let err = validate_all_pack_manifests(temp.path()).unwrap_err();
    assert!(err.contains("must not carry language-model"), "{err}");

    build_all_packs(temp.path());
    validate_all_pack_manifests(temp.path()).unwrap();
}

#[test]
fn test_writer_rejects_out_of_range_ids_and_unsorted_vocabulary() {
    let temp = tempdir().unwrap();
    let bad_ids = LanguageModelContent {
        vocabulary: vec!["a".into(), "b".into()],
        unigrams: vec![],
        bigrams: vec![(0, 7, 1, 1, 1_000_000)],
        trigrams: vec![],
    };
    assert!(write_language_model(temp.path(), test_manifest("m"), &bad_ids).is_err());
    let unsorted = LanguageModelContent {
        vocabulary: vec!["b".into(), "a".into()],
        ..Default::default()
    };
    assert!(write_language_model(temp.path(), test_manifest("m"), &unsorted).is_err());
    let bad_probability = LanguageModelContent {
        vocabulary: vec!["a".into(), "b".into()],
        bigrams: vec![(0, 1, 1, 2, 1_000_000)],
        ..Default::default()
    };
    assert!(write_language_model(temp.path(), test_manifest("m"), &bad_probability).is_err());
    let duplicate_trigram = LanguageModelContent {
        vocabulary: vec!["a".into(), "b".into()],
        trigrams: vec![(0, 1, 0, 1, 1, 1_000_000), (0, 1, 0, 1, 1, 1_000_000)],
        ..Default::default()
    };
    assert!(write_language_model(temp.path(), test_manifest("m"), &duplicate_trigram).is_err());
}

#[test]
fn test_writer_rejects_multi_token_vocabulary_entries() {
    // The non-prose guarantee requires single tokens: an entry with whitespace could
    // never match tokenizer output and would smuggle a readable phrase into the artifact.
    let temp = tempdir().unwrap();
    for bad in ["a b", "a\tb", "a\u{0}b", ""] {
        let content = LanguageModelContent {
            vocabulary: vec![bad.to_string(), "zz".into()],
            ..Default::default()
        };
        let err = write_language_model(temp.path(), test_manifest("m"), &content)
            .expect_err(&format!("vocabulary entry {bad:?} must be rejected"));
        assert!(err.contains("single non-empty tokens"), "{err}");
    }
}
