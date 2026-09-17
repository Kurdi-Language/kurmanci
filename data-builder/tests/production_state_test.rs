//! `verify-production-state` is read-only and fail-closed over the complete pack artifact
//! set; `rebuild-production` verifies the authoritative inputs before it mutates anything,
//! refuses when its inputs are missing or invalid, and never touches review artifacts.

use data_builder_lib::pack::{assemble_pack_artifacts, build_pack, PACK_ARTIFACT_FILES};
use data_builder_lib::production::{
    abbrev, duplicate_identity_report, rebuild_production, render_state_text,
    verify_production_state, CheckStatus,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn ws_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

/// Recursive listing of (path, sha256, mtime) for every file under `dir`.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (String, std::time::SystemTime)> {
    fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, (String, std::time::SystemTime)>) {
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if let (Ok(bytes), Ok(meta)) = (fs::read(&p), fs::metadata(&p)) {
                    use sha2::Digest;
                    let sha = format!("{:x}", sha2::Sha256::digest(&bytes));
                    out.insert(p, (sha, meta.modified().unwrap()));
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, &mut out);
    out
}

fn copy_tree(src: &Path, dst: &Path) {
    if src.is_dir() {
        fs::create_dir_all(dst).unwrap();
        for e in fs::read_dir(src).unwrap().flatten() {
            copy_tree(&e.path(), &dst.join(e.file_name()));
        }
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::copy(src, dst).unwrap();
    }
}

/// A temp root holding a copy of the repository's authoritative inputs (registries, policy,
/// seed lexicon, review queues/reports/decisions/batches, committed language model) but no
/// corpus files and no built packs: the authoritative inputs verify, the outputs are absent.
fn authoritative_inputs_copy() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for rel in [
        "data/reviewed",
        "data/review-decisions",
        "data/review-queues",
        "data/review-batches",
        "data/reports/controlled-lexicon-review",
        "data/source-registry",
        "data/pack-policy.toml",
        "data/language-model",
        "data-builder/config/ngrams.toml",
    ] {
        let src = ws_root().join(rel);
        assert!(src.exists(), "{} missing in the workspace", rel);
        copy_tree(&src, &temp.path().join(rel));
    }
    temp
}

fn check_status(
    report: &data_builder_lib::production::ProductionStateReport,
    name: &str,
) -> CheckStatus {
    report
        .checks
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("check {} missing", name))
        .status
}

#[test]
fn workspace_production_state_is_ok_and_verification_writes_nothing() {
    let root = ws_root();
    let watched = [
        root.join("data/reviewed"),
        root.join("data/review-decisions"),
        root.join("data/review-queues"),
        root.join("data/review-batches"),
        root.join("data/language-model"),
        root.join("data/source-registry"),
        root.join("data/pack-policy.toml"),
        root.join("data/build/packs"),
    ];
    let before: Vec<_> = watched.iter().map(|p| snapshot(p)).collect();
    let report = verify_production_state(root).unwrap();
    let after: Vec<_> = watched.iter().map(|p| snapshot(p)).collect();
    assert_eq!(before, after, "verification must not modify any data file");

    let text = render_state_text(&report);
    assert!(report.ok, "{}", text);
    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    for expected in [
        "source-registry",
        "corpus-registry",
        "pack-policy",
        "hunspell-review",
        "kuwiki-review",
        "review-identities-unique",
        "trust-subsets",
        "policy-model-set",
        "pack-reproducible:seed",
        "pack-reproducible:reviewed",
        "pack-reproducible:experimental-full",
        "pack-manifests",
    ] {
        assert!(
            names.contains(&expected),
            "missing check {}: {}",
            expected,
            text
        );
    }
    assert!(report
        .checks
        .iter()
        .any(|c| c.name.starts_with("language-model:") && c.status == CheckStatus::Pass));
    assert_eq!(check_status(&report, "pack-manifests"), CheckStatus::Pass);

    // Every built artifact of every pack equals the in-memory assembly.
    assert_eq!(report.packs.len(), 3);
    for p in &report.packs {
        assert_eq!(p.built_matches, Some(true), "{}", p.pack_id);
        assert_eq!(
            p.artifacts.len(),
            PACK_ARTIFACT_FILES.len(),
            "{}",
            p.pack_id
        );
        for a in &p.artifacts {
            assert_eq!(a.built_matches, Some(true), "{}/{}", p.pack_id, a.name);
            assert_eq!(
                a.built_sha256.as_deref(),
                Some(a.in_memory_sha256.as_str()),
                "{}/{}",
                p.pack_id,
                a.name
            );
        }
    }
    assert!(report.language_model.is_some());
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("\"ok\":true"));
}

