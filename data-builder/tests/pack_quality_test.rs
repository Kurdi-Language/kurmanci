//! Pack-quality measurement against the committed fixture pack, bound to a synthetic
//! provenance chain in a temporary root.
//!
//! The corruption classes are tested directly, because a corruption that silently does nothing
//! would be counted as a trivially restored word and would inflate every correction rate. The
//! end-to-end tests then prove the accounting, that targets do not depend on what the pack
//! knows (otherwise a smaller pack would be measured on an easier set), and that the shared
//! provenance chain is actually traversed.
mod common;

use data_builder_lib::eval_pack_quality::{
    evaluate_pack_quality, Corruption, PACK_QUALITY_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const MODEL: &str = "test-model";
const CORPUS: &str = "test-corpus";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn record(partition: &str, doc: &str, text: &str) -> String {
    format!(
        r#"{{"partition":"{partition}","corpus_id":"{CORPUS}","document_id":"{doc}","duplicate_group_id":"g-{doc}","is_duplicate":false,"canonical_corpus_id":"{CORPUS}","canonical_document_id":"{doc}","source_file":"c.txt","source_sha256":"00","text":"{text}"}}"#
    )
}

struct Fixture {
    root: tempfile::TempDir,
    pack: PathBuf,
}

impl Fixture {
    fn new(partition: &str, records: &[String]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let partitions = r.join("data/build/corpus-partitions");
        fs::create_dir_all(&partitions).unwrap();
        let (dev, eval) = if partition == "development" {
            (records.len(), 0)
        } else {
            (0, records.len())
        };
        fs::write(
            partitions.join("manifest.json"),
            format!(
                r#"{{"partition_policy_version":"kurmanci-partition-v1","train_documents":1,"development_documents":{dev},"evaluation_documents":{eval}}}"#
            ),
        )
        .unwrap();
        let partition_manifest_sha = sha(&fs::read(partitions.join("manifest.json")).unwrap());
        fs::write(
            partitions.join(format!("{partition}.jsonl")),
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
}

// ---------------------------------------------------------------------------
// Corruption classes
// ---------------------------------------------------------------------------

#[test]
fn every_corruption_actually_corrupts_and_is_deterministic() {
    for corruption in Corruption::ALL {
        // "pirtûk" carries a diacritic and has no doubled middle letter, so every class applies.
        let first = corruption
            .apply("pirtûk")
            .unwrap_or_else(|| panic!("{:?} did not apply to pirtûk", corruption));
        assert_ne!(
            first, "pirtûk",
            "{:?} returned the word unchanged, which would count as a free restoration",
            corruption
        );
        // Deterministic: no seed, no iteration order, position derived from the word.
        for _ in 0..3 {
            assert_eq!(corruption.apply("pirtûk").unwrap(), first);
        }
    }
}

#[test]
fn a_corruption_that_would_be_a_no_op_is_refused() {
    // Stripping diacritics from a word that has none changes nothing: counting that as a
    // corruption would hand every pack a free restoration.
    assert_eq!(Corruption::StripDiacritics.apply("welat"), None);
    // Transposing two identical letters is likewise no corruption at all.
    assert_eq!(Corruption::TransposeTwoLetters.apply("rojja"), None);
    // Words too short to corrupt meaningfully are refused by every class.
    for corruption in Corruption::ALL {
        assert_eq!(
            corruption.apply("roj"),
            None,
            "{:?} on a 3-letter word",
            corruption
        );
    }
}

#[test]
fn each_corruption_is_the_single_edit_it_claims_to_be() {
    let word = "pirtûk";
    let n = word.chars().count();
    assert_eq!(
        Corruption::DeleteOneLetter
            .apply(word)
            .unwrap()
            .chars()
            .count(),
        n - 1
    );
    assert_eq!(
        Corruption::DuplicateOneLetter
            .apply(word)
            .unwrap()
            .chars()
            .count(),
        n + 1
    );
    assert_eq!(
        Corruption::SubstituteOneLetter
            .apply(word)
            .unwrap()
            .chars()
            .count(),
        n
    );
    let transposed = Corruption::TransposeTwoLetters.apply(word).unwrap();
    assert_eq!(transposed.chars().count(), n);
    let mut a: Vec<char> = word.chars().collect();
    let mut b: Vec<char> = transposed.chars().collect();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(
        a, b,
        "a transposition must not change which letters are present"
    );
}

// ---------------------------------------------------------------------------
// Accounting
// ---------------------------------------------------------------------------

#[test]
fn coverage_counts_occurrences_and_forms_separately() {
    // "pirtûk" three times, "heval" once, and one form the pack does not contain.
    let f = Fixture::new(
        "development",
        &[
            record("development", "d1", "pirtûk heval pirtûk"),
            record("development", "d2", "pirtûk zzzqqqwww"),
        ],
    );
    let r = evaluate_pack_quality(f.root(), &f.pack, "development", 100).unwrap();

    assert_eq!(r.schema_version, PACK_QUALITY_SCHEMA_VERSION);
    assert_eq!(r.corpus.documents_read, 2);
    assert_eq!(r.corpus.documents_evaluated, 2);
    assert_eq!(
        r.known_words.token_occurrences, 5,
        "five eligible tokens in total"
    );
    assert_eq!(
        r.known_words.distinct_forms, 3,
        "pirtûk, heval and the unknown form, all eligible"
    );
    // A common word counts once per occurrence, which is what a person typing would feel.
    assert!(
        r.known_words.token_occurrences_known >= 3,
        "pirtûk occurs three times and is in the pack: {:?}",
        r.known_words
    );
    assert!(r.known_words.distinct_forms_known < r.known_words.distinct_forms);
}

#[test]
fn targets_do_not_depend_on_what_the_pack_knows() {
    // A form absent from the pack must still become a target; otherwise a smaller pack would be
    // measured on an easier set and no comparison between packs would mean anything.
    let f = Fixture::new(
        "development",
        &[record("development", "d1", "pirtûk zzzqqqwww heval")],
    );
    let r = evaluate_pack_quality(f.root(), &f.pack, "development", 100).unwrap();

    assert_eq!(
        r.clean_input.targets, 3,
        "every eligible form is a target, known or not"
    );
    assert_eq!(
        r.clean_input.targets_known + r.clean_input.treated_as_unknown,
        r.clean_input.targets
    );
    assert!(
        r.clean_input.treated_as_unknown >= 1,
        "the unknown form must be counted as unknown, not dropped"
    );
    // Its completion row carries the same target count, so a missing target is a miss.
    let four = r.completion.iter().find(|c| c.prefix_chars == 4).unwrap();
    assert_eq!(four.targets, 3);
}

#[test]
fn top_k_rates_are_nested_and_bounded_by_the_targets() {
    let f = Fixture::new(
        "development",
        &[record("development", "d1", "pirtûk heval azadî kurmancî")],
    );
    let r = evaluate_pack_quality(f.root(), &f.pack, "development", 100).unwrap();

    for c in &r.completion {
        assert!(c.top1 <= c.top3 && c.top3 <= c.top5, "completion {:?}", c);
        assert!(c.top5 <= c.targets, "completion {:?}", c);
        assert!(c.candidates_returned <= c.targets * r.provenance.result_limit);
    }
    for c in &r.correction {
        assert!(c.top1 <= c.top3 && c.top3 <= c.top5, "correction {:?}", c);
        assert!(c.top5 <= c.measured, "correction {:?}", c);
        assert_eq!(
            c.measured + c.corruption_is_known_word,
            c.targets,
            "every target is either measured or set aside as already a word: {:?}",
            c
        );
    }
    // The ceiling the report prints is the share of targets the pack knows.
    assert!(r.clean_input.targets_known <= r.clean_input.targets);
}

#[test]
fn a_known_word_is_completed_and_restored_and_left_alone() {
    let f = Fixture::new("development", &[record("development", "d1", "pirtûk")]);
    let r = evaluate_pack_quality(f.root(), &f.pack, "development", 100).unwrap();

    assert_eq!(r.clean_input.targets, 1);
    assert_eq!(
        r.clean_input.targets_known, 1,
        "pirtûk is in the fixture pack"
    );
    assert_eq!(
        r.clean_input.corrected_away_from_input, 0,
        "a correctly spelled known word must be answered with itself"
    );
    let four = r.completion.iter().find(|c| c.prefix_chars == 4).unwrap();
    assert_eq!(four.top5, 1, "pirt… must complete to pirtûk within five");
    // At least one corruption class restores it.
    assert!(
        r.correction.iter().any(|c| c.top5 == 1),
        "no corruption of pirtûk was restored: {:?}",
        r.correction
    );
}

// ---------------------------------------------------------------------------
// The shared provenance chain is traversed
// ---------------------------------------------------------------------------

#[test]
fn the_train_partition_is_refused() {
    let f = Fixture::new("development", &[record("development", "d1", "pirtûk")]);
    let err = evaluate_pack_quality(f.root(), &f.pack, "train", 100)
        .expect_err("measuring on the partition that built the model must be refused");
    assert!(
        err.contains("partition must be 'development' or 'evaluation'"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_pack_that_does_not_match_its_manifest_is_refused() {
    let f = Fixture::new("development", &[record("development", "d1", "pirtûk")]);
    // Change the binary so it no longer matches the hash its manifest records.
    let mut bytes = fs::read(&f.pack).unwrap();
    bytes.push(0);
    fs::write(&f.pack, &bytes).unwrap();
    let err = evaluate_pack_quality(f.root(), &f.pack, "development", 100)
        .expect_err("a pack must not borrow another pack's manifest");
    assert!(
        err.contains("do not belong together") || err.contains("binary_sha256"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_zero_target_limit_is_refused() {
    let f = Fixture::new("development", &[record("development", "d1", "pirtûk")]);
    let err = evaluate_pack_quality(f.root(), &f.pack, "development", 0)
        .expect_err("a zero target limit measures nothing");
    assert!(err.contains("at least 1"), "unexpected error: {err}");
}

// ---------------------------------------------------------------------------
// The eligibility boundary is the production one
// ---------------------------------------------------------------------------

#[test]
fn forms_production_excludes_are_out_of_the_denominator_and_out_of_the_targets() {
    // "kurdî2" carries a digit, so the alphabet policy excludes it. "dest-pêk" and "bav'ê"
    // carry word-internal punctuation, which the alphabet helper deliberately permits because
    // the punctuation hold governs it: a measurement that checked only the alphabet would make
    // them targets even though the reviewed pack may not contain them until a linguist has
    // reviewed them. The tokenizer splits on punctuation, so those two arrive already split;
    // the digit case is the one a corpus can actually produce.
    let f = Fixture::new(
        "development",
        &[record("development", "d1", "pirtûk kurdî2 heval")],
    );
    let r = evaluate_pack_quality(f.root(), &f.pack, "development", 100).unwrap();

    assert_eq!(
        r.known_words.distinct_forms, 2,
        "only pirtûk and heval are eligible: {:?}",
        r.known_words
    );
    assert_eq!(r.known_words.token_occurrences, 2);
    assert_eq!(r.known_words.excluded_out_of_alphabet_forms, 1);
    assert_eq!(r.known_words.excluded_out_of_alphabet_occurrences, 1);
    assert_eq!(
        r.clean_input.targets, 2,
        "an ineligible form must not become a target"
    );
}

#[test]
fn a_punctuation_held_form_is_never_a_target() {
    // Proven at the boundary itself, since the corpus tokenizer cannot emit such a token: the
    // alphabet policy admits these forms and the punctuation hold is what keeps them out.
    for held in ["dest-pêk", "bav'ê", "bav’ê"] {
        assert!(
            data_builder_lib::alphabet::default_pack_eligibility(held).is_ok(),
            "{held} is expected to pass the alphabet policy"
        );
        assert!(
            data_builder_lib::alphabet::word_punctuation_hold(held).is_err(),
            "{held} must be held by the punctuation policy, which is what excludes it"
        );
    }
}

#[test]
fn a_pack_whose_manifest_declares_no_identity_is_refused() {
    // A report identifies its pack by the declared pack_id and hash, never by the invocation
    // path, so a manifest without an identity cannot be measured.
    let f = Fixture::new("development", &[record("development", "d1", "pirtûk")]);
    let manifest = f.pack.parent().unwrap().join("manifest.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, text.replace(r#""pack_id":"reviewed","#, "")).unwrap();
    let err = evaluate_pack_quality(f.root(), &f.pack, "development", 100)
        .expect_err("a manifest without a pack_id must be refused");
    assert!(
        err.contains("declares no pack_id"),
        "unexpected error: {err}"
    );
}

#[test]
fn the_report_carries_the_declared_identity_and_no_invocation_path() {
    let f = Fixture::new("development", &[record("development", "d1", "pirtûk")]);
    let r = evaluate_pack_quality(f.root(), &f.pack, "development", 100).unwrap();
    assert_eq!(r.provenance.pack_id, "reviewed");
    let json = serde_json::to_string(&r).unwrap();
    assert!(
        !json.contains(&f.root().display().to_string()),
        "the report leaked the temporary root, so a committed report would be machine-specific"
    );
}
