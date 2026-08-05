pub mod api;
pub mod distance;
pub mod engine;
pub mod errors;
pub mod format;
pub mod normalization;
pub mod ranking;
pub mod trie;

// Recommended high-level public integration API
pub use api::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, PackInfo, Prediction, PredictionOptions,
    SuggestOptions, SuggestionResult, DEFAULT_RESULT_LIMIT, MAX_RESULT_LIMIT,
};
pub use errors::{EngineError, PackLoadError};

// Existing low-level exports (maintained temporarily for backward compatibility)
pub use engine::{Engine, LexiconEntry};
pub use normalization::{normalize, strip_diacritics};
pub use ranking::{
    ContextPredictionResult, FrequencyMetadata, NextWordPrediction, PredictionSource,
    RankedCandidate, RankingConfig, Suggestion, SuggestionKind, UnknownContextPolicy,
};
