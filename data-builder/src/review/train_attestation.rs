//! Diagnostic-only TRAIN corpus attestation for a lexical review pool
//! (`train-attestation-v1`).
//!
//! One question, answered with counts and hashes only: for each entry of a source's ordinary
//! review pool that no human has decided yet, does its normalized form occur in the canonical
//! TRAIN document set of a registered corpus, and in how many of those documents? Nothing here
//! assigns or changes a review status, a pack, a decision, a ranking policy, the language model
//! or a release artifact, and no lexical form, context or note is written to the report.
//!
//! # Why the TRAIN partition only
//!
//! This signal exists to select the membership of an *evaluation* pack whose completion,
//! correction and coverage behaviour is then measured on the DEVELOPMENT partition. Attestation
//! computed over the whole corpus would admit a word *because* it occurs in a held-out
//! document, and the benchmark would then find that same word: the measurement would be
//! reporting its own selection rule back to us. The committed language model already draws its
//! vocabulary and n-grams from the TRAIN partition alone (`pack/language_model.rs`: "DEV/EVAL
//! partitions never contribute"), and this report deliberately uses exactly that population.
//!
//! # How that is enforced, rather than documented
//!
//! The walk is not merely described as TRAIN-only. It is proved to be the model's own document
//! set. Every record read must declare `partition == "train"`; only this corpus's canonical
//! duplicate representatives contribute, the same rule every other TRAIN consumer applies; and
//! the report then recomputes `train_document_set_sha256` over exactly the records it visited
//! and refuses to continue unless it equals the value the committed language-model manifest
//! records, alongside that manifest's train document count and the partition and corpus hashes
//! it pins. A walk that admitted one DEVELOPMENT or EVALUATION document, or that ran against a
//! different partitioning, cannot produce that hash. This is a stronger guarantee than sharing
//! the walk's code with the model builder would give, because it compares the resulting
//! document set rather than the algorithm that chose it.
//!
//! # What is not decided here
//!
//! Attestation is evidence, never a review status. A form attested in a thousand documents is
//! not thereby approved, and an unattested form is not thereby rejected; both remain exactly as
//! unreviewed as they were. The report also deliberately does not size any pack: the entry
//! count of an evaluation population is produced by the pack builder that assembles it, which
//! is the only component that knows the collision and dedupe rules.

use crate::corpus::importer::calculate_file_sha256;
use crate::corpus::partition::PartitionDocumentRecord;
use crate::corpus::tokenizer::tokenize_text;
use crate::pack::language_model::{validate_model_id, LANGUAGE_MODEL_SCHEMA_VERSION};
use crate::sources::SourceRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

pub const TRAIN_ATTESTATION_SCHEMA_VERSION: &str = "train-attestation-v1";

/// The ordinary review pool of a source: the candidates the review infrastructure placed in
/// front of a human, after the alphabet and punctuation policies took their exclusions out.
pub const POOL_FILE: &str = "hunspell-only.jsonl";
const CONFLICT_GROUPS_FILE: &str = "metadata-conflict-groups.jsonl";
const ALPHABET_EXCLUDED_FILE: &str = "alphabet-policy-excluded.jsonl";
const PUNCTUATION_HELD_FILE: &str = "punctuation-policy-needs-linguist.jsonl";

/// Document-count buckets. Deliberately finer than any threshold anyone might want to adopt:
/// the distribution is published so that a boundary can be argued from it later, and this
/// report proposes none.
const BUCKETS: [(&str, usize, Option<usize>); 7] = [
    ("0", 0, Some(0)),
    ("1-4", 1, Some(4)),
    ("5-9", 5, Some(9)),
    ("10-24", 10, Some(24)),
    ("25-49", 25, Some(49)),
    ("50-99", 50, Some(99)),
    ("100+", 100, None),
];

