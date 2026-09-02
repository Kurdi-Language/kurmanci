pub mod audit;
pub mod compile;
pub mod config;
pub mod corpus;
pub mod eval;
pub mod eval_next_word;
pub mod eval_ranking;
pub mod evaluation;
pub mod importers;
pub mod merge;
pub mod normalize;
pub mod pack;
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
    analyze_corpus_quality, audit_corpora, build_corpus_bigrams, build_corpus_frequencies,
    build_corpus_ngrams, build_corpus_train_frequencies, build_corpus_trigrams,
    build_vocabulary_evidence, compute_experimental_lexicon_fingerprint, generate_corpus_inventory,
    import_all_corpora, import_corpus, join_frequencies_to_lexicon, partition_corpora,
    split_into_sentences, tokenize_text, validate_registry_relative_path, BigramBuildStats,
    BigramRecord, BigramSummaryReport, CanonicalDocumentRecord, CorpusAuditSummary,
    CorpusImportSummaryReport, CorpusInventorySummary, CorpusQualityMetrics, CorpusRegistry,
    CorpusRegistryEntry, FrequencyBuildManifest, FrequencyBuildStats, FrequencyJoinSummaryReport,
    FrequencyRecord, LexicalTokenQualityMetrics, NgramBuildStats, NgramConfig, OovCandidateRecord,
    PartitionSummary, RepresentativeContext, SourceDocumentQualityMetrics, TrigramBuildStats,
    TrigramRecord, TrigramSummaryReport, VocabularyEvidenceSummaryReport,
};
pub use eval::{evaluate_lexicon_impact, BenchmarkItem, EvaluationReport, LexiconMetrics};
pub use eval_next_word::{run_next_word_evaluation, NextWordEvalSummaryReport};
pub use eval_ranking::{run_ranking_evaluation, RankingEvalSummaryReport};
pub use evaluation::comparison::evaluate_candidate_experiment_pack;
pub use importers::{
    import_hunspell_dic, parse_hunspell_line, parse_hunspell_source, HunspellSourceEvent,
    ImportSummaryReport,
};
pub use merge::merge_and_deduplicate;
pub use normalize::normalize_text;
pub use pack::{
    build_pack, build_temp_frequency_pack, manifest::PackManifest, policy::PackPolicyConfig,
    resolve_authoritative_pack_lexicon, resolve_authoritative_pack_payload,
    AuthoritativePackResolution,
};
pub use report::{generate_and_save_report, BuildReport};
pub use review::{
    compute_conflict_group_id, compute_entry_id, generate_kuwiki_review_batch,
    generate_review_queues, load_and_validate_kuwiki_decisions, select_kuwiki_candidates_for_pack,
    validate_review_decisions, verify_vocabulary_evidence_provenance, ContextReference,
    KuwikiDecisionsSnapshot, KuwikiReviewBatchCandidate, KuwikiReviewBatchManifest,
    KuwikiReviewBatchSummary, ReviewDecisionRecord, ReviewDecisionStatus, ReviewMergerSummary,
    ReviewQueueSummary, ReviewTargetType, SpecialTargetBatchPresence, DEFAULT_KUWIKI_BATCH_ID,
    DEFAULT_KUWIKI_BATCH_SIZE, KUWIKI_REVIEW_BATCH_MANIFEST_SCHEMA_VERSION,
    KUWIKI_REVIEW_BATCH_SCHEMA_VERSION, REVIEW_DECISION_SCHEMA_VERSION,
};
pub use sources::{SourceRegistry, SourceRegistryEntry};
pub use validate::{validate_entry, FrequencyMetadata, SourceLexiconEntry};
