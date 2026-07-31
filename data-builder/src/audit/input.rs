//! Input loaders for the quality audit. Reads all required artifacts and
//! replays the preserved `.dic` through the shared parser.

use crate::eval::BenchmarkItem;
use crate::importers::{
    parse_hunspell_source, ConflictRecord, HunspellSourceEvent, ImportSummaryReport,
    ImportedLexiconRecord, RejectedRecord,
};
use crate::sources::SourceRegistry;
use crate::validate::SourceLexiconEntry;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

// ─── Shared audit types ─────────────────────────────────────────────────────

/// A single parsed source record before deduplication, preserving all evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditableSourceRecord {
    pub source_line_num: usize,
    pub raw_line: String,
    pub word: String,
    pub normalized: String,
    pub flags: String,
    pub morphology: Vec<String>,
    pub part_of_speech: String,
}

/// A rejected source record with reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditableRejectedRecord {
    pub source_line_num: usize,
    pub raw_line: String,
    pub reason_code: String,
    pub explanation: String,
}

/// A blank or header line in the source.
#[derive(Debug, Clone)]
pub struct AuditableNonEntry {
    pub source_line_num: usize,
    pub raw_line: String,
    pub kind: NonEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonEntryKind {
    DeclaredCount(usize),
    Blank,
    Utf8Error,
}

/// All inputs needed for the quality audit, loaded once.
pub struct AuditInputs {
    pub source_id: String,
    pub import_summary: ImportSummaryReport,
    pub imported_records: Vec<ImportedLexiconRecord>,
    pub importer_conflicts: Vec<ConflictRecord>,
    pub importer_rejections: Vec<RejectedRecord>,
    pub replayed_parsed: Vec<AuditableSourceRecord>,
    pub replayed_rejected: Vec<AuditableRejectedRecord>,
    pub replayed_non_entries: Vec<AuditableNonEntry>,
    pub physical_line_count: usize,
    pub declared_entry_count: Option<usize>,
    pub manual_seed: Vec<SourceLexiconEntry>,
    pub benchmark_items: Vec<BenchmarkItem>,
    pub source_revision: String,
}

// ─── Loaders ────────────────────────────────────────────────────────────────

/// Loads all audit inputs from the filesystem.
pub fn load_all_inputs(source_id: &str, root: &Path) -> Result<AuditInputs, String> {
    // Load import summary
    let summary_path = root
        .join("data/reports")
        .join(source_id)
        .join("import-summary.json");
    let import_summary = load_import_summary(&summary_path)?;

    // Load imported lexicon
    let lexicon_path = root
        .join("data/imported")
        .join(source_id)
        .join("lexicon.jsonl");
    let imported_records = load_imported_records(&lexicon_path)?;

    // Load importer conflicts
    let conflicts_path = root
        .join("data/reports")
        .join(source_id)
        .join("conflicts.jsonl");
    let importer_conflicts = load_conflicts(&conflicts_path)?;

    // Load importer rejections
    let rejections_path = root
        .join("data/reports")
        .join(source_id)
        .join("rejected.jsonl");
    let importer_rejections = load_rejections(&rejections_path)?;

    // Load source registry to find the .dic file
    let registry_path = root.join("data/source-registry/sources.toml");
    let registry = SourceRegistry::load_from_file(&registry_path)?;
    let source = registry
        .sources
        .iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| format!("Source ID '{}' not found in registry", source_id))?;

    let dic_file_entry = source
        .files
        .iter()
        .find(|f| f.path.ends_with(".dic"))
        .ok_or_else(|| format!("No .dic file registered for source '{}'", source_id))?;

    let dic_path = root.join(&dic_file_entry.path);

    // Replay the preserved .dic through the shared parser
    let (
        replayed_parsed,
        replayed_rejected,
        replayed_non_entries,
        physical_line_count,
        declared_entry_count,
    ) = replay_source_records(&dic_path)?;

    // Load manual seed
    let seed_path = root.join("data/reviewed/lexicon.jsonl");
    let manual_seed = load_manual_seed(&seed_path)?;

    // Load benchmark
    let benchmark_path = root.join("data/benchmarks/spelling_gold.jsonl");
    let benchmark_items = load_benchmark(&benchmark_path)?;

    let source_revision = source.version.clone();

    Ok(AuditInputs {
        source_id: source_id.to_string(),
        import_summary,
        imported_records,
        importer_conflicts,
        importer_rejections,
        replayed_parsed,
        replayed_rejected,
        replayed_non_entries,
        physical_line_count,
        declared_entry_count,
        manual_seed,
        benchmark_items,
        source_revision,
    })
}

fn load_import_summary(path: &Path) -> Result<ImportSummaryReport, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read import summary {:?}: {}", path, e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse import summary {:?}: {}", path, e))
}

fn load_imported_records(path: &Path) -> Result<Vec<ImportedLexiconRecord>, String> {
    let file = File::open(path)
        .map_err(|e| format!("Failed to open imported lexicon {:?}: {}", path, e))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (idx, line_res) in reader.lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error at line {} of {:?}: {}", idx + 1, path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: ImportedLexiconRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON parse error at line {} of {:?}: {}", idx + 1, path, e))?;
        records.push(record);
    }
    Ok(records)
}

