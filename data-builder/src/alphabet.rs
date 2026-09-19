//! The Kurmancî Latin alphabet and the default-pack alphabet policy.
//!
//! Explicit human project policy (project owner, 2026-09-17): a lexical entry of the
//! default/reviewed pack consists only of the 31 letters of the Kurmancî alphabet. After
//! canonical normalization (`normalize_text`: NFC, lower case) the 31 letters are eligible;
//! alphabetic characters outside the set, digits, superscript digits and other symbols are
//! not, unless a separately approved lexical policy admits a character class. This is not a
//! linguistic inference by tooling: it is an explicit human policy that tooling enforces
//! deterministically, through the one function `default_pack_eligibility`, which nothing
//! else re-implements.
//!
//! Where the policy is enforced, in order:
//!
//! 1. Before ordinary human review, mechanically. `generate-review-queues` keeps every
//!    Hunspell import entry whose normalized form is ineligible out of the ordinary review
//!    pool and records it in `alphabet-policy-excluded.jsonl` with the reason
//!    (`OUT_OF_ALPHABET_REASON_CODE`, `EXCLUDED_BY_ALPHABET_POLICY_ACTION`); the corpus
//!    technical filter (`classify_technical_noise` → `OUT_OF_ALPHABET_REASON`) keeps such
//!    tokens out of Kuwiki and other corpus review batches. The raw import, the candidate
//!    batches and every review identity are preserved as source evidence.
//! 2. At the default-vocabulary boundary. `validate_entry` (manual seed lexicon and
//!    metadata-change replacements) rejects ineligible forms.
//! 3. At authoritative resolution, fail closed. `pack::selection::apply_default_pack_alphabet_policy`
//!    runs over the merged candidates of every source: an approved, metadata-change or seed
//!    candidate that is ineligible is a contradiction between an authoritative decision and
//!    the production policy and fails resolution naming form, source and decision. It is
//!    never silently removed, reinterpreted or converted; the decision artifact must be
//!    corrected explicitly.
//!
//! Historical correction: the 13 Kuwiki candidates approved before the policy although
//! ineligible were transparently set to `rejected_from_default_pack` under the project
//! owner's policy on 2026-09-17, each record naming the policy basis, the characters and the
//! previous decision (see `docs/lexicon-review.md`).
//!
//! What the policy does not touch: undecided and experimental-only evidence stays in the
//! experimental-full reservoir according to the existing pack policy (an out-of-alphabet
//! source form is not an error); source records are never modified.
//!
//! # Word-punctuation policy (project owner, 2026-09-19)
//!
//! Hyphen and apostrophes (`WORD_INTERNAL_PUNCTUATION`: `-`, U+0027, U+2019) are not
//! letters. By explicit human decision, a lexical form containing any of them is **not
//! approved into the reviewed/default pack until a linguist has reviewed it**; which
//! apostrophe code point is canonical stays undecided, so both are held alike. Mechanically:
//!
//! 1. Before ordinary review, `generate-review-queues` keeps such Hunspell entries out of the
//!    ordinary pool and writes them to `punctuation-policy-needs-linguist.jsonl`
//!    (`WORD_PUNCTUATION_REASON_CODE`, `HELD_FOR_LINGUIST_ACTION`), each record also naming
//!    the characters and, as `POSSIBLE_DUPLICATE_OF:<form>` reason codes, every other import
//!    or seed form that is identical once the punctuation is removed (a possible duplicate is
//!    flagged for a human, never merged).
//! 2. At authoritative resolution, fail closed: `pack::selection::apply_default_pack_word_punctuation_policy`
//!    refuses an approved, metadata-change or seed candidate whose form contains held
//!    punctuation, and refuses two default-vocabulary candidates that differ only by held
//!    punctuation (both cannot be approved).
//! 3. At the default-vocabulary boundary: `validate_entry` refuses such forms.
//!
//! Historical correction: the one Hunspell approval of such a form (`'azîm`, approved
//! 2026-08-24) was set to `needs_linguist` on 2026-09-19 with the previous decision preserved
//! in its note and evidence (see `docs/lexicon-review.md`). Nothing is decided linguistically
//! here; a linguist's decision on any held form is recorded through the ordinary review
//! artifacts.

/// The 31 letters, in alphabetical order.
pub const KURMANCI_ALPHABET: [char; 31] = [
    'a', 'b', 'c', 'ç', 'd', 'e', 'ê', 'f', 'g', 'h', 'i', 'î', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    'q', 'r', 's', 'ş', 't', 'u', 'û', 'v', 'w', 'x', 'y', 'z',
];

/// Non-letter characters held for linguist review by the word-punctuation policy
/// (2026-09-19): hyphen-minus, apostrophe U+0027 and right single quotation mark U+2019. The
/// alphabet policy neither accepts nor rejects them; the word-punctuation policy holds them.
pub const WORD_INTERNAL_PUNCTUATION: [char; 3] = ['-', '\'', '\u{2019}'];

/// Date of the word-punctuation policy decision (project owner).
pub const WORD_PUNCTUATION_POLICY_DATE: &str = "2026-09-19";
/// Reason code carried by review-queue records of an entry held by the word-punctuation policy.
pub const WORD_PUNCTUATION_REASON_CODE: &str = "WORD_PUNCTUATION";
/// `suggested_action` of such records: the entry waits for a linguist, it is not reviewed on
/// the ordinary desk.
pub const HELD_FOR_LINGUIST_ACTION: &str = "needs_linguist";
/// Queue file that carries the held entries.
pub const PUNCTUATION_HELD_QUEUE_FILE: &str = "punctuation-policy-needs-linguist.jsonl";
/// Prefix of the reason code that names a possible duplicate (a form identical once the held
/// punctuation is removed). Flagged for a human; never merged automatically.
pub const POSSIBLE_DUPLICATE_REASON_PREFIX: &str = "POSSIBLE_DUPLICATE_OF:";

