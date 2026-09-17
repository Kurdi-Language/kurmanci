//! The two ku-Latn contracts are machine-checkable: strict schemas, the alphabet identical to
//! the project's alphabet policy, distinct letters consistent with Unicode and recomposed by
//! the engine's normalization, casing pairs that round-trip through Unicode default casing
//! (which is what makes a wrong dotted-i casing visible), keyboard requirements that demand
//! exactly the alphabet and prescribe no access mechanism, key-adjacency evidence keyed by
//! the alphabet, and default vocabularies that use only alphabet letters (plus the
//! word-internal punctuation left to its own policy). Nothing here decides anything
//! linguistic; the contracts carry `review_status`.

use data_builder_lib::alphabet::{KURMANCI_ALPHABET, WORD_INTERNAL_PUNCTUATION};
use data_builder_lib::pack::builder::{assemble_pack, resolve_authoritative_pack_lexicon};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn ws_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageName {
    native: String,
    english: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DistinctLetter {
    letter: String,
    base: String,
    uppercase: String,
    code_point: String,
    decomposition: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Casing {
    rule: String,
    pairs: Vec<[String; 2]>,
    notes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Normalization {
    reference: String,
    decomposed_input_accepted: bool,
    notes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Orthography {
    schema_version: String,
    locale_tag: String,
    language_name: LanguageName,
    review_status: String,
    reviewed_by: Option<String>,
    review_date: Option<String>,
    review_notes: Vec<String>,
    alphabet: Vec<String>,
    alphabet_basis: String,
    distinct_letters: Vec<DistinctLetter>,
    distinct_letters_note: String,
    casing: Casing,
    normalization: Normalization,
    not_covered_by_this_contract: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Requirement {
    id: String,
    statement: String,
    #[serde(default)]
    letters: Option<Vec<String>>,
    #[serde(default)]
    access_mechanism: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutDecision {
    statement: String,
    kind: String,
    decided_by: String,
    date: String,
    basis: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedImplementation {
    vendor: String,
    evidence_level: String,
    letter_access: String,
    note: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutArrangement {
    status: String,
    statement: String,
    decision: LayoutDecision,
    observed_implementations: Vec<ObservedImplementation>,
    observed_implementations_note: String,
    engine_evidence: String,
    existing_platform_keyboards: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Requirements {
    schema_version: String,
    locale_tag: String,
    orthography_contract: String,
    review_status: String,
    reviewed_by: Option<String>,
    review_date: Option<String>,
    review_notes: Vec<String>,
    requirements: Vec<Requirement>,
    layout_arrangement: LayoutArrangement,
    open_questions_for_human_review: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Adjacency {
    layout_name: String,
    adjacencies: BTreeMap<String, Vec<String>>,
}

fn load<T: serde::de::DeserializeOwned>(rel: &str) -> T {
    let path = ws_root().join(rel);
    serde_json::from_slice(&std::fs::read(&path).unwrap())
        .unwrap_or_else(|e| panic!("{} must match the strict schema: {}", rel, e))
}

fn single_char(s: &str) -> char {
    let mut it = s.chars();
    let c = it.next().expect("empty string is not a character");
    assert!(it.next().is_none(), "{:?} is not a single character", s);
    c
}

fn code_point(c: char) -> String {
    format!("U+{:04X}", c as u32)
}

fn assert_review_metadata(
    status: &str,
    reviewed_by: &Option<String>,
    review_date: &Option<String>,
    notes: &[String],
) {
    assert!(!notes.is_empty());
    match status {
        "draft-pending-human-review" => {
            assert!(
                reviewed_by.is_none() && review_date.is_none(),
                "a draft carries no reviewer attribution"
            );
        }
        "human-reviewed" => {
            assert!(reviewed_by.is_some() && review_date.is_some());
        }
        other => panic!("unknown review_status {:?}", other),
    }
}

#[test]
fn orthography_contract_is_the_alphabet_policy() {
    let o: Orthography = load("data/keyboard/ku-Latn-orthography.json");
    assert_eq!(o.schema_version, "ku-latn-orthography-v1");
    assert_eq!(o.locale_tag, "ku-Latn");
    assert_eq!(o.language_name.native, "Kurmancî");
    assert!(!o.language_name.english.is_empty() && !o.alphabet_basis.is_empty());
    assert!(!o.distinct_letters_note.is_empty() && !o.not_covered_by_this_contract.is_empty());
    assert_review_metadata(
        &o.review_status,
        &o.reviewed_by,
        &o.review_date,
        &o.review_notes,
    );

    // Alphabet: exactly the project's 31-letter policy, in policy order.
    let alphabet: Vec<char> = o.alphabet.iter().map(|s| single_char(s)).collect();
    assert_eq!(alphabet, KURMANCI_ALPHABET.to_vec());
    let alphabet_set: BTreeSet<char> = alphabet.iter().copied().collect();
    assert_eq!(alphabet_set.len(), 31);
    for c in &alphabet {
        assert!(c.is_lowercase() && c.is_alphabetic(), "{:?}", c);
    }

    // Distinct letters: the five, consistent with Unicode, recomposed by the engine.
    assert_eq!(o.distinct_letters.len(), 5);
    let mut seen = BTreeSet::new();
    for d in &o.distinct_letters {
        let letter = single_char(&d.letter);
        let base = single_char(&d.base);
        assert!(seen.insert(letter));
        assert!(alphabet_set.contains(&letter) && alphabet_set.contains(&base));
        assert_ne!(letter, base);
        assert_eq!(letter.to_uppercase().to_string(), d.uppercase);
        assert_eq!(code_point(letter), d.code_point);
        let parts: Vec<&str> = d.decomposition.split(' ').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(single_char(parts[0]), base);
        let mark = u32::from_str_radix(parts[1].trim_start_matches("U+"), 16).unwrap();
        let decomposed = format!("{}{}", base, char::from_u32(mark).unwrap());
        assert_eq!(kurmanci_engine::normalize(&decomposed), d.letter);
        assert_eq!(kurmanci_engine::normalize(&d.uppercase), d.letter);
    }
    assert_eq!(
        seen,
        ['ç', 'ê', 'î', 'ş', 'û']
            .into_iter()
            .collect::<BTreeSet<_>>()
    );

    // Casing: Unicode default, covering the alphabet exactly, round-tripping both ways.
    assert_eq!(o.casing.rule, "unicode-default");
    assert!(!o.casing.notes.is_empty());
    let lower: Vec<char> = o.casing.pairs.iter().map(|p| single_char(&p[0])).collect();
    assert_eq!(
        lower, alphabet,
        "casing pairs must list the alphabet in order"
    );
    for [l, u] in &o.casing.pairs {
        assert_eq!(&l.to_uppercase(), u, "{}", l);
        assert_eq!(&u.to_lowercase(), l, "{}", u);
        assert_eq!(kurmanci_engine::normalize(u), *l);
        assert_eq!(single_char(u).to_lowercase().to_string(), *l);
    }
    assert_eq!("i".to_uppercase(), "I");
    assert_eq!("I".to_lowercase(), "i");

    // Normalization contract names the engine rule and accepts decomposed input.
    assert!(o.normalization.decomposed_input_accepted);
    assert!(o.normalization.reference.contains("NFC"));
    assert!(!o.normalization.notes.is_empty());
    assert_eq!(kurmanci_engine::normalize("\u{FEFF}Şe\u{200B}v"), "şev");

    // The contract admits nothing outside the alphabet: every letter-like field is in it.
    for d in &o.distinct_letters {
        assert!(alphabet_set.contains(&single_char(&d.letter)));
    }
}

#[test]
fn keyboard_requirements_demand_exactly_the_alphabet_and_prescribe_no_mechanism() {
    let r: Requirements = load("data/keyboard/ku-Latn-keyboard-requirements.json");
    assert_eq!(r.schema_version, "ku-latn-keyboard-requirements-v1");
    assert_eq!(r.locale_tag, "ku-Latn");
    assert_eq!(
        r.orthography_contract,
        "data/keyboard/ku-Latn-orthography.json"
    );
    assert!(ws_root().join(&r.orthography_contract).is_file());
    assert_review_metadata(
        &r.review_status,
        &r.reviewed_by,
        &r.review_date,
        &r.review_notes,
    );
    assert!(!r.open_questions_for_human_review.is_empty());

    let ids: BTreeSet<&str> = r.requirements.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(
        ids.len(),
        r.requirements.len(),
        "requirement ids must be unique"
    );
    for id in [
        "letters-typeable",
        "access-mechanism-vendor-choice",
        "distinct-letter-identity",
        "casing",
        "encoding",
        "locale-tag",
        "non-lexical-layers",
    ] {
        assert!(ids.contains(id), "missing requirement {}", id);
    }
    for q in &r.requirements {
        assert!(!q.statement.is_empty(), "{}", q.id);
    }

    // The typeable set is exactly the 31 letters: no extra character is a keyboard requirement.
    let typeable = r
        .requirements
        .iter()
        .find(|q| q.id == "letters-typeable")
        .unwrap();
    let letters: Vec<char> = typeable
        .letters
        .as_ref()
        .expect("letters-typeable lists the letters")
        .iter()
        .map(|s| single_char(s))
        .collect();
    assert_eq!(letters, KURMANCI_ALPHABET.to_vec());
    for q in &r.requirements {
        if q.id != "letters-typeable" {
            assert!(q.letters.is_none(), "{} must not list letters", q.id);
        }
    }

    // The access mechanism is normatively the vendor's choice, and only that requirement
    // carries an access_mechanism value at all.
    let access = r
        .requirements
        .iter()
        .find(|q| q.id == "access-mechanism-vendor-choice")
        .unwrap();
    assert_eq!(access.access_mechanism.as_deref(), Some("vendor-choice"));
    for q in &r.requirements {
        if q.id != "access-mechanism-vendor-choice" {
            assert!(
                q.access_mechanism.is_none(),
                "{} must not carry an access mechanism",
                q.id
            );
        }
    }

    // The arrangement is intentionally unspecified by a dated project decision; vendor
    // observations are evidence with an explicit level, never requirements; the adjacency
    // file is evidence only.
    let la = &r.layout_arrangement;
    assert_eq!(la.status, "intentionally-unspecified");
    let st = la.statement.to_lowercase();
    assert!(
        st.contains("does not prescribe") && st.contains("universal"),
        "{}",
        la.statement
    );
    assert!(la
        .decision
        .statement
        .to_lowercase()
        .contains("no universal physical layout"));
    assert!(
        la.decision.kind.contains("project decision")
            && la.decision.kind.contains("not a linguistic")
    );
    assert_eq!(la.decision.decided_by, "project owner");
    assert!(la.decision.date.len() == 10 && la.decision.date.starts_with("2026-"));
    assert!(
        ws_root().join(&la.decision.basis).is_file(),
        "{}",
        la.decision.basis
    );
    let vendors: Vec<&str> = la
        .observed_implementations
        .iter()
        .map(|o| o.vendor.as_str())
        .collect();
    for v in ["Apple", "Google", "Samsung"] {
        assert!(
            vendors.iter().any(|x| x.contains(v)),
            "missing vendor evidence {}",
            v
        );
    }
    // Observations are evidence: each carries a level and a non-empty observed mechanism.
    // The set of mechanisms a vendor may use is open (vendor-choice), so no universe of
    // allowed values is asserted here; only the current records' observed values are.
    for o in &la.observed_implementations {
        assert!(
            !o.evidence_level.is_empty() && !o.note.is_empty() && !o.letter_access.is_empty(),
            "{}",
            o.vendor
        );
    }
    let observed = |vendor: &str| -> &str {
        la.observed_implementations
            .iter()
            .find(|o| o.vendor.contains(vendor))
            .map(|o| o.letter_access.as_str())
            .unwrap()
    };
    assert_eq!(observed("Apple"), "dedicated-keys");
    assert_eq!(observed("Google"), "dedicated-keys");
    assert_eq!(observed("Samsung"), "long-press");
    assert!(la.observed_implementations.iter().any(|o| o
        .evidence_level
        .to_lowercase()
        .contains("preliminary")
        && o.vendor.contains("Apple")));
    assert!(la
        .observed_implementations_note
        .to_lowercase()
        .contains("not requirements"));
    assert!(la.engine_evidence.contains("layout_ku.json"));
    assert!(la.engine_evidence.contains("not a layout authority"));
    assert!(la
        .engine_evidence
        .to_lowercase()
        .contains("separate future engine task"));
    assert!(!la.existing_platform_keyboards.is_empty());
    // The layout question is settled: the open questions state that no universal physical
    // arrangement is prescribed, and none asks to select a reference or universal layout.
    assert!(r.open_questions_for_human_review.iter().any(|q| q
        .to_lowercase()
        .contains("no universal physical arrangement is prescribed")));
    for q in &r.open_questions_for_human_review {
        let ql = q.to_lowercase();
        let asks_to_select = (ql.contains("select")
            || ql.contains("choose")
            || ql.contains("adopt")
            || ql.contains("record"))
            && (ql.contains("reference layout")
                || ql.contains("universal layout")
                || ql.contains("reference arrangement"));
        assert!(
            !asks_to_select,
            "open question asks to select a layout: {}",
            q
        );
    }
}

#[test]
fn key_adjacency_evidence_is_keyed_by_the_alphabet() {
    let adjacency: Adjacency = load("data/keyboard/layout_ku.json");
    assert_eq!(adjacency.layout_name, "ku-Latn-QWERTY");
    let alphabet: BTreeSet<char> = KURMANCI_ALPHABET.iter().copied().collect();
    let keys: BTreeSet<char> = adjacency
        .adjacencies
        .keys()
        .map(|k| single_char(k))
        .collect();
    assert_eq!(
        keys, alphabet,
        "adjacency keys must be exactly the alphabet"
    );
    for (k, neighbours) in &adjacency.adjacencies {
        for n in neighbours {
            assert!(
                alphabet.contains(&single_char(n)),
                "{} lists non-letter neighbour {}",
                k,
                n
            );
            assert!(
                adjacency.adjacencies[n].contains(k),
                "adjacency is not symmetric: {} -> {} but not back",
                k,
                n
            );
        }
    }
    // Each distinct letter is adjacent to its base and the base back to it (typo evidence).
    let o: Orthography = load("data/keyboard/ku-Latn-orthography.json");
    for d in &o.distinct_letters {
        assert!(adjacency.adjacencies[&d.letter].contains(&d.base));
        assert!(adjacency.adjacencies[&d.base].contains(&d.letter));
    }
}

/// The default vocabularies use only the alphabet (the word-internal punctuation left to
/// its own policy excepted), so the keyboard contract needs nothing beyond the 31 letters.
/// The experimental vocabulary is an evidence reservoir and is inventoried only.
#[test]
fn default_vocabularies_need_only_the_alphabet() {
    let mut allowed: BTreeSet<char> = KURMANCI_ALPHABET.iter().copied().collect();
    allowed.extend(WORD_INTERNAL_PUNCTUATION.iter().copied());
    for pack in ["seed", "reviewed"] {
        let entries = resolve_authoritative_pack_lexicon(pack, ws_root()).unwrap();
        let mut outside: BTreeMap<char, Vec<String>> = BTreeMap::new();
        for e in &entries {
            for c in e.normalized.chars() {
                if !allowed.contains(&c) {
                    let list = outside.entry(c).or_default();
                    if list.len() < 5 {
                        list.push(e.normalized.clone());
                    }
                }
            }
        }
        assert!(
            outside.is_empty(),
            "{} vocabulary needs characters outside the alphabet: {:?}",
            pack,
            outside
        );
        eprintln!(
            "{}: {} entries, all within the 31 letters",
            pack,
            entries.len()
        );
    }
    let entries = resolve_authoritative_pack_lexicon("experimental-full", ws_root()).unwrap();
    let inventory = entries
        .iter()
        .filter(|e| e.normalized.chars().any(|c| !allowed.contains(&c)))
        .count();
    eprintln!(
        "experimental-full: {} entries; {} forms with characters outside the alphabet (evidence reservoir, informational; see data-builder audit-alphabet)",
        entries.len(),
        inventory
    );
}

/// The distinct-letter identity requirement must not forbid what the product does: identity
/// and normalization never map a base letter to a distinct letter, while spell correction
/// and diacritic restoration may propose one as a linguistic correction. Proven against the
/// engine on the seed pack assembled in memory and a human-reviewed benchmark case (no new
/// linguistic decision is hard-coded here).
#[test]
fn distinct_letter_identity_does_not_forbid_diacritic_correction() {
    let r: Requirements = load("data/keyboard/ku-Latn-keyboard-requirements.json");
    let identity = r
        .requirements
        .iter()
        .find(|q| q.id == "distinct-letter-identity")
        .expect("distinct-letter-identity requirement");
    let text = identity.statement.to_lowercase();
    assert!(text.contains("distinct"), "{}", identity.statement);
    assert!(
        text.contains("spell correction") && text.contains("linguistic correction"),
        "the requirement must leave room for spelling correction: {}",
        identity.statement
    );
    assert!(
        !text.contains("never silently substitute") && !text.contains("must never silently"),
        "the requirement must not forbid diacritic correction: {}",
        identity.statement
    );

    // Identity/normalization: a base letter never becomes a distinct letter, or the reverse.
    for (base, distinct) in [("c", "ç"), ("e", "ê"), ("i", "î"), ("s", "ş"), ("u", "û")] {
        assert_eq!(kurmanci_engine::normalize(base), base);
        assert_eq!(kurmanci_engine::normalize(distinct), distinct);
        assert_ne!(
            kurmanci_engine::normalize(base),
            kurmanci_engine::normalize(distinct)
        );
    }
    assert_eq!(kurmanci_engine::normalize("biji"), "biji");

    // Correction: an existing human-reviewed missing-diacritics benchmark case whose expected
    // word is in the seed vocabulary is still returned by the correction API.
    let seed = assemble_pack("seed", ws_root()).unwrap();
    let engine = kurmanci_engine::KurmanciEngine::from_pack_bytes(&seed.binary_bytes).unwrap();
    let seed_words: BTreeSet<String> = seed
        .payload
        .resolved_entries
        .iter()
        .map(|e| e.normalized.clone())
        .collect();
    let cases = std::fs::read_to_string(ws_root().join("evaluation/spelling/reviewed-cases.jsonl"))
        .unwrap();
    let mut checked = 0;
    for line in cases.lines().filter(|l| !l.trim().is_empty()) {
        let case: serde_json::Value = serde_json::from_str(line).unwrap();
        if case["category"] != "missing-diacritics" || case["review_status"] != "human-reviewed" {
            continue;
        }
        let input = case["input"].as_str().unwrap();
        let expected: Vec<&str> = case["expectation"]["expected_candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        if !expected.iter().any(|w| seed_words.contains(*w)) {
            continue;
        }
        assert!(
            !engine.is_known_word(input),
            "{} is not identical to its corrected form",
            input
        );
        let results = engine.correct(input, kurmanci_engine::CorrectionOptions::default());
        let hit = results
            .iter()
            .find(|s| expected.contains(&s.text.as_str()))
            .unwrap_or_else(|| panic!("correction for {} must still offer {:?}", input, expected));
        assert_eq!(
            hit.kind,
            kurmanci_engine::SuggestionKind::DiacriticCorrection
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "at least one reviewed missing-diacritics case must be checkable on the seed pack"
    );
    eprintln!(
        "{} human-reviewed diacritic-correction cases still served by the correction API",
        checked
    );
}

/// The inspection evidence document exists and covers the three vendors with explicit
/// evidence levels and the observed access mechanisms, while the contracts it supports keep
/// exactly the 31-letter alphabet and the unspecified arrangement.
#[test]
fn platform_inspection_evidence_document_is_present_and_bounded() {
    let r: Requirements = load("data/keyboard/ku-Latn-keyboard-requirements.json");
    let path = ws_root().join(&r.layout_arrangement.decision.basis);
    let doc = std::fs::read_to_string(&path).unwrap();
    for needle in [
        "Apple",
        "Gboard",
        "Samsung",
        "Evidence level",
        "dedicated keys",
        "long press",
        "intentionally-unspecified",
        "layout_ku.json",
    ] {
        assert!(doc.contains(needle), "evidence document lacks {:?}", needle);
    }
    assert!(doc.contains("ç ê î ş û"));
    // The evidence may truthfully quote vendor labels and observed characters; what it must
    // not do is change the contracts. The real invariants live in the contract files:
    let o: Orthography = load("data/keyboard/ku-Latn-orthography.json");
    let alphabet: Vec<char> = o.alphabet.iter().map(|s| single_char(s)).collect();
    assert_eq!(alphabet, KURMANCI_ALPHABET.to_vec());
    let typeable: Vec<char> = r
        .requirements
        .iter()
        .find(|q| q.id == "letters-typeable")
        .and_then(|q| q.letters.as_ref())
        .unwrap()
        .iter()
        .map(|s| single_char(s))
        .collect();
    assert_eq!(typeable, KURMANCI_ALPHABET.to_vec());
    assert_eq!(r.layout_arrangement.status, "intentionally-unspecified");
    assert!(doc.contains("intentionally-unspecified"));
}
