//! Controlled Lexicon Review Infrastructure (Milestone 4A.1).

pub mod merger;
pub mod queues;
pub mod schema;

pub use merger::{validate_review_decisions, ReviewMergerSummary};
pub use queues::{generate_review_queues, ReviewQueueSummary};
pub use schema::{
    compute_conflict_group_id, compute_entry_id, ReviewDecisionRecord, ReviewDecisionStatus,
    ReviewTargetType, REVIEW_DECISION_SCHEMA_VERSION,
};
