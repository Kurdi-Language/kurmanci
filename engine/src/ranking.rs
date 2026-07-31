//! Engine Suggestion Ranking Policy & Candidate Comparison.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SuggestionKind {
    Exact,
    Completion,
    Correction,
    DiacriticCorrection,
    NextWord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FrequencyMetadata {
    pub token_count: u64,
    pub document_count: u64,
    pub zipf_milli: u32,
}

#[derive(Debug, Clone)]
pub struct RankingConfig {
    pub use_frequency: bool,
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            use_frequency: true,
        }
    }
}

impl RankingConfig {
    pub fn disabled() -> Self {
        Self {
            use_frequency: false,
        }
    }
}

fn kind_priority(kind: &SuggestionKind) -> u8 {
    match kind {
        SuggestionKind::Exact => 0,
        SuggestionKind::DiacriticCorrection => 1,
        SuggestionKind::Completion => 2,
        SuggestionKind::Correction => 3,
        SuggestionKind::NextWord => 4,
    }
}

#[derive(Debug, Clone)]
pub struct RankedCandidate {
    pub word: String,
    pub edit_cost: u32,
    pub is_diacritic_match: bool,
    pub prefix_quality: u32,
    pub frequency: FrequencyMetadata,
    pub kind: SuggestionKind,
}

impl RankedCandidate {
    /// Compares two candidate suggestions deterministically according to ranking policy.
    pub fn cmp_with_config(&self, other: &Self, config: &RankingConfig) -> Ordering {
        // Guarantee Exact matches rank before all non-exact candidates
        match (
            self.kind == SuggestionKind::Exact,
            other.kind == SuggestionKind::Exact,
        ) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }

        let self_prio = kind_priority(&self.kind);
        let other_prio = kind_priority(&other.kind);

        if self_prio != other_prio {
            return self_prio.cmp(&other_prio);
        }

        if self.kind == SuggestionKind::Completion {
            // Prefix completions
            other
                .prefix_quality
                .cmp(&self.prefix_quality)
                .then_with(|| {
                    if config.use_frequency {
                        other.frequency.zipf_milli.cmp(&self.frequency.zipf_milli)
                    } else {
                        Ordering::Equal
                    }
                })
                .then_with(|| {
                    if config.use_frequency {
                        other
                            .frequency
                            .document_count
                            .cmp(&self.frequency.document_count)
                    } else {
                        Ordering::Equal
                    }
                })
                .then_with(|| self.word.chars().count().cmp(&other.word.chars().count()))
                .then_with(|| self.word.cmp(&other.word))
        } else {
            // Spelling corrections
            self.edit_cost
                .cmp(&other.edit_cost)
                .then_with(|| other.is_diacritic_match.cmp(&self.is_diacritic_match))
                .then_with(|| {
                    if config.use_frequency {
                        other.frequency.zipf_milli.cmp(&self.frequency.zipf_milli)
                    } else {
                        Ordering::Equal
                    }
                })
                .then_with(|| {
                    if config.use_frequency {
                        other
                            .frequency
                            .document_count
                            .cmp(&self.frequency.document_count)
                    } else {
                        Ordering::Equal
                    }
                })
                .then_with(|| self.word.cmp(&other.word))
        }
    }

    /// Formats diagnostic explanation string for CLI --explain mode.
    pub fn ranking_reason(&self, config: &RankingConfig) -> String {
        if self.kind == SuggestionKind::Completion {
            if config.use_frequency && self.frequency.zipf_milli > 0 {
                format!(
                    "prefix match quality {}, then higher frequency (zipf_milli: {})",
                    self.prefix_quality, self.frequency.zipf_milli
                )
            } else {
                format!(
                    "prefix match quality {}, then lexical order",
                    self.prefix_quality
                )
            }
        } else if config.use_frequency && self.frequency.zipf_milli > 0 {
            format!(
                "lower edit cost (cost: {}), then higher frequency (zipf_milli: {})",
                self.edit_cost, self.frequency.zipf_milli
            )
        } else {
            format!(
                "lower edit cost (cost: {}), then lexical order",
                self.edit_cost
            )
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub text: String,
    pub score: f64,
    pub kind: SuggestionKind,
    #[serde(default)]
    pub edit_cost: u32,
    #[serde(default)]
    pub zipf_milli: u32,
    #[serde(default)]
    pub document_count: u64,
    #[serde(default)]
    pub ranking_reason: String,
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
