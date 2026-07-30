pub mod compile;
pub mod config;
pub mod eval;
pub mod importers;
pub mod merge;
pub mod normalize;
pub mod report;
pub mod sources;
pub mod validate;

pub use compile::{calculate_sha256, compile_binary_pack, write_artifacts, ReleaseManifest};
pub use config::BuilderConfig;
pub use eval::{evaluate_lexicon_impact, EvaluationReport, LexiconMetrics};
pub use importers::{import_hunspell_dic, parse_hunspell_line, ImportSummaryReport};
pub use merge::merge_and_deduplicate;
pub use normalize::normalize_text;
pub use report::{generate_and_save_report, BuildReport};
pub use sources::{SourceRegistry, SourceRegistryEntry};
pub use validate::{validate_entry, SourceLexiconEntry};
