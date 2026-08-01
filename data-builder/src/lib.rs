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
pub mod sources;
pub mod validate;

pub use audit::run_quality_audit;
pub use compile::{
    calculate_sha256, compile_binary_pack, compile_binary_pack_with_root, write_artifacts,
    ReleaseManifest, MAGIC_BYTES, PACK_VERSION,
};
pub use config::BuilderConfig;
pub use corpus::{
    build_corpus_bigrams, build_corpus_frequencies, build_corpus_ngrams, build_corpus_trigrams,
    import_corpus, join_frequencies_to_lexicon, split_into_sentences, tokenize_text,
    BigramBuildStats, BigramRecord, BigramSummaryReport, CorpusImportSummaryReport, CorpusRegistry,
    CorpusRegistryEntry, FrequencyBuildStats, FrequencyJoinSummaryReport, FrequencyRecord,
    NgramBuildStats, NgramConfig, TrigramBuildStats, TrigramRecord, TrigramSummaryReport,
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
pub use sources::{SourceRegistry, SourceRegistryEntry};
pub use validate::{validate_entry, FrequencyMetadata, SourceLexiconEntry};
