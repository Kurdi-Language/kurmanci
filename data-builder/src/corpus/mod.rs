//! Corpus module for Kurmancî text corpora management, tokenization, frequency building, and statistical reporting.

pub mod frequency;
pub mod importer;
pub mod join;
pub mod registry;
pub mod reports;
pub mod tokenizer;

pub use frequency::{build_corpus_frequencies, FrequencyBuildStats, FrequencyRecord};
pub use importer::{import_corpus, CorpusImportSummaryReport};
pub use join::{join_frequencies_to_lexicon, FrequencyJoinSummaryReport};
pub use registry::{CorpusFile, CorpusRegistry, CorpusRegistryEntry};
pub use tokenizer::tokenize_text;