#[test]
fn verification_reports_every_failure_instead_of_stopping_at_the_first() {
    // An empty root fails closed on every check that has an input, with a reason each.
    let temp = tempfile::tempdir().unwrap();
    let report = verify_production_state(temp.path()).unwrap();
    assert!(!report.ok);
    let failed: Vec<&str> = report
        .checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .map(|c| c.name.as_str())
        .collect();
    for expected in [
        "source-registry",
        "corpus-registry",
        "pack-policy",
        "hunspell-review",
        "trust-subsets",
        "policy-model-set",
        "pack-reproducible:seed",
    ] {
        assert!(failed.contains(&expected), "{:?}", failed);
    }
    for c in &report.checks {
        assert!(!c.detail.is_empty(), "{} has no reason", c.name);
    }
    assert!(report.packs.is_empty());
    assert!(
        fs::read_dir(temp.path()).unwrap().next().is_none(),
        "nothing written"
    );
}

#[test]
fn complete_artifact_set_is_reproducible_and_stale_metadata_is_detected() {
    let temp = authoritative_inputs_copy();
    let root = temp.path();

    // Two complete in-memory assemblies are byte-identical in every file.
    let a = assemble_pack_artifacts("seed", root).unwrap();
    let b = assemble_pack_artifacts("seed", root).unwrap();
    for name in PACK_ARTIFACT_FILES {
        assert_eq!(a.files[name], b.files[name], "{}", name);
    }

    // A built pack consists of exactly the assembled bytes.
    build_pack("seed", root).unwrap();
    let pack_dir = root.join("data/build/packs/seed");
    for name in PACK_ARTIFACT_FILES {
        assert_eq!(
            fs::read(pack_dir.join(name)).unwrap(),
            a.files[name],
            "{}",
            name
        );
    }
    let report = verify_production_state(root).unwrap();
    assert_eq!(
        check_status(&report, "pack-reproducible:seed"),
        CheckStatus::Pass,
        "{}",
        render_state_text(&report)
    );
    let seed = report.packs.iter().find(|p| p.pack_id == "seed").unwrap();
    assert_eq!(seed.built_matches, Some(true));
    assert!(seed.artifacts.iter().all(|a| a.built_matches == Some(true)));
    let lexicon_on_disk = seed.artifacts[0].built_sha256.clone().unwrap();

    // Change source attribution metadata only: the lexical content is untouched, so
    // lexicon.bin is unchanged, but the built attribution/manifest are now stale.
    let sources_path = root.join("data/source-registry/sources.toml");
    let sources = fs::read_to_string(&sources_path).unwrap();
    let seed_section = sources.find("source_id = \"manual-seed\"").unwrap();
    let author_at = seed_section + sources[seed_section..].find("author = \"").unwrap();
    let author_end = author_at + sources[author_at..].find('\n').unwrap();
    let edited = format!(
        "{}author = \"A different attribution line\"{}",
        &sources[..author_at],
        &sources[author_end..]
    );
    assert_ne!(edited, sources);
    fs::write(&sources_path, edited).unwrap();

    let report = verify_production_state(root).unwrap();
    let check = report
        .checks
        .iter()
        .find(|c| c.name == "pack-reproducible:seed")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Fail, "{}", check.detail);
    assert!(check.detail.contains("stale"), "{}", check.detail);
    assert!(check.detail.contains("attribution.txt"), "{}", check.detail);
    let seed = report.packs.iter().find(|p| p.pack_id == "seed").unwrap();
    assert_eq!(seed.built_matches, Some(false));
    let by_name: BTreeMap<&str, &data_builder_lib::production::PackArtifactState> = seed
        .artifacts
        .iter()
        .map(|a| (a.name.as_str(), a))
        .collect();
    assert_eq!(by_name["lexicon.bin"].built_matches, Some(true));
    assert_eq!(by_name["lexicon.bin"].in_memory_sha256, lexicon_on_disk);
    assert_eq!(by_name["attribution.txt"].built_matches, Some(false));
    assert_eq!(by_name["artifacts.sha256"].built_matches, Some(false));
    assert_eq!(
        check_status(&report, "pack-manifests"),
        CheckStatus::Skipped
    );
    assert!(!report.ok);
}

