//! Evaluation-pack membership rule and the guards that keep an evaluation artifact away from
//! the production packs.
//!
//! The output-directory guard is tested hard because an earlier version of it was wrong in a
//! way that silently passed: it normalized one side of the containment check and not the
//! other, so `--out data/build/packs/reviewed` was accepted and overwrote the production
//! `reviewed` pack. Every form of that path is now a test case.

use data_builder_lib::pack::evaluation::{in_evaluation_population, BASE_PACK_ID};
use data_builder_lib::pack::selection::EntryPopulation;
use data_builder_lib::pack::{assemble_evaluation_pack, build_evaluation_pack};
use std::fs;
use std::path::{Path, PathBuf};

const EVAL_PACK_ID: &str = "reviewed-plus-train-attested";
const MODEL: &str = "test-model";

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// A root carrying only a pack policy, which is all the guards need: each refusal under test
/// happens before any corpus, review or pack artifact is read.
fn root_with_policy(base_model: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("data/pack-policy.toml"),
        &format!(
            r#"schema_version = "pack-policy-v1"
default_pack = "seed"

[packs.seed]
description = "seed"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.reviewed]
description = "reviewed"
opt_in = false
allow_as_default = true
model_profile = "prediction"
language_model = "{model}"

[packs.{base}]
description = "base"
opt_in = true
allow_as_default = false
model_profile = "prediction"
language_model = "{model}"
"#,
            base = BASE_PACK_ID,
            model = base_model
        ),
    );
    root
}

fn build_at(root: &Path, out: &Path) -> Result<(), String> {
    build_evaluation_pack(root, EVAL_PACK_ID, "test-corpus", "test-source", MODEL, out).map(|_| ())
}

// ---------------------------------------------------------------------------
// Membership rule
// ---------------------------------------------------------------------------

#[test]
fn human_decisions_are_respected_and_attestation_only_admits_undecided_entries() {
    use EntryPopulation::*;
    // What a human wrote or approved is in, whether or not the corpus happens to contain it.
    for population in [
        ManualSeed,
        SeedMetadataChange,
        ExternalApproved,
        ExternalApprovedMetadataChange,
    ] {
        assert!(
            in_evaluation_population(population, true),
            "{population:?} attested must be kept"
        );
        assert!(
            in_evaluation_population(population, false),
            "{population:?} unattested must still be kept: a human approved it"
        );
    }
    // A human decided this one is not for the reviewed pack. Corpus evidence does not
    // overturn a human decision, so attestation must not readmit it.
    assert!(!in_evaluation_population(ExternalExperimentalOnly, true));
    assert!(!in_evaluation_population(ExternalExperimentalOnly, false));
    // The undecided population is the only one attestation decides.
    assert!(in_evaluation_population(ExternalUnreviewed, true));
    assert!(!in_evaluation_population(ExternalUnreviewed, false));
}

// ---------------------------------------------------------------------------
// Output directory guard
// ---------------------------------------------------------------------------

#[test]
fn refuses_every_spelling_of_the_production_pack_directory() {
    let root = root_with_policy(MODEL);
    let r = root.path();
    fs::create_dir_all(r.join("data/build/packs/reviewed")).unwrap();

    // Relative, dot-relative, nested deeper, the packs root itself, and an absolute path:
    // every one of these resolves into or onto the production pack directory.
    let cases: Vec<PathBuf> = vec![
        PathBuf::from("data/build/packs/reviewed"),
        PathBuf::from("./data/build/packs/reviewed"),
        PathBuf::from("data/build/packs/reviewed/nested"),
        PathBuf::from("data/build/packs"),
        PathBuf::from("./data/build/packs"),
        r.join("data/build/packs/experimental-full"),
    ];
    for out in cases {
        let err = build_at(r, &out).expect_err(&format!("{out:?} must be refused"));
        assert!(
            err.contains("refusing to write an evaluation pack to"),
            "{out:?} was refused for another reason: {err}"
        );
    }
}

#[test]
fn refuses_an_output_directory_that_contains_the_production_packs() {
    // Writing to an ancestor of the pack directory would also place an evaluation artifact
    // where a release reads from.
    let root = root_with_policy(MODEL);
    let r = root.path();
    for out in ["data/build", "data", "."] {
        let err = build_at(r, Path::new(out)).expect_err(&format!("{out:?} must be refused"));
        assert!(
            err.contains("refusing to write an evaluation pack to"),
            "{out:?} was refused for another reason: {err}"
        );
    }
}

#[test]
fn refuses_a_traversing_output_directory() {
    let root = root_with_policy(MODEL);
    let err = build_at(root.path(), Path::new("dist/../data/build/packs/seed"))
        .expect_err("a traversing path must be refused");
    assert!(err.contains("containing '..'"), "unexpected error: {err}");
}

#[test]
fn accepts_an_output_directory_outside_the_pack_tree() {
    // The guard must pass this path through; the build then fails later for want of a corpus,
    // which proves the refusal came from the guard in the cases above and not from the setup.
    let root = root_with_policy(MODEL);
    let err = build_at(root.path(), Path::new("dist/evaluation-packs/x"))
        .expect_err("no corpus is present in this root");
    assert!(
        !err.contains("refusing to write an evaluation pack to"),
        "a legitimate output directory was refused by the guard: {err}"
    );
    assert!(
        !err.contains("containing '..'"),
        "a legitimate output directory was refused as traversing: {err}"
    );
}

#[test]
fn writes_nothing_when_the_guard_refuses() {
    let root = root_with_policy(MODEL);
    let r = root.path();
    let target = r.join("data/build/packs/reviewed");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("lexicon.bin"), b"production bytes").unwrap();

    build_at(r, Path::new("data/build/packs/reviewed")).expect_err("must be refused");

    assert_eq!(
        fs::read(target.join("lexicon.bin")).unwrap(),
        b"production bytes",
        "the refused run modified the production pack"
    );
    assert!(
        !target.join("provenance.json").exists(),
        "the refused run left an evaluation artifact behind"
    );
}

// ---------------------------------------------------------------------------
// Identity and model guards
// ---------------------------------------------------------------------------

#[test]
fn refuses_a_pack_id_that_is_an_authoritative_pack() {
    let root = root_with_policy(MODEL);
    for declared in ["seed", BASE_PACK_ID] {
        let err =
            assemble_evaluation_pack(root.path(), declared, "test-corpus", "test-source", MODEL)
                .expect_err("an authoritative pack name must be refused");
        assert!(
            err.contains("must not reuse the name of an authoritative pack"),
            "unexpected error for {declared}: {err}"
        );
    }
}

#[test]
fn refuses_a_model_the_base_pack_does_not_embed() {
    // Checked before the corpus walk, so this root needs no corpus at all.
    let root = root_with_policy("another-model");
    let err = assemble_evaluation_pack(
        root.path(),
        EVAL_PACK_ID,
        "test-corpus",
        "test-source",
        MODEL,
    )
    .expect_err("a model disagreement must be refused");
    assert!(
        err.contains("both must be the same model"),
        "unexpected error: {err}"
    );
}
