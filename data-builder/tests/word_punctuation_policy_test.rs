//! The word-punctuation policy (project owner, 2026-09-19): a form containing a hyphen or an
//! apostrophe is held for linguist review and never approved into the default vocabulary;
//! punctuation-only variants are flagged, never merged, never both admitted. Proven at the
//! rule, at authoritative resolution (fail closed for every source), at the default-vocabulary
//! boundary, in the generated queues of the repository and in the corrected decision store.

use data_builder_lib::alphabet::{
    punctuation_stripped_form, word_punctuation_hold, HELD_FOR_LINGUIST_ACTION,
    POSSIBLE_DUPLICATE_REASON_PREFIX, PUNCTUATION_HELD_QUEUE_FILE, WORD_PUNCTUATION_REASON_CODE,
};
use data_builder_lib::pack::selection::{
    apply_default_pack_word_punctuation_policy, EntryPopulation, SelectedCandidate,
};
use data_builder_lib::validate::validate_entry;
use data_builder_lib::SourceLexiconEntry;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

fn ws_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn candidate(
    normalized: &str,
    population: EntryPopulation,
    source_id: &str,
    status: &str,
) -> SelectedCandidate {
    SelectedCandidate {
        entry_id: format!("id-{}", normalized),
        display: normalized.to_string(),
        normalized: normalized.to_string(),
        population,
        source_id: source_id.to_string(),
        source_lines: vec![],
        flags: String::new(),
        morphology: vec![],
        part_of_speech: "unknown".to_string(),
        status: status.to_string(),
    }
}

#[test]
fn rule_holds_hyphen_and_both_apostrophes_and_strips_them_for_duplicate_flags() {
    assert!(word_punctuation_hold("rojbaş").is_ok());
    assert_eq!(word_punctuation_hold("'azîm"), Err(vec!['\'']));
    assert_eq!(word_punctuation_hold("bin-av"), Err(vec!['-']));
    assert_eq!(
        word_punctuation_hold("be\u{2019}ecok"),
        Err(vec!['\u{2019}'])
    );
    assert_eq!(punctuation_stripped_form("'azîm"), "azîm");
    assert_eq!(punctuation_stripped_form("bi-nan-û-xwê"), "binanûxwê");
}

#[test]
fn approved_held_form_fails_authoritative_resolution_for_every_source() {
    use EntryPopulation::*;
    for (source, population, form) in [
        ("kurdish-hunspell-kmr", ExternalApproved, "'azîm"),
        ("kuwiki-batch-001", ExternalApproved, "bin-av"),
        (
            "future-source-xyz",
            ExternalApprovedMetadataChange,
            "hew-hew",
        ),
        ("manual-seed", ManualSeed, "be\u{2019}ecok"),
    ] {
        let err = apply_default_pack_word_punctuation_policy(
            "reviewed",
            &[
                candidate("rojbaş", ExternalApproved, source, "approved"),
                candidate(form, population, source, "approved"),
            ],
        )
        .unwrap_err();
        assert!(
            err.contains("production lexical eligibility violation"),
            "{err}"
        );
        assert!(err.contains(form) && err.contains(source), "{err}");
        assert!(err.contains("held for linguist review"), "{err}");
    }
    // Undecided or experimental-only evidence in the reservoir is not gated.
    apply_default_pack_word_punctuation_policy(
        "experimental-full",
        &[
            candidate(
                "'azîm",
                EntryPopulation::ExternalUnreviewed,
                "kurdish-hunspell-kmr",
                "unreviewed",
            ),
            candidate(
                "bin-av",
                EntryPopulation::ExternalExperimentalOnly,
                "kurdish-hunspell-kmr",
                "experimental_only",
            ),
        ],
    )
    .unwrap();
}

#[test]
fn punctuation_only_variants_cannot_both_enter_the_default_vocabulary() {
    use EntryPopulation::*;
    // Two candidates identical once the held punctuation is removed: refused even though the
    // punctuation-free one is, on its own, eligible.
    let err = apply_default_pack_word_punctuation_policy(
        "reviewed",
        &[
            candidate("azîm", ExternalApproved, "kurdish-hunspell-kmr", "approved"),
            candidate("'azîm", ManualSeed, "manual-seed", "approved"),
        ],
    )
    .unwrap_err();
    assert!(err.contains("possible duplicate lexical words"), "{err}");
    assert!(err.contains("never merged automatically"), "{err}");
    // The punctuation-free form alone passes.
    apply_default_pack_word_punctuation_policy(
        "reviewed",
        &[candidate(
            "azîm",
            ExternalApproved,
            "kurdish-hunspell-kmr",
            "approved",
        )],
    )
    .unwrap();
}