fn load_conflicts(path: &Path) -> Result<Vec<ConflictRecord>, String> {
    let file =
        File::open(path).map_err(|e| format!("Failed to open conflicts {:?}: {}", path, e))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (idx, line_res) in reader.lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error at line {} of {:?}: {}", idx + 1, path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: ConflictRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON parse error at line {} of {:?}: {}", idx + 1, path, e))?;
        records.push(record);
    }
    Ok(records)
}

fn load_rejections(path: &Path) -> Result<Vec<RejectedRecord>, String> {
    let file =
        File::open(path).map_err(|e| format!("Failed to open rejections {:?}: {}", path, e))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (idx, line_res) in reader.lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error at line {} of {:?}: {}", idx + 1, path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: RejectedRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON parse error at line {} of {:?}: {}", idx + 1, path, e))?;
        records.push(record);
    }
    Ok(records)
}

fn load_manual_seed(path: &Path) -> Result<Vec<SourceLexiconEntry>, String> {
    let file =
        File::open(path).map_err(|e| format!("Failed to open manual seed {:?}: {}", path, e))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (idx, line_res) in reader.lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error at line {} of {:?}: {}", idx + 1, path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: SourceLexiconEntry = serde_json::from_str(&line)
            .map_err(|e| format!("JSON parse error at line {} of {:?}: {}", idx + 1, path, e))?;
        records.push(record);
    }
    Ok(records)
}

fn load_benchmark(path: &Path) -> Result<Vec<BenchmarkItem>, String> {
    let file =
        File::open(path).map_err(|e| format!("Failed to open benchmark {:?}: {}", path, e))?;
    let reader = BufReader::new(file);
    let mut items = Vec::new();

    for (idx, line_res) in reader.lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error at line {} of {:?}: {}", idx + 1, path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let item: BenchmarkItem = serde_json::from_str(&line)
            .map_err(|e| format!("JSON parse error at line {} of {:?}: {}", idx + 1, path, e))?;
        items.push(item);
    }
    Ok(items)
}

/// Result type for replay_source_records to avoid type_complexity warnings.
type ReplayResult = (
    Vec<AuditableSourceRecord>,
    Vec<AuditableRejectedRecord>,
    Vec<AuditableNonEntry>,
    usize,         // physical_line_count
    Option<usize>, // declared_entry_count
);

/// Replays the preserved `.dic` through the shared parser, emitting every
/// parsed, rejected, blank, and declared-count line.
fn replay_source_records(dic_path: &Path) -> Result<ReplayResult, String> {
    let file = File::open(dic_path)
        .map_err(|e| format!("Failed to open .dic file {:?}: {}", dic_path, e))?;
    let reader = BufReader::new(file);
    let events = parse_hunspell_source(reader);

    let physical_line_count = events.len();
    let mut parsed = Vec::new();
    let mut rejected = Vec::new();
    let mut non_entries = Vec::new();
    let mut declared_entry_count = None;

    for event in events {
        match event {
            HunspellSourceEvent::DeclaredCount {
                source_line_num,
                raw_line,
                count,
            } => {
                declared_entry_count = Some(count);
                non_entries.push(AuditableNonEntry {
                    source_line_num,
                    raw_line,
                    kind: NonEntryKind::DeclaredCount(count),
                });
            }
            HunspellSourceEvent::Blank {
                source_line_num,
                raw_line,
            } => {
                non_entries.push(AuditableNonEntry {
                    source_line_num,
                    raw_line,
                    kind: NonEntryKind::Blank,
                });
            }
            HunspellSourceEvent::Parsed {
                source_line_num,
                raw_line,
                entry,
                normalized,
                part_of_speech,
            } => {
                parsed.push(AuditableSourceRecord {
                    source_line_num,
                    raw_line,
                    word: entry.raw_word,
                    normalized,
                    flags: entry.flags,
                    morphology: entry.morphology,
                    part_of_speech,
                });
            }
            HunspellSourceEvent::Rejected {
                source_line_num,
                raw_line,
                reason_code,
                explanation,
            } => {
                rejected.push(AuditableRejectedRecord {
                    source_line_num,
                    raw_line,
                    reason_code,
                    explanation,
                });
            }
            HunspellSourceEvent::Utf8Error {
                source_line_num,
                explanation,
            } => {
                rejected.push(AuditableRejectedRecord {
                    source_line_num,
                    raw_line: String::new(),
                    reason_code: "INVALID_UTF8".to_string(),
                    explanation,
                });
                non_entries.push(AuditableNonEntry {
                    source_line_num,
                    raw_line: String::new(),
                    kind: NonEntryKind::Utf8Error,
                });
            }
        }
    }

    Ok((
        parsed,
        rejected,
        non_entries,
        physical_line_count,
        declared_entry_count,
    ))
}