fn bucket_of(doc_count: usize) -> &'static str {
    for (label, lo, hi) in BUCKETS.iter() {
        if doc_count >= *lo && hi.map(|h| doc_count <= h).unwrap_or(true) {
            return label;
        }
    }
    unreachable!("bucket table covers every count")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrainAttestationReport {
    pub schema_version: String,
    pub source: SourceProvenance,
    pub corpus: CorpusProvenance,
    pub funnel: Funnel,
    /// Pending entries per document-count bucket, in the order the bucket table declares.
    pub pending_by_bucket: Vec<BucketCount>,
    /// Already-decided entries per bucket with their recorded outcomes. Published because it is
    /// the only existing evidence of how attestation relates to reviewability, and it is
    /// observational: these entries were not selected at random, so it supports no inference on
    /// its own. It is here to be superseded by a randomised, blinded sample.
    pub decided_by_bucket: Vec<BucketOutcomes>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceProvenance {
    pub source_id: String,
    pub source_revision: String,
    pub pool_file: String,
    pub pool_sha256: String,
    pub conflict_groups_sha256: String,
    pub decisions_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorpusProvenance {
    pub corpus_id: String,
    pub corpus_version: String,
    pub language_model_id: String,
    pub language_model_manifest_sha256: String,
    pub corpus_source_artifact_sha256: String,
    pub corpus_documents_sha256: String,
    pub corpus_registry_sha256: String,
    pub canonical_manifest_sha256: String,
    pub partition_manifest_sha256: String,
    pub train_partition_sha256: String,
    pub train_records_read: usize,
    pub train_document_count: usize,
    pub train_document_set_sha256: String,
    pub tokenizer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Funnel {
    pub pool_records: usize,
    pub excluded_conflict_group_member: usize,
    pub excluded_already_decided: usize,
    pub pending: usize,
    pub pending_train_attested: usize,
    pub pending_train_unattested: usize,
    /// Distinct normalized forms among the pending entries. Lower than `pending` wherever the
    /// source carries several entries for one form; the identity of a pack entry is the
    /// normalized form, so this is the figure a vocabulary population is built from.
    pub pending_distinct_normalized: usize,
    pub pending_distinct_normalized_train_attested: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BucketCount {
    pub bucket: String,
    pub entries: usize,
    pub distinct_normalized: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BucketOutcomes {
    pub bucket: String,
    pub decided: usize,
    /// Recorded review statuses and their counts, verbatim as the decisions carry them.
    pub by_status: BTreeMap<String, usize>,
}

// ---------------------------------------------------------------------------
// Input records. Each declares only the fields this report reads, so an unrelated schema
// addition upstream cannot break it.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PoolRecord {
    target_id: String,
    normalized: String,
    #[serde(default)]
    source_id: String,
    #[serde(default)]
    source_revision: String,
}

#[derive(Debug, Deserialize)]
struct ConflictGroupRecord {
    #[serde(default)]
    member_entry_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DecisionRecord {
    target_id: String,
    review_status: String,
}

#[derive(Debug, Deserialize)]
struct TargetIdRecord {
    target_id: String,
}

#[derive(Debug, Deserialize)]
struct ModelManifestFields {
    schema_version: Option<String>,
    model_id: String,
    corpus_id: String,
    corpus_version: String,
    corpus_source_artifact_sha256: String,
    corpus_documents_sha256: String,
    corpus_registry_sha256: String,
    canonical_manifest_sha256: String,
    partition_manifest_sha256: String,
    train_partition_sha256: String,
    train_document_count: usize,
    train_document_set_sha256: String,
}

#[derive(Debug, Deserialize)]
struct PartitionManifestFields {
    train_documents: usize,
}

/// A source id is a single path component naming a registered source. Validated before any
/// path is derived from it, the same rule `validate_model_id` applies to a model id.
fn validate_source_id(source_id: &str) -> Result<(), String> {
    let ok = !source_id.is_empty()
        && source_id.len() <= 128
        && !source_id.starts_with('.')
        && source_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Invalid source id '{}' (ASCII letters, digits, '-', '_', '.' only)",
            source_id
        ))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn read_jsonl_lines(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Builds the report. `model_id` names the committed language model whose manifest is the
/// anchor for the TRAIN document set; `corpus_id` must be the corpus that model was built from.
pub fn build_train_attestation_report<P: AsRef<Path>>(
    root_dir: P,
    corpus_id: &str,
    source_id: &str,
    model_id: &str,
) -> Result<TrainAttestationReport, String> {
    let root = root_dir.as_ref();

    // ------------------------------------------------------------------
    // 1. The language-model manifest: the anchor for the TRAIN population.
    // ------------------------------------------------------------------
    validate_model_id(model_id)?;
    let model_manifest_path = root
        .join("data/language-model")
        .join(model_id)
        .join("manifest.json");
    let model_bytes = fs::read(&model_manifest_path).map_err(|e| {
        format!(
            "language model '{}' has no manifest at {:?}: {}",
            model_id, model_manifest_path, e
        )
    })?;
    let language_model_manifest_sha256 = sha256_hex(&model_bytes);
    let model: ModelManifestFields = serde_json::from_slice(&model_bytes).map_err(|e| {
        format!(
            "Invalid language model manifest {:?}: {}",
            model_manifest_path, e
        )
    })?;
    // The manifest must be of the schema this report understands before any of its corpus,
    // partition or document-set fields are interpreted: a document that merely happens to
    // carry these field names is not a language-model manifest.
    if model.schema_version.as_deref() != Some(LANGUAGE_MODEL_SCHEMA_VERSION) {
        return Err(format!(
            "language model manifest {:?} has schema {:?}, expected {:?}; refusing to interpret its corpus and partition provenance",
            model_manifest_path, model.schema_version, LANGUAGE_MODEL_SCHEMA_VERSION
        ));
    }
    if model.model_id != model_id {
        return Err(format!(
            "language model manifest {:?} declares model_id '{}', '{}' was requested",
            model_manifest_path, model.model_id, model_id
        ));
    }
    if model.corpus_id != corpus_id {
        return Err(format!(
            "language model '{}' was built from corpus '{}', attestation was requested for '{}'; a model's TRAIN document set is evidence for its own corpus only",
            model_id, model.corpus_id, corpus_id
        ));
    }

    // The corpus artifacts the model pinned must still be the ones on disk, otherwise the
    // document set it recorded describes different bytes than the walk is about to read.
    for (label, path, recorded) in [
        (
            "corpus registry",
            root.join("data/source-registry/corpora.toml"),
            &model.corpus_registry_sha256,
        ),
        (
            "canonical import manifest",
            root.join("data/imported-canonical/manifest.json"),
            &model.canonical_manifest_sha256,
        ),
    ] {
        let actual = calculate_file_sha256(&path)
            .map_err(|e| format!("{} at {:?} could not be hashed: {}", label, path, e))?;
        if &actual != recorded {
            return Err(format!(
                "{} at {:?} hashes {} but language model '{}' was built against {}; the corpus state has changed since the model was built",
                label, path, actual, model_id, recorded
            ));
        }
    }

    // ------------------------------------------------------------------
    // 2. The partitioning, pinned by that same manifest.
    // ------------------------------------------------------------------
    let partition_dir = root.join("data/build/corpus-partitions");
    let partition_manifest_path = partition_dir.join("manifest.json");
    let train_partition_path = partition_dir.join("train.jsonl");
    if !partition_manifest_path.exists() || !train_partition_path.exists() {
        return Err(format!(
            "corpus partitions missing under {:?}; acquire the corpus and run import-all-corpora (the corpus is external and is not in the repository)",
            partition_dir
        ));
    }
    let partition_manifest_sha256 = calculate_file_sha256(&partition_manifest_path)?;
    if partition_manifest_sha256 != model.partition_manifest_sha256 {
        return Err(format!(
            "partition manifest hashes {} but language model '{}' pins {}; the corpus was repartitioned after the model was built and TRAIN is no longer the same set of documents",
            partition_manifest_sha256, model_id, model.partition_manifest_sha256
        ));
    }
    let train_partition_sha256 = calculate_file_sha256(&train_partition_path)?;
    if train_partition_sha256 != model.train_partition_sha256 {
        return Err(format!(
            "train partition hashes {} but language model '{}' pins {}",
            train_partition_sha256, model_id, model.train_partition_sha256
        ));
    }
    let partition_manifest: PartitionManifestFields = serde_json::from_slice(
        &fs::read(&partition_manifest_path)
            .map_err(|e| format!("Failed to read {:?}: {}", partition_manifest_path, e))?,
    )
    .map_err(|e| {
        format!(
            "Invalid partition manifest {:?}: {}",
            partition_manifest_path, e
        )
    })?;

    // ------------------------------------------------------------------
    // 3. The canonical TRAIN walk, then proved to be the model's own set.
    // ------------------------------------------------------------------
    let file = File::open(&train_partition_path)
        .map_err(|e| format!("Failed to open {:?}: {}", train_partition_path, e))?;
    let mut train_records_read = 0usize;
    let mut train_document_count = 0usize;
    let mut document_set_hasher = Sha256::new();
    let mut doc_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (idx, line_res) in BufReader::new(file).lines().enumerate() {
        let line = line_res.map_err(|e| format!("Read error line {}: {}", idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: PartitionDocumentRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON error in train partition line {}: {}", idx + 1, e))?;
        // A held-out document inside the TRAIN file is a corrupt partitioning, not an input to
        // interpret: refuse rather than silently measure on it.
        if record.partition != "train" {
            return Err(format!(
                "train partition line {} declares partition '{}'; TRAIN attestation refuses to read a DEVELOPMENT or EVALUATION document",
                idx + 1,
                record.partition
            ));
        }
        train_records_read += 1;
        if record.corpus_id != corpus_id {
            continue;
        }
        // INVARIANT: only this corpus's canonical duplicate representatives contribute, the
        // same rule as every other TRAIN consumer.
        if record.corpus_id != record.canonical_corpus_id
            || record.document_id != record.canonical_document_id
        {
            continue;
        }
        train_document_count += 1;
        document_set_hasher.update(line.as_bytes());
        document_set_hasher.update(b"\n");
        for token in tokenize_text(&record.text)
            .into_iter()
            .collect::<BTreeSet<_>>()
        {
            *doc_counts.entry(token).or_insert(0) += 1;
        }
    }
    if train_records_read != partition_manifest.train_documents {
        return Err(format!(
            "train records read {} but the partition manifest declares {}",
            train_records_read, partition_manifest.train_documents
        ));
    }
    if train_document_count != model.train_document_count {
        return Err(format!(
            "walked {} canonical TRAIN documents of corpus '{}' but language model '{}' recorded {}",
            train_document_count, corpus_id, model_id, model.train_document_count
        ));
    }
    let train_document_set_sha256 = format!("{:x}", document_set_hasher.finalize());
    if train_document_set_sha256 != model.train_document_set_sha256 {
        return Err(format!(
            "the canonical TRAIN document set walked here hashes {} but language model '{}' recorded {}; this report refuses to attribute attestation to a document set the model did not use",
            train_document_set_sha256, model_id, model.train_document_set_sha256
        ));
    }

    // ------------------------------------------------------------------
    // 4. The source: a registered source id, validated before a path is built from it.
    // ------------------------------------------------------------------
    validate_source_id(source_id)?;
    let sources_path = root.join("data/source-registry/sources.toml");
    let registry = SourceRegistry::load_from_file(&sources_path).map_err(|e| {
        format!(
            "Source registry {:?} could not be read: {}",
            sources_path, e
        )
    })?;
    let registered = registry
        .sources
        .iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| {
            format!(
                "source '{}' is not registered in {:?}; attestation is reported for registered sources only",
                source_id, sources_path
            )
        })?;
    let registered_version = registered.version.clone();

    // ------------------------------------------------------------------
    // 5. The review pool and what has already been taken out of it.
    // ------------------------------------------------------------------
    let queue_dir = root.join("data/review-queues").join(source_id);
    let pool_path = queue_dir.join(POOL_FILE);
    let conflict_path = queue_dir.join(CONFLICT_GROUPS_FILE);
    let decisions_path = root
        .join("data/review-decisions")
        .join(source_id)
        .join("decisions.jsonl");
    for p in [&pool_path, &conflict_path, &decisions_path] {
        if !p.exists() {
            return Err(format!(
                "required review artifact missing at {:?}; run generate-review-queues first",
                p
            ));
        }
    }
    let pool_sha256 = calculate_file_sha256(&pool_path)?;
    let conflict_groups_sha256 = calculate_file_sha256(&conflict_path)?;
    let decisions_sha256 = calculate_file_sha256(&decisions_path)?;

    let mut conflict_members: BTreeSet<String> = BTreeSet::new();
    for line in read_jsonl_lines(&conflict_path)? {
        let rec: ConflictGroupRecord = serde_json::from_str(&line)
            .map_err(|e| format!("Invalid conflict group record: {}", e))?;
        conflict_members.extend(rec.member_entry_ids);
    }
    let mut decisions: BTreeMap<String, String> = BTreeMap::new();
    for line in read_jsonl_lines(&decisions_path)? {
        let rec: DecisionRecord =
            serde_json::from_str(&line).map_err(|e| format!("Invalid decision record: {}", e))?;
        decisions.insert(rec.target_id, rec.review_status);
    }

    // Consistency of the authoritative artifacts against each other: an entry the policy took
    // out must not also sit in the ordinary pool. This compares committed artifacts and
    // re-evaluates no character.
    let mut policy_removed: BTreeSet<String> = BTreeSet::new();
    for name in [ALPHABET_EXCLUDED_FILE, PUNCTUATION_HELD_FILE] {
        let p = queue_dir.join(name);
        if p.exists() {
            for line in read_jsonl_lines(&p)? {
                if let Ok(rec) = serde_json::from_str::<TargetIdRecord>(&line) {
                    policy_removed.insert(rec.target_id);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // 6. The funnel.
    // ------------------------------------------------------------------
    let mut pool_records = 0usize;
    let mut excluded_conflict = 0usize;
    let mut excluded_decided = 0usize;
    let mut pending_entries_by_bucket: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut pending_forms_by_bucket: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    let mut decided_by_bucket: BTreeMap<&'static str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut pending_forms: BTreeSet<String> = BTreeSet::new();
    let mut pending_forms_attested: BTreeSet<String> = BTreeSet::new();
    let mut pending = 0usize;
    let mut pending_attested = 0usize;

    for line in read_jsonl_lines(&pool_path)? {
        let rec: PoolRecord =
            serde_json::from_str(&line).map_err(|e| format!("Invalid pool record: {}", e))?;
        pool_records += 1;
        if !rec.source_id.is_empty() && rec.source_id != source_id {
            return Err(format!(
                "pool record {} declares source_id '{}', '{}' was requested",
                rec.target_id, rec.source_id, source_id
            ));
        }
        // The pool must describe the registered immutable version of the source, otherwise the
        // attestation is reported against a different set of lexical entries than the registry
        // records.
        if rec.source_revision != registered_version {
            return Err(format!(
                "pool record {} declares source_revision '{}', the registry records '{}' for source '{}'; regenerate the review queues",
                rec.target_id, rec.source_revision, registered_version, source_id
            ));
        }
        if policy_removed.contains(&rec.target_id) {
            return Err(format!(
                "target {} is in both {} and a policy-exclusion queue; regenerate the review queues before measuring attestation",
                rec.target_id, POOL_FILE
            ));
        }
        let doc_count = doc_counts.get(&rec.normalized).copied().unwrap_or(0);
        let bucket = bucket_of(doc_count);

        if let Some(status) = decisions.get(&rec.target_id) {
            excluded_decided += 1;
            *decided_by_bucket
                .entry(bucket)
                .or_default()
                .entry(status.clone())
                .or_insert(0) += 1;
            continue;
        }
        if conflict_members.contains(&rec.target_id) {
            excluded_conflict += 1;
            continue;
        }
        pending += 1;
        pending_forms.insert(rec.normalized.clone());
        *pending_entries_by_bucket.entry(bucket).or_insert(0) += 1;
        pending_forms_by_bucket
            .entry(bucket)
            .or_default()
            .insert(rec.normalized.clone());
        if doc_count > 0 {
            pending_attested += 1;
            pending_forms_attested.insert(rec.normalized.clone());
        }
    }

    let pending_by_bucket = BUCKETS
        .iter()
        .map(|(label, _, _)| BucketCount {
            bucket: (*label).to_string(),
            entries: pending_entries_by_bucket.get(label).copied().unwrap_or(0),
            distinct_normalized: pending_forms_by_bucket
                .get(label)
                .map(|s| s.len())
                .unwrap_or(0),
        })
        .collect();
    let decided_by_bucket = BUCKETS
        .iter()
        .filter_map(|(label, _, _)| {
            decided_by_bucket
                .get(label)
                .map(|by_status| BucketOutcomes {
                    bucket: (*label).to_string(),
                    decided: by_status.values().sum(),
                    by_status: by_status.clone(),
                })
        })
        .collect();

    Ok(TrainAttestationReport {
        schema_version: TRAIN_ATTESTATION_SCHEMA_VERSION.to_string(),
        source: SourceProvenance {
            source_id: source_id.to_string(),
            source_revision: registered_version,
            pool_file: POOL_FILE.to_string(),
            pool_sha256,
            conflict_groups_sha256,
            decisions_sha256,
        },
        corpus: CorpusProvenance {
            corpus_id: corpus_id.to_string(),
            corpus_version: model.corpus_version,
            language_model_id: model_id.to_string(),
            language_model_manifest_sha256,
            corpus_source_artifact_sha256: model.corpus_source_artifact_sha256,
            corpus_documents_sha256: model.corpus_documents_sha256,
            corpus_registry_sha256: model.corpus_registry_sha256,
            canonical_manifest_sha256: model.canonical_manifest_sha256,
            partition_manifest_sha256,
            train_partition_sha256,
            train_records_read,
            train_document_count,
            train_document_set_sha256,
            tokenizer: "corpus::tokenizer::tokenize_text".to_string(),
        },
        funnel: Funnel {
            pool_records,
            excluded_conflict_group_member: excluded_conflict,
            excluded_already_decided: excluded_decided,
            pending,
            pending_train_attested: pending_attested,
            pending_train_unattested: pending - pending_attested,
            pending_distinct_normalized: pending_forms.len(),
            pending_distinct_normalized_train_attested: pending_forms_attested.len(),
        },
        pending_by_bucket,
        decided_by_bucket,
    })
}

/// Fixed-width text rendering. Counts and hashes only.
pub fn format_report(r: &TrainAttestationReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "TRAIN corpus attestation ({})\n  source {} @ {}\n  corpus {} {} via language model {}\n",
        r.schema_version,
        r.source.source_id,
        &r.source.source_revision,
        r.corpus.corpus_id,
        r.corpus.corpus_version,
        r.corpus.language_model_id
    ));
    out.push_str(&format!(
        "  canonical TRAIN documents {} of {} records read; document set {}\n",
        r.corpus.train_document_count,
        r.corpus.train_records_read,
        &r.corpus.train_document_set_sha256[..16]
    ));
    out.push_str("\nfunnel\n");
    for (label, value) in [
        ("review pool records", r.funnel.pool_records),
        ("less already decided", r.funnel.excluded_already_decided),
        (
            "less conflict group members",
            r.funnel.excluded_conflict_group_member,
        ),
        ("pending entries", r.funnel.pending),
        ("pending, TRAIN-attested", r.funnel.pending_train_attested),
        (
            "pending, TRAIN-unattested",
            r.funnel.pending_train_unattested,
        ),
        (
            "pending distinct normalized forms",
            r.funnel.pending_distinct_normalized,
        ),
        (
            "pending distinct forms, TRAIN-attested",
            r.funnel.pending_distinct_normalized_train_attested,
        ),
    ] {
        out.push_str(&format!("  {:<40} {:>8}\n", label, value));
    }
    out.push_str("\npending by TRAIN document count\n  bucket      entries   distinct\n");
    for b in &r.pending_by_bucket {
        out.push_str(&format!(
            "  {:<10} {:>8} {:>10}\n",
            b.bucket, b.entries, b.distinct_normalized
        ));
    }
    if !r.decided_by_bucket.is_empty() {
        out.push_str(
            "\nalready-decided entries by TRAIN document count (observational, not a random sample)\n",
        );
        for b in &r.decided_by_bucket {
            let mut parts: Vec<String> = b
                .by_status
                .iter()
                .map(|(s, n)| format!("{} {}", s, n))
                .collect();
            parts.sort();
            out.push_str(&format!(
                "  {:<10} {:>6}   {}\n",
                b.bucket,
                b.decided,
                parts.join(", ")
            ));
        }
    }
    out
}
