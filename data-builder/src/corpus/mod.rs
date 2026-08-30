//! Corpus module for Kurmancî text corpora management, tokenization, frequency building, and statistical reporting.

pub mod audit;
pub mod extractors;
pub mod frequency;
pub mod importer;
pub mod inventory;
pub mod join;
pub mod ngrams;
pub mod partition;
pub mod registry;
pub mod reports;
pub mod tokenizer;

pub use audit::{audit_corpora, CorpusAuditSummary};
pub use frequency::{
    build_corpus_frequencies, build_corpus_train_frequencies, FrequencyBuildStats, FrequencyRecord,
};
pub use importer::{
    import_all_corpora, import_corpus, CanonicalDocumentRecord, CorpusImportSummaryReport,
};
pub use inventory::{generate_corpus_inventory, CorpusInventorySummary};
pub use join::{join_frequencies_to_lexicon, FrequencyJoinSummaryReport};
pub use ngrams::{
    build_corpus_bigrams, build_corpus_ngrams, build_corpus_trigrams, split_into_sentences,
    BigramBuildStats, BigramRecord, BigramSummaryReport, NgramBuildStats, NgramConfig,
    TrigramBuildStats, TrigramRecord, TrigramSummaryReport,
};
pub use partition::{partition_corpora, PartitionSummary};
pub use registry::{
    validate_registry_relative_path, CorpusFile, CorpusRegistry, CorpusRegistryEntry,
};
pub use tokenizer::tokenize_text;
