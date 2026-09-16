pub mod api;
pub mod compat;
pub mod distance;
pub mod engine;
pub mod errors;
pub mod format;
pub(crate) mod index;
pub mod memory;
pub mod normalization;
pub mod ranking;
pub mod trie;

#[cfg(test)]
mod query_equivalence_tests;

// Recommended high-level public integration API
pub use api::{
    CompletionOptions, CorrectionOptions, KurmanciEngine, PackInfo, Prediction, PredictionOptions,
    SuggestOptions, SuggestionResult, DEFAULT_RESULT_LIMIT, MAX_RESULT_LIMIT,
};
pub use compat::{
    probe_pack_header, CompatibilityTable, LoadFailureClass, PackHeader, ENGINE_VERSION,
    LANGUAGE_MODEL_SCHEMA_VERSION, PACK_SCHEMA_VERSION, SUPPORTED_LANGUAGE_TAG,
    SUPPORTED_PACK_SCHEMA_VERSIONS,
};
pub use errors::{EngineError, PackLoadError};
pub use memory::{MemoryAttribution, StructureMemory};

// Existing low-level exports (maintained temporarily for backward compatibility)
pub use engine::{Engine, LexiconEntry};
pub use normalization::{normalize, strip_diacritics};
pub use ranking::{
    ContextPredictionResult, FrequencyMetadata, NextWordPrediction, PredictionSource,
    RankedCandidate, RankingConfig, Suggestion, SuggestionKind, UnknownContextPolicy,
};
