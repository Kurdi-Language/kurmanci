//! Controlled Lexicon Review Infrastructure & Vocabulary Review Batch Generator.

pub mod kuwiki_batch;
pub mod merger;
pub mod queues;
pub mod schema;
pub mod vocabulary_batch;

pub use kuwiki_batch::{
    generate_kuwiki_review_batch, verify_vocabulary_evidence_provenance, ContextReference,
    KuwikiReviewBatchCandidate, KuwikiReviewBatchManifest, KuwikiReviewBatchSummary,
    SpecialTargetBatchPresence, DEFAULT_KUWIKI_BATCH_ID, DEFAULT_KUWIKI_BATCH_SIZE,
    KUWIKI_REVIEW_BATCH_MANIFEST_SCHEMA_VERSION, KUWIKI_REVIEW_BATCH_SCHEMA_VERSION,
};
pub use merger::{validate_review_decisions, ReviewMergerSummary};
pub use queues::{generate_review_queues, ReviewQueueSummary};
pub use schema::{
    compute_conflict_group_id, compute_entry_id, ReviewDecisionRecord, ReviewDecisionStatus,
    ReviewTargetType, REVIEW_DECISION_SCHEMA_VERSION,
};
pub use vocabulary_batch::{
    generate_vocabulary_review_batch, VocabularyReviewBatchSummary, VocabularyReviewRecord,
    VOCABULARY_REVIEW_SUMMARY_SCHEMA,
};
