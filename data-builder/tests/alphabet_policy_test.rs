//! The default-pack alphabet policy (explicit human project policy, 2026-09-17): a lexical
//! candidate carrying characters outside the approved 31-letter Kurmancî alphabet is
//! mechanically ineligible for the default/reviewed pack. These tests prove the rule, the
//! normalization it relies on, the gate on the reviewed pack, that the experimental-full
//! reservoir and the source records are untouched, and that the audit decides nothing.

use data_builder_lib::alphabet::{
    default_pack_eligibility, is_kurmanci_letter, out_of_alphabet_chars, KURMANCI_ALPHABET,
};
use data_builder_lib::corpus::quality::classify_technical_noise;
use data_builder_lib::normalize_text;
use data_builder_lib::pack::builder::{
    resolve_authoritative_pack_lexicon, resolve_authoritative_pack_payload,
};
use data_builder_lib::pack::selection::{
    apply_default_pack_alphabet_policy, EntryPopulation, SelectedCandidate,
};
use data_builder_lib::review::alphabet_audit::{audit_alphabet, write_alphabet_audit};
use data_builder_lib::validate::{validate_entry, SourceLexiconEntry};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn ws_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn snapshot(dir: &Path) -> BTreeMap<PathBuf, String> {
    fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, String>) {
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if let Ok(bytes) = fs::read(&p) {
                    use sha2::Digest;
                    out.insert(p, format!("{:x}", sha2::Sha256::digest(&bytes)));
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, &mut out);
    out
}

#[test]
fn eligibility_rule_is_the_31_letters_with_punctuation_left_open() {
    assert_eq!(KURMANCI_ALPHABET.len(), 31);
    for c in ['ç', 'ê', 'î', 'ş', 'û'] {
        assert!(is_kurmanci_letter(c), "{} is a distinct Kurmancî letter", c);
    }
    assert_eq!(
        KURMANCI_ALPHABET.iter().collect::<BTreeSet<_>>().len(),
        31,
        "the five diacritic letters are distinct from their bases"
    );
    for ok in [
        "rojbaş",
        "kurmancî",
        "pirtûk",
        "çav",
        "êvar",
        "ser-hev",
        "'ez",
        "bi’çûk",
    ] {
        assert_eq!(default_pack_eligibility(ok), Ok(()), "{}", ok);
    }
    // alphabetic characters outside the alphabet
    for (word, bad) in [
        ("héraultê", vec!['é']),
        ("württemberg", vec!['ü']),
        ("côte", vec!['ô']),
        ("ḧeval", vec!['ḧ']),
        ("ẍort", vec!['ẍ']),
        ("ıspanak", vec!['ı']),
    ] {
        assert_eq!(default_pack_eligibility(word), Err(bad), "{}", word);
    }
    // digit-bearing and symbol-bearing forms
    assert_eq!(default_pack_eligibility("2012an"), Err(vec!['0', '1', '2']));
    assert_eq!(default_pack_eligibility("16ê"), Err(vec!['1', '6']));
    assert_eq!(default_pack_eligibility("km²"), Err(vec!['²']));
    assert_eq!(default_pack_eligibility("x!y"), Err(vec!['!']));
}

#[test]
fn casing_and_decomposition_normalize_onto_the_alphabet() {
    // Uppercase forms are allowed through normalization, never as letters themselves.
    assert!(!is_kurmanci_letter('Ş') && !is_kurmanci_letter('A'));
    assert_eq!(normalize_text("ROJBAŞ"), "rojbaş");
    assert_eq!(default_pack_eligibility(&normalize_text("ÊVAR")), Ok(()));
    assert_eq!(default_pack_eligibility(&normalize_text("PIRTÛK")), Ok(()));
    // Decomposed input recomposes to the approved letter.
    for (decomposed, letter) in [
        ("c\u{0327}", "ç"),
        ("e\u{0302}", "ê"),
        ("i\u{0302}", "î"),
        ("s\u{0327}", "ş"),
        ("u\u{0302}", "û"),
    ] {
        assert_eq!(normalize_text(decomposed), letter);
        assert_eq!(
            default_pack_eligibility(&normalize_text(decomposed)),
            Ok(())
        );
    }
    // Not every diacritic recomposes into a Kurmancî letter.
    assert_eq!(
        default_pack_eligibility(&normalize_text("e\u{0301}")),
        Err(vec!['é'])
    );
}

