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
//! source form is not an error); source records are never modified. Hyphen and apostrophes
//! (`WORD_INTERNAL_PUNCTUATION`) are not letters; whether they may occur inside a word is a
//! separate, still open human decision, so the policy neither accepts nor rejects a form on
//! their account.

/// The 31 letters, in alphabetical order.
pub const KURMANCI_ALPHABET: [char; 31] = [
    'a', 'b', 'c', 'ç', 'd', 'e', 'ê', 'f', 'g', 'h', 'i', 'î', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    'q', 'r', 's', 'ş', 't', 'u', 'û', 'v', 'w', 'x', 'y', 'z',
];

/// Non-letter characters whose place inside a word is still under human review; the alphabet
/// policy neither accepts nor rejects them.
pub const WORD_INTERNAL_PUNCTUATION: [char; 3] = ['-', '\'', '\u{2019}'];

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
