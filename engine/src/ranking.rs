use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SuggestionKind {
    Exact,
    Completion,
    Correction,
    DiacriticCorrection,
    NextWord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub text: String,
    pub score: f64,
    pub kind: SuggestionKind,
}

pub fn calculate_score(
    query: &str,
    candidate: &str,
    frequency: u64,
    max_frequency: u64,
    edit_distance: f64,
    kind: SuggestionKind,
) -> f64 {
    let max_freq_f = (max_frequency as f64).max(1.0);
    let freq_score = (frequency as f64 + 1.0).ln() / (max_freq_f + 1.0).ln();

    let edit_penalty = edit_distance * 0.35;

    let is_exact_diacritic_match = crate::normalization::strip_diacritics(candidate)
        == crate::normalization::strip_diacritics(query)
        && candidate.chars().count() == query.chars().count();

    let prefix_bonus =
        if (candidate.starts_with(query) || is_exact_diacritic_match) && query.len() >= 2 {
            0.25
        } else {
            0.0
        };

    let kind_bonus = match kind {
        SuggestionKind::Exact => 0.50,
        SuggestionKind::Completion => 0.30,
        SuggestionKind::DiacriticCorrection => 0.40,
        SuggestionKind::Correction => 0.10,
        SuggestionKind::NextWord => 0.20,
    };

    let raw_score = (freq_score * 0.35) + kind_bonus + prefix_bonus - edit_penalty;
    raw_score.clamp(0.01, 0.99)
}