#[test]
fn technical_filter_keeps_out_of_alphabet_tokens_out_of_future_review() {
    for tok in ["2016an", "héraultê", "16ê", "km²", "comté"] {
        assert_eq!(classify_technical_noise(tok), "out_of_alphabet", "{}", tok);
    }
    for tok in ["wêne", "şablon", "kategorî", "ser-hev"] {
        assert_eq!(classify_technical_noise(tok), "none", "{}", tok);
    }
    assert_eq!(classify_technical_noise("2016"), "pure_numeric");
}

fn cand(
    normalized: &str,
    source_id: &str,
    population: EntryPopulation,
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

/// The authoritative invariant, source-independent: an approved (default-producing)
/// candidate outside the alphabet is a contradiction that fails resolution with a clear
/// message; it is never silently removed, reinterpreted or converted.
#[test]
fn approved_out_of_alphabet_candidate_fails_closed_for_every_source() {
    use EntryPopulation::*;
    for (source, form, expect) in [
        ("kuwiki-batch-001", "più", "'ù' (U+00F9)"),
        ("kurdish-hunspell-kmr", "ḧeval", "'ḧ' (U+1E27)"),
        ("future-source-xyz", "2012an", "'0' (U+0030)"),
    ] {
        let candidates = vec![
            cand("rojbaş", source, ExternalApproved, "approved"),
            cand(form, source, ExternalApproved, "approved"),
        ];
        for pack in ["reviewed", "experimental-full"] {
            let err = apply_default_pack_alphabet_policy(pack, &candidates)
                .expect_err("approved out-of-alphabet form must fail");
            assert!(
                err.contains("production lexical eligibility violation"),
                "{}",
                err
            );
            assert!(err.contains(&format!("token: {:?}", form)), "{}", err);
            assert!(err.contains(&format!("source: {}", source)), "{}", err);
            assert!(err.contains("decision: approved"), "{}", err);
            assert!(err.contains(expect), "{}", err);
        }
    }
    // metadata-change approvals and seed populations are default-producing too
    for pop in [
        ExternalApprovedMetadataChange,
        ManualSeed,
        SeedMetadataChange,
    ] {
        let c = vec![cand("héval", "any", pop, "approved_with_metadata_change")];
        assert!(apply_default_pack_alphabet_policy("experimental-full", &c).is_err());
    }
    // valid approved forms succeed, hyphen/apostrophe are not decided by this policy
    let ok = vec![
        cand("kurmancî", "kuwiki-batch-002", ExternalApproved, "approved"),
        cand(
            "ser-hev",
            "kurdish-hunspell-kmr",
            ExternalApproved,
            "approved",
        ),
        cand(
            "bi’çûk",
            "kurdish-hunspell-kmr",
            ExternalApproved,
            "approved",
        ),
    ];
    for pack in ["seed", "reviewed", "experimental-full"] {
        assert_eq!(
            apply_default_pack_alphabet_policy(pack, &ok),
            Ok(()),
            "{}",
            pack
        );
    }
}

/// Source evidence outside the alphabet is not an error: undecided or experimental-only
/// records may sit in the experimental-full reservoir; a rejected record is never a
/// candidate at all. Only the default packs and default-producing decisions are gated.
#[test]
fn out_of_alphabet_evidence_is_legitimate_outside_the_default_vocabulary() {
    use EntryPopulation::*;
    let evidence = vec![
        cand(
            "più",
            "kurdish-hunspell-kmr",
            ExternalUnreviewed,
            "unreviewed",
        ),
        cand(
            "km²",
            "kuwiki-batch-001",
            ExternalExperimentalOnly,
            "experimental_only",
        ),
        cand(
            "</H po:punctuation",
            "kurdish-hunspell-kmr",
            ExternalUnreviewed,
            "unreviewed",
        ),
    ];
    assert_eq!(
        apply_default_pack_alphabet_policy("experimental-full", &evidence),
        Ok(())
    );
    // the same records can never be default vocabulary
    assert!(apply_default_pack_alphabet_policy("reviewed", &evidence).is_err());
    let seed = vec![cand("héval", "manual-seed", ManualSeed, "seed")];
    let err = apply_default_pack_alphabet_policy("seed", &seed).unwrap_err();
    assert!(err.contains("héval") && err.contains("U+00E9"), "{}", err);
}

#[test]
fn seed_and_reviewed_packs_satisfy_the_policy_and_experimental_stays_a_reservoir() {
    for pack in ["seed", "reviewed"] {
        let entries = resolve_authoritative_pack_lexicon(pack, ws_root()).unwrap();
        let offenders: Vec<&str> = entries
            .iter()
            .filter(|e| !out_of_alphabet_chars(&e.normalized).is_empty())
            .map(|e| e.normalized.as_str())
            .collect();
        assert!(offenders.is_empty(), "{}: {:?}", pack, offenders);
        eprintln!(
            "{}: {} entries, all within the alphabet",
            pack,
            entries.len()
        );
    }
    // The corrected repository resolves: no authoritative decision contradicts the policy.
    let payload = resolve_authoritative_pack_payload("reviewed", ws_root()).unwrap();
    assert_eq!(
        payload.resolved_entries.len(),
        resolve_authoritative_pack_lexicon("reviewed", ws_root())
            .unwrap()
            .len()
    );

    // Experimental-full is an evidence reservoir: it still holds out-of-alphabet forms from
    // undecided source records; reviewed stays a subset of it.
    let experimental = resolve_authoritative_pack_lexicon("experimental-full", ws_root()).unwrap();
    let reservoir = experimental
        .iter()
        .filter(|e| !out_of_alphabet_chars(&e.normalized).is_empty())
        .count();
    assert!(
        reservoir > 0,
        "experimental-full must keep out-of-alphabet evidence"
    );
    let reviewed_entries = resolve_authoritative_pack_lexicon("reviewed", ws_root()).unwrap();
    let reviewed_set: BTreeSet<&str> = reviewed_entries
        .iter()
        .map(|e| e.normalized.as_str())
        .collect();
    let experimental_set: BTreeSet<&str> =
        experimental.iter().map(|e| e.normalized.as_str()).collect();
    assert!(
        reviewed_set.is_subset(&experimental_set),
        "reviewed must stay a subset of experimental-full"
    );
    eprintln!(
        "experimental-full keeps {} out-of-alphabet forms",
        reservoir
    );
}

/// The permanent CI guard: no decided Kuwiki candidate outside the alphabet may carry an
/// `approved` (or metadata-change) decision. A future approval of such a form fails here,
/// naming it, instead of being silently gated; it is then corrected in the decision
/// artifacts under the policy, never added to any keyboard specification.
#[test]
fn no_kuwiki_approval_is_outside_the_alphabet() {
    let report = audit_alphabet(ws_root()).unwrap();
    let mut approved_outside = Vec::new();
    for b in &report.kuwiki_batches {
        for c in &b.outside_candidates {
            if c.review_status.starts_with("approved") {
                approved_outside.push((b.batch_id.clone(), c.batch_rank, c.normalized.clone()));
            }
        }
    }
    assert!(
        approved_outside.is_empty(),
        "approved decisions outside the alphabet (correct them under the policy): {:?}",
        approved_outside
    );
}

#[test]
fn source_records_are_untouched_and_the_hunspell_import_keeps_its_evidence() {
    let import =
        fs::read_to_string(ws_root().join("data/imported/kurdish-hunspell-kmr/lexicon.jsonl"))
            .unwrap();
    let outside = import
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            !out_of_alphabet_chars(v["normalized"].as_str().unwrap_or("")).is_empty()
        })
        .count();
    assert!(
        outside > 0,
        "the import is a source record and keeps its out-of-alphabet forms"
    );
}