#[test]
fn default_vocabulary_boundary_refuses_held_forms() {
    let mut entry = SourceLexiconEntry {
        word: "bin-av".into(),
        normalized: "bin-av".into(),
        lemma: "bin-av".into(),
        part_of_speech: "noun".into(),
        frequency: 0,
        status: "approved".into(),
        regions: vec!["general".into()],
        sources: vec!["manual-seed".into()],
        variants: vec![],
        frequency_metadata: None,
    };
    let err = validate_entry(&entry, 1).unwrap_err();
    assert!(err.contains("held for linguist review"), "{err}");
    entry.word = "binav".into();
    entry.normalized = "binav".into();
    entry.lemma = "binav".into();
    validate_entry(&entry, 1).unwrap();
}

#[test]
fn repository_queues_hold_every_punctuation_form_with_duplicate_flags_and_the_pool_has_none() {
    let qdir = ws_root().join("data/review-queues/kurdish-hunspell-kmr");
    let held: Vec<serde_json::Value> = fs::read_to_string(qdir.join(PUNCTUATION_HELD_QUEUE_FILE))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(!held.is_empty());
    let mut with_flags = 0;
    for rec in &held {
        let normalized = rec["normalized"].as_str().unwrap();
        assert!(word_punctuation_hold(normalized).is_err(), "{normalized}");
        assert_eq!(rec["suggested_action"], HELD_FOR_LINGUIST_ACTION);
        let codes: Vec<&str> = rec["reason_codes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .collect();
        assert!(
            codes.contains(&WORD_PUNCTUATION_REASON_CODE),
            "{normalized}: {codes:?}"
        );
        assert!(
            codes.iter().any(|c| c.contains("U+")),
            "{normalized}: {codes:?}"
        );
        if codes
            .iter()
            .any(|c| c.starts_with(POSSIBLE_DUPLICATE_REASON_PREFIX))
        {
            with_flags += 1;
        }
    }
    assert!(
        with_flags > 0,
        "some held forms have punctuation-free twins in the import"
    );
    // The ordinary pool carries no held form any more.
    for line in fs::read_to_string(qdir.join("hunspell-only.jsonl"))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
    {
        let rec: serde_json::Value = serde_json::from_str(line).unwrap();
        let normalized = rec["normalized"].as_str().unwrap();
        assert!(
            word_punctuation_hold(normalized).is_ok(),
            "pool still holds {normalized}"
        );
    }
    // The held targets are exactly the pool-external set: no target is in both files.
    let held_ids: BTreeSet<String> = held
        .iter()
        .map(|r| r["target_id"].as_str().unwrap().to_string())
        .collect();
    for line in fs::read_to_string(qdir.join("hunspell-only.jsonl"))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
    {
        let rec: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(!held_ids.contains(rec["target_id"].as_str().unwrap()));
    }
}

#[test]
fn corrected_decision_store_admits_no_held_form_and_preserves_the_previous_decision() {
    let decisions = fs::read_to_string(
        ws_root().join("data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl"),
    )
    .unwrap();
    let mut corrected = 0;
    for line in decisions.lines().filter(|l| !l.trim().is_empty()) {
        let d: serde_json::Value = serde_json::from_str(line).unwrap();
        if d["target_id"] == "01ba53649f4926366a288cd5e8a29552ce18893d1fe1ebb95d9edff0944bcaf1" {
            assert_eq!(d["review_status"], "needs_linguist");
            assert_eq!(d["review_date"], "2026-09-19");
            let notes = d["review_notes"].as_str().unwrap();
            assert!(notes.contains("word-punctuation policy") && notes.contains("2026-08-24"));
            let evidence: Vec<&str> = d["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e.as_str().unwrap())
                .collect();
            assert!(evidence.iter().any(|e| e.starts_with(
                "previous-decision:status=approved;reviewer=ferhatguneri;date=2026-08-24;policy=word-punctuation-2026-09-19"
            )), "{evidence:?}");
            corrected += 1;
        }
    }
    assert_eq!(corrected, 1);
}