#[test]
fn malformed_or_short_identities_produce_a_fail_result_not_a_panic() {
    assert_eq!(abbrev(""), "");
    assert_eq!(abbrev("short"), "short");
    assert_eq!(abbrev("aaaaaaaaaaaç"), "aaaaaaaaaaaç"); // byte 12 is inside a multibyte char
    assert_eq!(abbrev("0123456789abcdef"), "0123456789ab");

    let mut identities = BTreeMap::new();
    identities.insert(("kurdish-hunspell-kmr".to_string(), "ab".to_string()), 2);
    identities.insert(("kuwiki-batch-001".to_string(), "çêîşû".to_string()), 3);
    identities.insert(("kuwiki-batch-001".to_string(), String::new()), 2);
    identities.insert(("kuwiki-batch-002".to_string(), "x".repeat(64)), 1);
    let err = duplicate_identity_report(&identities).unwrap_err();
    assert!(err.contains("kurdish-hunspell-kmr:ab x2"), "{}", err);
    assert!(err.contains("kuwiki-batch-001:çêîşû x3"), "{}", err);
    assert!(err.contains("kuwiki-batch-001: x2"), "{}", err);
    assert!(!err.contains("kuwiki-batch-002"), "{}", err);

    identities.retain(|_, n| *n == 1);
    assert!(duplicate_identity_report(&identities).is_ok());
}

#[test]
fn rebuild_refuses_without_the_local_corpus_and_changes_nothing() {
    // The authoritative inputs verify; only the corpus is absent.
    let temp = authoritative_inputs_copy();
    let root = temp.path();
    let before = snapshot(root);
    let err = rebuild_production(root, false).unwrap_err();
    assert!(err.contains("not present locally"), "{}", err);
    assert!(err.contains("--acquire"), "{}", err);
    assert!(err.contains("nothing was changed"), "{}", err);
    assert_eq!(
        snapshot(root),
        before,
        "a refused rebuild must not write anything"
    );
    assert!(!root.join("data/original").exists());
    assert!(!root.join("data/imported").exists());
    assert!(!root.join("data/build").exists());
}

#[test]
fn rebuild_refuses_with_invalid_authoritative_review_state_and_writes_nothing() {
    let temp = authoritative_inputs_copy();
    let root = temp.path();
    // Corrupt the committed Hunspell decisions: duplicate the first decision record.
    let decisions_path = root.join("data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl");
    let decisions = fs::read_to_string(&decisions_path).unwrap();
    let first = decisions.lines().next().unwrap().to_string();
    fs::write(&decisions_path, format!("{}{}\n", decisions, first)).unwrap();

    let before = snapshot(root);
    let err = rebuild_production(root, false).unwrap_err();
    assert!(
        err.contains("authoritative inputs failed verification"),
        "{}",
        err
    );
    assert!(err.contains("nothing was changed"), "{}", err);
    assert!(err.contains("FAIL"), "{}", err);
    assert!(
        !err.contains("--acquire"),
        "the preflight must run before the corpus presence check: {}",
        err
    );
    assert_eq!(
        snapshot(root),
        before,
        "a refused rebuild must not write anything"
    );
    assert!(!root.join("data/build").exists());
}