#[test]
fn audit_reports_provenance_and_changes_nothing() {
    let root = ws_root();
    let watched = [
        root.join("data/reviewed"),
        root.join("data/review-queues"),
        root.join("data/review-decisions"),
        root.join("data/review-batches"),
        root.join("data/review-queues"),
        root.join("data/imported"),
        root.join("data/language-model"),
        root.join("data/build/packs"),
    ];
    let before: Vec<_> = watched.iter().map(|p| snapshot(p)).collect();
    let report = write_alphabet_audit(root).unwrap();
    let after: Vec<_> = watched.iter().map(|p| snapshot(p)).collect();
    assert_eq!(
        before, after,
        "the audit must not modify any data or build file"
    );

    assert_eq!(report.schema_version, "alphabet-audit-v2");
    assert_eq!(
        report.alphabet,
        KURMANCI_ALPHABET.iter().collect::<String>()
    );
    assert!(
        report.invalid_approvals.is_empty(),
        "authoritative decisions admitting out-of-alphabet forms: {:?}",
        report.invalid_approvals
    );
    assert!(
        report.pack_resolution_errors.is_empty(),
        "{:?}",
        report.pack_resolution_errors
    );
    assert_eq!(report.packs["seed"].outside_forms, 0);
    assert_eq!(report.packs["reviewed"].outside_forms, 0);
    assert!(report.packs["experimental-full"].outside_forms > 0);
    let h = report
        .hunspell_queues
        .as_ref()
        .expect("hunspell queues audited");
    assert_eq!(
        h.review_pool_outside_forms, 0,
        "ordinary review pool must be clean"
    );
    assert!(h.policy_excluded_records > 0);
    assert_eq!(
        h.review_pool_word_internal_punctuation_forms, 0,
        "hyphen/apostrophe forms are held for linguist review since 2026-09-19, not in the pool"
    );
    assert!(h.punctuation_policy_held_records > 0);
    // Every Kuwiki batch is inventoried with the decision each outside candidate carries,
    // and the corrected records carry the policy rejection with its date.
    assert_eq!(report.kuwiki_batches.len(), 2);
    for b in &report.kuwiki_batches {
        assert!(!b.outside_candidates.is_empty());
        assert_eq!(
            b.by_review_status.values().sum::<usize>(),
            b.outside_candidates.len()
        );
        assert_eq!(b.by_review_status.get("approved"), None, "{}", b.batch_id);
    }
    let batch_001 = &report.kuwiki_batches[0];
    assert_eq!(batch_001.batch_id, "kuwiki-batch-001");
    let known = batch_001
        .outside_candidates
        .iter()
        .find(|c| c.normalized == "20an")
        .expect("20an is inventoried");
    assert_eq!(known.batch_rank, 223);
    assert_eq!(known.review_status, "rejected_from_default_pack");
    assert_eq!(known.review_date.as_deref(), Some("2026-09-17"));
    // Deterministic: a second run is identical.
    assert_eq!(audit_alphabet(root).unwrap(), report);
    // The report files exist and parse; the audit dir is a generated report location.
    let dir = root.join("data/reports/alphabet-audit");
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(json["schema_version"], "alphabet-audit-v2");
    assert!(fs::read_to_string(dir.join("report.md"))
        .unwrap()
        .contains("diagnostic only"));
}

