pub mod audit;
pub mod compile;
pub mod config;
pub mod corpus;
pub mod eval;
pub mod eval_next_word;
pub mod eval_ranking;
pub mod importers;
pub mod merge;
pub mod normalize;
pub mod report;
pub mod review;
pub mod sources;
pub mod validate;

pub use audit::run_quality_audit;
pub use compile::{
    calculate_sha256, compile_binary_pack, compile_binary_pack_with_root, write_artifacts,
    ReleaseManifest, MAGIC_BYTES, PACK_VERSION,
};
pub use config::BuilderConfig;
pub use corpus::{
    audit_corpora, build_corpus_bigrams, build_corpus_frequencies, build_corpus_ngrams,
    build_corpus_trigrams, generate_corpus_inventory, import_all_corpora, import_corpus,
    join_frequencies_to_lexicon, partition_corpora, split_into_sentences, tokenize_text,
    validate_registry_relative_path, BigramBuildStats, BigramRecord, BigramSummaryReport,
    CanonicalDocumentRecord, CorpusAuditSummary, CorpusImportSummaryReport, CorpusInventorySummary,
    CorpusRegistry, CorpusRegistryEntry, FrequencyBuildStats, FrequencyJoinSummaryReport,
    FrequencyRecord, NgramBuildStats, NgramConfig, PartitionSummary, TrigramBuildStats,
    TrigramRecord, TrigramSummaryReport,
};
pub use eval::{evaluate_lexicon_impact, BenchmarkItem, EvaluationReport, LexiconMetrics};
pub use eval_next_word::{run_next_word_evaluation, NextWordEvalSummaryReport};
pub use eval_ranking::{run_ranking_evaluation, RankingEvalSummaryReport};
pub use importers::{
    import_hunspell_dic, parse_hunspell_line, parse_hunspell_source, HunspellSourceEvent,
    ImportSummaryReport,
};
pub use merge::merge_and_deduplicate;
pub use normalize::normalize_text;
pub use report::{generate_and_save_report, BuildReport};
pub use review::{
    compute_conflict_group_id, compute_entry_id, generate_review_queues, validate_review_decisions,
    ReviewDecisionRecord, ReviewDecisionStatus, ReviewMergerSummary, ReviewQueueSummary,
    ReviewTargetType, REVIEW_DECISION_SCHEMA_VERSION,
};
pub use sources::{SourceRegistry, SourceRegistryEntry};
pub use validate::{validate_entry, FrequencyMetadata, SourceLexiconEntry};
