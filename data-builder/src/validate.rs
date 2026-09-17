use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FrequencyMetadata {
    pub token_count: u64,
    pub document_count: u64,
    pub zipf_milli: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLexiconEntry {
    pub word: String,
    pub lemma: String,
    pub normalized: String,
    pub part_of_speech: String,
    pub frequency: u64,
    pub status: String,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub frequency_metadata: Option<FrequencyMetadata>,
}

/// Validates an entry at the default-vocabulary boundary: the manual seed lexicon
/// (`data/reviewed/lexicon.jsonl`, `build`) and the replacement metadata of an
/// `approved_with_metadata_change` decision (which enters the reviewed pack). Two concerns are
/// kept apart: the structural checks below say whether the record is technically valid
/// (fields present, length, no markup or URL fragment); the character rule is the one
/// production lexical eligibility policy, `crate::alphabet::default_pack_eligibility`, and
/// is not re-implemented here. A source record that is technically valid but ineligible
/// (a space, a digit, a foreign letter) is therefore rejected here exactly as the pack
/// resolver rejects it; hyphen and apostrophes are left to their own policy by both.
pub fn validate_entry(entry: &SourceLexiconEntry, line_num: usize) -> Result<(), String> {
    if entry.word.trim().is_empty() {
        return Err(format!("Line {}: 'word' field is empty", line_num));
    }
    if entry.normalized.trim().is_empty() {
        return Err(format!("Line {}: 'normalized' field is empty", line_num));
    }
    if entry.status.trim().is_empty() {
        return Err(format!("Line {}: 'status' field is empty", line_num));
    }

    // Length check
    let len = entry.normalized.chars().count();
    if !(1..=64).contains(&len) {
        return Err(format!(
            "Line {}: Invalid word length ({}) for '{}'",
            line_num, len, entry.word
        ));
    }

    // HTML / URL check
    if entry.word.contains('<')
        || entry.word.contains('>')
        || entry.word.starts_with("http://")
        || entry.word.starts_with("https://")
    {
        return Err(format!(
            "Line {}: Forbidden HTML/URL fragment in word '{}'",
            line_num, entry.word
        ));
    }

    // Default-vocabulary character eligibility: the single shared policy.
    if let Err(outside) = crate::alphabet::default_pack_eligibility(&entry.normalized) {
        return Err(format!(
            "Line {}: word '{}' is not eligible for the default vocabulary: {} outside the approved Kurmancî ku-Latn alphabet",
            line_num,
            entry.word,
            crate::alphabet::describe_out_of_alphabet(&outside)
        ));
    }

    Ok(())
}