/// The default-vocabulary validator and the pack resolver's gate are the same policy: for
/// spaces, foreign letters, digits, symbols and the undecided hyphen/apostrophe, both agree
/// with `default_pack_eligibility`; there is one implementation at the production boundary.
#[test]
fn validator_and_resolver_agree_with_the_single_eligibility_policy() {
    let entry = |normalized: &str| SourceLexiconEntry {
        word: normalized.to_string(),
        lemma: normalized.to_string(),
        normalized: normalized.to_string(),
        part_of_speech: "noun".to_string(),
        frequency: 0,
        status: "seed".to_string(),
        variants: vec![],
        sources: vec![],
        regions: vec![],
        frequency_metadata: None,
    };
    for form in [
        "rojbaş",
        "kurmancî",
        "ser-hev",
        "'ez",
        "bi’çûk",
        "ji bo",
        "héval",
        "ḧeval",
        "2012an",
        "km²",
        "x!y",
        "ıspanak",
    ] {
        let policy = default_pack_eligibility(form);
        let hold = data_builder_lib::alphabet::word_punctuation_hold(form);
        let validator = validate_entry(&entry(form), 1);
        let candidates = [cand(
            form,
            "any-source",
            EntryPopulation::ExternalApproved,
            "approved",
        )];
        let gate = apply_default_pack_alphabet_policy("reviewed", &candidates);
        let punctuation_gate =
            data_builder_lib::pack::selection::apply_default_pack_word_punctuation_policy(
                "reviewed",
                &candidates,
            );
        // The validator applies both policies (alphabet, word punctuation), exactly as the
        // resolver applies its two gates: one shared rule each, nothing re-implemented.
        assert_eq!(
            policy.is_ok() && hold.is_ok(),
            validator.is_ok(),
            "validator disagrees on {:?}: {:?}",
            form,
            validator
        );
        assert_eq!(policy.is_ok(), gate.is_ok(), "gate disagrees on {:?}", form);
        assert_eq!(
            hold.is_ok(),
            punctuation_gate.is_ok(),
            "punctuation gate disagrees on {:?}",
            form
        );
        if let Err(outside) = &policy {
            let described = data_builder_lib::alphabet::describe_out_of_alphabet(outside);
            assert!(
                validator.as_ref().unwrap_err().contains(&described),
                "{:?}",
                validator
            );
            assert!(gate.as_ref().unwrap_err().contains(&described));
        }
    }
    // A space is not eligible anywhere at the boundary (the earlier validator allowed it).
    assert_eq!(default_pack_eligibility("ji bo"), Err(vec![' ']));
    assert!(validate_entry(&entry("ji bo"), 1).is_err());
    // Structural validity stays separate from eligibility.
    assert!(validate_entry(&entry(""), 1).is_err());
    assert!(validate_entry(&entry("<b>"), 1).is_err());
}
