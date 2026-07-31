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

    // Flexible character set check (allowing letters, diacritics, apostrophes, hyphens, spaces)
    for ch in entry.normalized.chars() {
        let is_valid_char = ch.is_alphabetic() || ch == '\'' || ch == '’' || ch == '-' || ch == ' ';
        if !is_valid_char {
            return Err(format!(
                "Line {}: Invalid character '{}' (U+{:04X}) in word '{}'",
                line_num, ch, ch as u32, entry.word
            ));
        }
    }

    Ok(())
}