#[test]
fn rebuild_rejects_a_policy_model_set_the_registry_cannot_produce_before_mutation() {
    // One extra model id the registered corpora cannot produce.
    let temp = authoritative_inputs_copy();
    let root = temp.path();
    let policy_path = root.join("data/pack-policy.toml");
    let policy = fs::read_to_string(&policy_path).unwrap();
    let experimental = policy.find("[packs.experimental-full]").unwrap();
    let model_at = experimental + policy[experimental..].find("language_model = ").unwrap();
    let model_end = model_at + policy[model_at..].find('\n').unwrap();
    let edited = format!(
        "{}language_model = \"other-model-1\"{}",
        &policy[..model_at],
        &policy[model_end..]
    );
    fs::write(&policy_path, edited).unwrap();

    let before = snapshot(root);
    let err = rebuild_production(root, false).unwrap_err();
    assert!(err.contains("policy-model-set"), "{}", err);
    assert!(err.contains("other-model-1"), "{}", err);
    assert!(err.contains("nothing was changed"), "{}", err);
    assert_eq!(snapshot(root), before);
    assert!(!root.join("data/build").exists());

    // A registry whose Kuwiki version does not match the policy's model id.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("data/source-registry")).unwrap();
    fs::write(
        root.join("data/source-registry/corpora.toml"),
        r#"
[[corpora]]
corpus_id = "kuwiki"
corpus_name = "Kuwiki"
language = "ku-Latn"
license = "CC BY-SA 4.0"
license_spdx = "CC-BY-SA-4.0"
license_url = "https://creativecommons.org/licenses/by-sa/4.0/"
url = "https://example.org"
version = "20990101"
description = "test"
attribution = "test"
notes = "test"
document_format = "jsonl"
document_id_field = "page_id"
text_field = "text"
acquisition = "external"

[corpora.source_artifact]
url = "https://example.org/dump.xml.bz2"
path = "data/original/kuwiki/dump.xml.bz2"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
extractor = "wikimedia-xml"

[[corpora.files]]
path = "data/imported/kuwiki/documents.jsonl"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
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
model_profile = "prediction"
language_model = "kuwiki-20260801"

[packs.experimental-full]
description = "Experimental"
opt_in = true
allow_as_default = false
model_profile = "none"
"#,
    )
    .unwrap();
    let before = snapshot(root);
    let err = rebuild_production(root, false).unwrap_err();
    assert!(err.contains("kuwiki-20990101"), "{}", err);
    assert!(err.contains("kuwiki-20260801"), "{}", err);
    assert!(err.contains("nothing was changed"), "{}", err);
    assert_eq!(snapshot(root), before);
}

/// The committed language model records the n-gram pruning configuration it was built with;
/// a different `data-builder/config/ngrams.toml` makes the model stale and must fail closed
/// (a test once left a modified configuration behind and the model silently disagreed).
#[test]
fn language_model_built_with_another_ngram_config_fails_closed() {
    let temp = authoritative_inputs_copy();
    let root = temp.path();
    let config = root.join("data-builder/config/ngrams.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();

    // Same configuration as the workspace: the model check passes on this copy.
    fs::copy(ws_root().join("data-builder/config/ngrams.toml"), &config).unwrap();
    let report = verify_production_state(root).unwrap();
    let model_check = report
        .checks
        .iter()
        .find(|c| c.name.starts_with("language-model:"))
        .expect("language model check present");
    assert_eq!(
        model_check.status,
        CheckStatus::Pass,
        "{}",
        model_check.detail
    );

    // A different pruning configuration: the same model is now stale.
    fs::write(
        &config,
        "[bigram]\nminimum_count = 3\nmaximum_predictions_per_context = 8\n",
    )
    .unwrap();
    let report = verify_production_state(root).unwrap();
    let model_check = report
        .checks
        .iter()
        .find(|c| c.name.starts_with("language-model:"))
        .unwrap();
    assert_eq!(
        model_check.status,
        CheckStatus::Fail,
        "{}",
        model_check.detail
    );
    assert!(
        model_check.detail.contains("ngrams.toml"),
        "{}",
        model_check.detail
    );
    assert!(!report.ok);
}

/// A missing n-gram configuration cannot be verified against the committed model and must
/// fail closed rather than be skipped.
#[test]
fn missing_ngram_config_fails_the_language_model_check() {
    let temp = authoritative_inputs_copy();
    let root = temp.path();
    let config = root.join("data-builder/config/ngrams.toml");
    assert!(config.is_file());
    let report = verify_production_state(root).unwrap();
    let check = |r: &data_builder_lib::production::ProductionStateReport| {
        r.checks
            .iter()
            .find(|c| c.name.starts_with("language-model:"))
            .cloned()
            .expect("language model check present")
    };
    assert_eq!(
        check(&report).status,
        CheckStatus::Pass,
        "{}",
        check(&report).detail
    );

    fs::remove_file(&config).unwrap();
    let report = verify_production_state(root).unwrap();
    let c = check(&report);
    assert_eq!(c.status, CheckStatus::Fail, "{}", c.detail);
    assert!(c.detail.contains("ngrams.toml"), "{}", c.detail);
    assert!(c.detail.contains("missing or unreadable"), "{}", c.detail);
    assert!(!report.ok);
    assert!(!config.exists(), "verification writes nothing");
}