/// Reason code carried by review-queue records of an entry the policy excludes from
/// ordinary review (`generate-review-queues`).
pub const OUT_OF_ALPHABET_REASON_CODE: &str = "OUT_OF_ALPHABET";
/// `suggested_action` of such records.
pub const EXCLUDED_BY_ALPHABET_POLICY_ACTION: &str = "excluded_by_alphabet_policy";
/// Technical-noise reason returned by the corpus filter for such tokens.
pub const OUT_OF_ALPHABET_REASON: &str = "out_of_alphabet";

/// True for the 31 lowercase letters. Input is expected to be normalized (lowercase, NFC);
/// uppercase letters are not letters of the normalized alphabet.
pub fn is_kurmanci_letter(c: char) -> bool {
    KURMANCI_ALPHABET.contains(&c)
}

/// The distinct characters of `normalized` that are neither a Kurmancî letter nor
/// word-internal punctuation, in code point order. Empty when the form passes the policy.
pub fn out_of_alphabet_chars(normalized: &str) -> Vec<char> {
    let mut out: Vec<char> = normalized
        .chars()
        .filter(|c| !is_kurmanci_letter(*c) && !WORD_INTERNAL_PUNCTUATION.contains(c))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Default-pack eligibility under the alphabet policy: `Err(chars)` names the offending
/// characters of an ineligible normalized form.
pub fn default_pack_eligibility(normalized: &str) -> Result<(), Vec<char>> {
    let outside = out_of_alphabet_chars(normalized);
    if outside.is_empty() {
        Ok(())
    } else {
        Err(outside)
    }
}

/// The distinct held punctuation characters of `normalized`, in code point order. Empty when
/// the word-punctuation policy does not hold the form.
pub fn word_punctuation_chars(normalized: &str) -> Vec<char> {
    let mut out: Vec<char> = normalized
        .chars()
        .filter(|c| WORD_INTERNAL_PUNCTUATION.contains(c))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Word-punctuation hold: `Err(chars)` names the held characters of a form that must not enter
/// the default vocabulary until a linguist has reviewed it.
pub fn word_punctuation_hold(normalized: &str) -> Result<(), Vec<char>> {
    let held = word_punctuation_chars(normalized);
    if held.is_empty() {
        Ok(())
    } else {
        Err(held)
    }
}

/// `normalized` with every held punctuation character removed: the key under which forms that
/// differ only by such punctuation are flagged as possible duplicates. A flag, not an identity:
/// review identity stays the exact normalized form.
pub fn punctuation_stripped_form(normalized: &str) -> String {
    normalized
        .chars()
        .filter(|c| !WORD_INTERNAL_PUNCTUATION.contains(c))
        .collect()
}

/// Human-readable description of the offending characters, e.g. `'é' (U+00E9), '2' (U+0032)`.
pub fn describe_out_of_alphabet(chars: &[char]) -> String {
    chars
        .iter()
        .map(|c| format!("'{}' (U+{:04X})", c, *c as u32))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphabet_is_31_distinct_lowercase_letters() {
        let mut sorted = KURMANCI_ALPHABET.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 31);
        assert!(KURMANCI_ALPHABET.iter().all(|c| c.is_lowercase()));
        for c in ['ç', 'ê', 'î', 'ş', 'û'] {
            assert!(is_kurmanci_letter(c));
        }
        for c in ['ḧ', 'ẍ', 'é', 'ü', 'ı', 'ğ', 'İ', 'A', '2', '²', '!', ' '] {
            assert!(!is_kurmanci_letter(c), "{}", c);
        }
    }

    #[test]
    fn word_punctuation_policy_holds_hyphen_and_both_apostrophes() {
        assert!(word_punctuation_hold("azîm").is_ok());
        assert_eq!(word_punctuation_hold("'azîm"), Err(vec!['\'']));
        assert_eq!(word_punctuation_hold("bin-av"), Err(vec!['-']));
        assert_eq!(
            word_punctuation_hold("be\u{2019}ecok"),
            Err(vec!['\u{2019}'])
        );
        assert_eq!(word_punctuation_hold("a-b'c"), Err(vec!['\'', '-']));
        assert_eq!(punctuation_stripped_form("'azîm"), "azîm");
        assert_eq!(punctuation_stripped_form("bin-av"), "binav");
        assert_eq!(punctuation_stripped_form("rojbaş"), "rojbaş");
        // The alphabet policy still neither accepts nor rejects these characters on its own.
        assert!(out_of_alphabet_chars("'azîm").is_empty());
        assert!(out_of_alphabet_chars("bin-av").is_empty());
    }

    #[test]
    fn policy_flags_only_characters_outside_the_alphabet() {
        assert!(out_of_alphabet_chars("rojbaş").is_empty());
        assert!(out_of_alphabet_chars("kurmancî").is_empty());
        assert!(out_of_alphabet_chars("ser-hev").is_empty());
        assert!(out_of_alphabet_chars("'ez").is_empty());
        assert_eq!(out_of_alphabet_chars("2012an"), vec!['0', '1', '2']);
        assert_eq!(out_of_alphabet_chars("km²"), vec!['²']);
        assert_eq!(out_of_alphabet_chars("héraultê"), vec!['é']);
        assert_eq!(out_of_alphabet_chars("württemberg"), vec!['ü']);
        assert_eq!(out_of_alphabet_chars("ḧeval"), vec!['ḧ']);
        assert_eq!(
            describe_out_of_alphabet(&out_of_alphabet_chars("2é")),
            "'2' (U+0032), 'é' (U+00E9)"
        );
    }
}
