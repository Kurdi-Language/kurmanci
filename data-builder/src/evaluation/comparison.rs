//! Deterministic Three-Pack Benchmark Comparison Engine for Seed, Reviewed, and Experimental-Full Packs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::corpus::importer::LockFileGuard;
use crate::evaluation::reports::{calculate_file_sha256, load_benchmark_cases};
use crate::evaluation::schema::{
    BenchmarkCaseRecord, BenchmarkExpectation, BenchmarkReviewStatus, BenchmarkTask,
    BENCHMARK_CASE_SCHEMA_VERSION,
};
use crate::pack::manifest::{validate_all_pack_manifests, PackManifest};
use kurmanci_engine::Engine;

pub const COMPARISON_POLICY_VERSION: &str = "three-pack-comparison-v1";
pub const REQUIRED_PACK_IDS: [&str; 3] = ["seed", "reviewed", "experimental-full"];
pub const DEFAULT_CANDIDATE_LIMIT: usize = 10;

/// Pack binary and manifest loaded metadata.
pub struct LoadedPackInfo {
    pub pack_id: String,
    pub pack_dir: PathBuf,
    pub manifest: PackManifest,
    pub manifest_sha256: String,
    pub binary_size_bytes: u64,
    pub binary_sha256: String,
    pub engine: Engine,
}

impl std::fmt::Debug for LoadedPackInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedPackInfo")
            .field("pack_id", &self.pack_id)
            .field("pack_dir", &self.pack_dir)
            .field("manifest", &self.manifest)
            .field("manifest_sha256", &self.manifest_sha256)
            .field("binary_size_bytes", &self.binary_size_bytes)
            .field("binary_sha256", &self.binary_sha256)
            .finish()
    }
}

/// Query result for a single pack on a benchmark case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackQueryResult {
    pub accepted: bool,
    pub suggestions: Vec<String>,
    pub best_expected_rank: Option<usize>,
    pub satisfies_required_top_k: bool,
    pub forbidden_hits: Vec<String>,
    pub best_forbidden_rank: Option<usize>,
}

/// Pairwise comparison classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairwiseComparisonClass {
    Improvement,
    Regression,
    Unchanged,
}

/// Evaluation result for a single benchmark case across all three packs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseComparisonResult {
    pub case_id: String,
    pub task: String,
    pub category: String,
    pub input: String,
    pub expected_candidates: Vec<String>,
    pub forbidden_candidates: Vec<String>,
    pub expectation_accepted: Option<bool>,
    pub packs: BTreeMap<String, PackQueryResult>,
    pub reviewed_vs_seed: PairwiseComparisonClass,
    pub experimental_vs_seed: PairwiseComparisonClass,
}

/// Metric recording eligible count, matched count, excluded count, and value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue {
    pub eligible_count: usize,
    pub matched_count: usize,
    pub excluded_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// Summary metrics for a single pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinglePackMetrics {
    pub binary_size_bytes: u64,
    pub binary_sha256: String,
    pub known_word_coverage: MetricValue,
    pub false_acceptance_rate: MetricValue,
    pub top_1_accuracy: MetricValue,
    pub top_3_accuracy: MetricValue,
    pub top_5_accuracy: MetricValue,
    pub mrr: MetricValue,
    pub completion_recall: MetricValue,
    pub exact_preservation_rate: MetricValue,
    pub no_candidate_rate: MetricValue,
}

/// Summary counts for a pairwise comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairwiseSummary {
    pub improvement_count: usize,
    pub regression_count: usize,
    pub unchanged_count: usize,
}

/// Overall three-pack comparison summary report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreePackComparisonSummary {
    pub comparison_policy_version: String,
    pub benchmark_schema_version: String,
    pub benchmark_ready: bool,
    pub total_reviewed_cases: usize,
    pub reviewed_cases_sha256: String,
    pub current_pack_policy_sha256: String,
    pub candidate_limit_default: usize,
    pub review_decisions_sha256: String,
    pub review_queue_manifest_sha256: String,
    pub controlled_review_report_manifest_sha256: String,
    pub pack_manifest_sha256: BTreeMap<String, String>,
    pub binary_sha256: BTreeMap<String, String>,
    pub task_counts: BTreeMap<String, usize>,
    pub category_counts: BTreeMap<String, usize>,
    pub packs: BTreeMap<String, SinglePackMetrics>,
    pub pairwise_summaries: BTreeMap<String, PairwiseSummary>,
}

fn remove_dir_or_file<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    let p = path.as_ref();
    if p.is_dir() {
        fs::remove_dir_all(p)
    } else if p.exists() || p.symlink_metadata().is_ok() {
        fs::remove_file(p)
    } else {
        Ok(())
    }
}

fn require_matching_provenance(
    field_name: &str,
    reviewed: &Option<String>,
    experimental: &Option<String>,
) -> Result<String, String> {
    let reviewed_val = reviewed
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "Reviewed pack is missing required {} provenance or it is empty",
                field_name
            )
        })?;

    let experimental_val = experimental
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "Experimental-full pack is missing required {} provenance or it is empty",
                field_name
            )
        })?;

    if reviewed_val != experimental_val {
        return Err(format!(
            "{} mismatch between reviewed and experimental-full packs",
            field_name
        ));
    }

    Ok(reviewed_val.to_string())
}

/// Strictly validates a controlled pack directory and loads its engine.
pub fn validate_and_load_pack<P: AsRef<Path>>(
    root_dir: P,
    pack_id: &str,
    expected_policy_sha256: &str,
) -> Result<LoadedPackInfo, String> {
    let root = root_dir.as_ref();
    let pack_dir = root.join("data/build/packs").join(pack_id);

    if !pack_dir.exists() {
        return Err(format!(
            "Controlled pack directory missing at '{:?}'. Run `cargo run -p kurmanci-data-builder -- build-pack {}` first.",
            pack_dir, pack_id
        ));
    }

    let manifest_path = pack_dir.join("manifest.json");
    let binary_path = pack_dir.join("lexicon.bin");
    let collision_path = pack_dir.join("collision-report.jsonl");
    let attr_path = pack_dir.join("attribution.txt");
    let artifacts_path = pack_dir.join("artifacts.sha256");

    for f in [
        &manifest_path,
        &binary_path,
        &collision_path,
        &attr_path,
        &artifacts_path,
    ] {
        if !f.exists() {
            return Err(format!(
                "Required pack file missing at '{:?}' in pack '{}'",
                f, pack_id
            ));
        }
    }

    // Verify self-excluding artifacts.sha256 checksums
    let art_bytes = fs::read(&artifacts_path).map_err(|e| e.to_string())?;
    let art_lines: Vec<String> = BufReader::new(&art_bytes[..])
        .lines()
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    for rel in [
        "manifest.json",
        "lexicon.bin",
        "collision-report.jsonl",
        "attribution.txt",
    ] {
        let abs = pack_dir.join(rel);
        let actual_hash = calculate_file_sha256(&abs)?;
        let expected_suffix = format!("data/build/packs/{}/{}", pack_id, rel);
        let found = art_lines.iter().any(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts.len() == 2 && parts[0] == actual_hash && parts[1] == expected_suffix
        });
        if !found {
            return Err(format!(
                "Artifact SHA-256 mismatch for '{:?}' in pack '{}'",
                rel, pack_id
            ));
        }
    }

    // Load manifest
    let manifest_bytes = fs::read(&manifest_path).map_err(|e| e.to_string())?;
    let manifest_sha256 = calculate_file_sha256(&manifest_path)?;
    let manifest: PackManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;

    if manifest.pack_id != pack_id {
        return Err(format!(
            "Pack manifest pack_id '{}' mismatch: expected '{}'",
            manifest.pack_id, pack_id
        ));
    }

    if manifest.pack_format_version != 4 {
        return Err(format!(
            "Pack format version {} mismatch: expected 4 in pack '{}'",
            manifest.pack_format_version, pack_id
        ));
    }

    if manifest.model_profile != "none" {
        return Err(format!(
            "Pack model_profile '{}' mismatch: expected 'none' in pack '{}'",
            manifest.model_profile, pack_id
        ));
    }

    if manifest.frequency_entry_count != 0
        || manifest.bigram_count != 0
        || manifest.trigram_count != 0
    {
        return Err(format!(
            "Pack '{}' contains model counts (freq={}, bi={}, tri={}); expected all 0",
            pack_id, manifest.frequency_entry_count, manifest.bigram_count, manifest.trigram_count
        ));
    }

    if manifest.pack_policy_sha256 != expected_policy_sha256 {
        return Err(format!(
            "Pack '{}' policy SHA-256 '{}' does not match current pack-policy.toml SHA-256 '{}'",
            pack_id, manifest.pack_policy_sha256, expected_policy_sha256
        ));
    }

    let binary_hash = calculate_file_sha256(&binary_path)?;
    if binary_hash != manifest.binary_sha256 {
        return Err(format!(
            "Binary SHA-256 hash mismatch in pack '{}': manifest expected '{}', found '{}'",
            pack_id, manifest.binary_sha256, binary_hash
        ));
    }

    let binary_size_bytes = fs::metadata(&binary_path).map_err(|e| e.to_string())?.len();

    let binary_bytes = fs::read(&binary_path).map_err(|e| e.to_string())?;
    let mut engine = Engine::new();
    engine
        .load_binary_pack(&binary_bytes)
        .map_err(|e| format!("Failed to load engine binary for pack '{}': {}", pack_id, e))?;

    Ok(LoadedPackInfo {
        pack_id: pack_id.to_string(),
        pack_dir,
        manifest,
        manifest_sha256,
        binary_size_bytes,
        binary_sha256: binary_hash,
        engine,
    })
}

fn query_engine_for_case(engine: &Engine, record: &BenchmarkCaseRecord) -> PackQueryResult {
    let accepted = engine.contains(&record.input);

    // Always query and collect full DEFAULT_CANDIDATE_LIMIT (10) suggestions
    let suggestions = match record.task {
        BenchmarkTask::AcceptWord => Vec::new(),
        BenchmarkTask::CorrectWord => {
            let candidates = engine.suggest(&record.input, DEFAULT_CANDIDATE_LIMIT);
            candidates
                .into_iter()
                .take(DEFAULT_CANDIDATE_LIMIT)
                .map(|c| c.text)
                .collect()
        }
        BenchmarkTask::CompletePrefix => {
            let candidates = engine.complete(&record.input, DEFAULT_CANDIDATE_LIMIT);
            candidates
                .into_iter()
                .take(DEFAULT_CANDIDATE_LIMIT)
                .map(|c| c.text)
                .collect()
        }
    };

    // Calculate best_expected_rank across full collected suggestions
    let best_expected_rank = if record.expectation.expected_candidates.is_empty() {
        None
    } else {
        let mut best_r = None;
        for exp in &record.expectation.expected_candidates {
            if let Some(pos) = suggestions.iter().position(|s| s == exp) {
                let rank = pos + 1;
                best_r = Some(best_r.map_or(rank, |b: usize| b.min(rank)));
            }
        }
        best_r
    };

    let required_cutoff = record
        .expectation
        .required_top_k
        .unwrap_or(DEFAULT_CANDIDATE_LIMIT);
    let satisfies_required_top_k = best_expected_rank.is_some_and(|r| r <= required_cutoff);

    // Calculate forbidden_hits and best_forbidden_rank across full suggestions
    let mut forbidden_hits = Vec::new();
    let mut best_forbidden_rank = None;
    if !record.expectation.forbidden_candidates.is_empty() {
        for forb in &record.expectation.forbidden_candidates {
            if let Some(pos) = suggestions.iter().position(|s| s == forb) {
                let rank = pos + 1;
                forbidden_hits.push(forb.clone());
                best_forbidden_rank =
                    Some(best_forbidden_rank.map_or(rank, |b: usize| b.min(rank)));
            }
        }
    }

    PackQueryResult {
        accepted,
        suggestions,
        best_expected_rank,
        satisfies_required_top_k,
        forbidden_hits,
        best_forbidden_rank,
    }
}

pub fn classify_pairwise_comparison(
    baseline: &PackQueryResult,
    candidate: &PackQueryResult,
    exp: &BenchmarkExpectation,
    task: BenchmarkTask,
) -> PairwiseComparisonClass {
    match task {
        BenchmarkTask::AcceptWord => {
            if exp.preserve_exact == Some(true) {
                if !baseline.accepted && candidate.accepted {
                    return PairwiseComparisonClass::Improvement;
                }
                if baseline.accepted && !candidate.accepted {
                    return PairwiseComparisonClass::Regression;
                }
                return PairwiseComparisonClass::Unchanged;
            }

            if exp.accepted == Some(false) {
                if baseline.accepted && !candidate.accepted {
                    return PairwiseComparisonClass::Improvement;
                }
                if !baseline.accepted && candidate.accepted {
                    return PairwiseComparisonClass::Regression;
                }
                return PairwiseComparisonClass::Unchanged;
            }

            if exp.accepted == Some(true) {
                if !baseline.accepted && candidate.accepted {
                    return PairwiseComparisonClass::Improvement;
                }
                if baseline.accepted && !candidate.accepted {
                    return PairwiseComparisonClass::Regression;
                }
                return PairwiseComparisonClass::Unchanged;
            }

            PairwiseComparisonClass::Unchanged
        }
        BenchmarkTask::CorrectWord | BenchmarkTask::CompletePrefix => {
            // 1. Forbidden candidate changes take priority
            let b_forb = !baseline.forbidden_hits.is_empty();
            let c_forb = !candidate.forbidden_hits.is_empty();

            if !b_forb && c_forb {
                return PairwiseComparisonClass::Regression;
            }
            if b_forb && !c_forb {
                return PairwiseComparisonClass::Improvement;
            }
            if b_forb && c_forb {
                let b_rank = baseline.best_forbidden_rank.unwrap_or(usize::MAX);
                let c_rank = candidate.best_forbidden_rank.unwrap_or(usize::MAX);
                if c_rank < b_rank {
                    return PairwiseComparisonClass::Regression;
                }
                if c_rank > b_rank {
                    return PairwiseComparisonClass::Improvement;
                }
            }

            // 2. Allow no candidate
            if exp.allow_no_candidate == Some(true) {
                let b_no_cand = baseline.suggestions.is_empty();
                let c_no_cand = candidate.suggestions.is_empty();

                if !b_no_cand && c_no_cand {
                    return PairwiseComparisonClass::Improvement;
                }
                if b_no_cand && !c_no_cand {
                    return PairwiseComparisonClass::Regression;
                }
            }

            // 3. Expected candidate rank changes
            if !exp.expected_candidates.is_empty() {
                let b_sat = baseline.satisfies_required_top_k;
                let c_sat = candidate.satisfies_required_top_k;

                if !b_sat && c_sat {
                    return PairwiseComparisonClass::Improvement;
                }
                if b_sat && !c_sat {
                    return PairwiseComparisonClass::Regression;
                }

                let b_rank = baseline.best_expected_rank;
                let c_rank = candidate.best_expected_rank;

                match (b_rank, c_rank) {
                    (None, Some(_)) => return PairwiseComparisonClass::Improvement,
                    (Some(_), None) => return PairwiseComparisonClass::Regression,
                    (Some(b), Some(c)) => {
                        if c < b {
                            return PairwiseComparisonClass::Improvement;
                        }
                        if c > b {
                            return PairwiseComparisonClass::Regression;
                        }
                    }
                    (None, None) => {}
                }
            }

            PairwiseComparisonClass::Unchanged
        }
    }
}

fn calculate_metric<F>(cases: &[BenchmarkCaseRecord], query_fn: F) -> MetricValue
where
    F: Fn(&BenchmarkCaseRecord) -> Option<bool>,
{
    let mut eligible = 0;
    let mut matched = 0;
    let mut excluded = 0;

    for c in cases {
        match query_fn(c) {
            Some(true) => {
                eligible += 1;
                matched += 1;
            }
            Some(false) => {
                eligible += 1;
            }
            None => {
                excluded += 1;
            }
        }
    }

    let value = if eligible == 0 {
        None
    } else {
        Some((matched as f64) / (eligible as f64))
    };

    MetricValue {
        eligible_count: eligible,
        matched_count: matched,
        excluded_count: excluded,
        value,
    }
}

fn calculate_mrr(
    cases: &[BenchmarkCaseRecord],
    pack_results: &[CaseComparisonResult],
    pack_id: &str,
) -> MetricValue {
    let mut eligible = 0;
    let mut matched = 0;
    let mut excluded = 0;
    let mut recip_sum = 0.0;

    for (idx, c) in cases.iter().enumerate() {
        if c.task == BenchmarkTask::CorrectWord && !c.expectation.expected_candidates.is_empty() {
            eligible += 1;
            let q_res = &pack_results[idx].packs[pack_id];
            if let Some(rank) = q_res.best_expected_rank {
                matched += 1;
                recip_sum += 1.0 / (rank as f64);
            }
        } else {
            excluded += 1;
        }
    }

    let value = if eligible == 0 {
        None
    } else {
        Some(recip_sum / (eligible as f64))
    };

    MetricValue {
        eligible_count: eligible,
        matched_count: matched,
        excluded_count: excluded,
        value,
    }
}

/// Evaluates three controlled packs and writes deterministic report suite under `data/reports/pack-comparison/`.
pub fn evaluate_packs<P: AsRef<Path>>(root_dir: P) -> Result<ThreePackComparisonSummary, String> {
    let root = root_dir.as_ref();

    let data_dir = root.join("data");
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).map_err(|e| format!("Failed to create data dir: {}", e))?;
    }

    let policy_path = root.join("data/pack-policy.toml");
    if !policy_path.exists() {
        return Err(
            "Authoritative pack policy file missing at 'data/pack-policy.toml'".to_string(),
        );
    }
    let current_pack_policy_sha256 = calculate_file_sha256(&policy_path)?;

    // Run strict multi-pack manifest validation
    validate_all_pack_manifests(root)?;

    let lock_path = root.join("data/pack-comparison.lock");
    let lock = LockFileGuard::acquire(&lock_path)?;

    // 1. Validate and load all three controlled packs
    let mut packs_map = BTreeMap::new();
    let mut pack_manifest_sha256 = BTreeMap::new();
    let mut binary_sha256 = BTreeMap::new();

    for p_id in REQUIRED_PACK_IDS {
        let loaded = validate_and_load_pack(root, p_id, &current_pack_policy_sha256)?;
        pack_manifest_sha256.insert(p_id.to_string(), loaded.manifest_sha256.clone());
        binary_sha256.insert(p_id.to_string(), loaded.binary_sha256.clone());
        packs_map.insert(p_id.to_string(), loaded);
    }

    // Cross-check mandatory review provenance between reviewed and experimental-full packs
    let rev_pack = &packs_map["reviewed"];
    let exp_pack = &packs_map["experimental-full"];

    let review_decisions_sha256 = require_matching_provenance(
        "review_decisions_sha256",
        &rev_pack.manifest.review_decisions_sha256,
        &exp_pack.manifest.review_decisions_sha256,
    )?;

    let review_queue_manifest_sha256 = require_matching_provenance(
        "review_queue_manifest_sha256",
        &rev_pack.manifest.review_queue_manifest_sha256,
        &exp_pack.manifest.review_queue_manifest_sha256,
    )?;

    let controlled_review_report_manifest_sha256 = require_matching_provenance(
        "controlled_review_report_manifest_sha256",
        &rev_pack.manifest.controlled_review_report_manifest_sha256,
        &exp_pack.manifest.controlled_review_report_manifest_sha256,
    )?;

    // 2. Load authoritative human-reviewed benchmark cases ONLY
    let reviewed_path = root.join("evaluation/spelling/reviewed-cases.jsonl");
    if !reviewed_path.exists() {
        return Err(
            "Authoritative benchmark file missing at 'evaluation/spelling/reviewed-cases.jsonl'"
                .to_string(),
        );
    }
    let reviewed_cases_sha256 = calculate_file_sha256(&reviewed_path)?;

    let reviewed_cases = load_benchmark_cases(&reviewed_path)?;

    // Strictly validate review_status == human-reviewed
    for case in &reviewed_cases {
        if case.review_status != BenchmarkReviewStatus::HumanReviewed {
            return Err(format!(
                "Record '{}' in reviewed-cases.jsonl has review_status '{:?}': expected 'human-reviewed'",
                case.case_id, case.review_status
            ));
        }
    }

    let benchmark_ready = !reviewed_cases.is_empty();

    // 3. Query engines and record case-level results
    let mut case_results = Vec::new();
    let mut reviewed_vs_seed_counts = PairwiseSummary {
        improvement_count: 0,
        regression_count: 0,
        unchanged_count: 0,
    };
    let mut experimental_vs_seed_counts = PairwiseSummary {
        improvement_count: 0,
        regression_count: 0,
        unchanged_count: 0,
    };

    let mut task_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();

    for case in &reviewed_cases {
        *task_counts
            .entry(case.task.as_str().to_string())
            .or_default() += 1;
        *category_counts
            .entry(case.category.as_str().to_string())
            .or_default() += 1;

        let mut q_map = BTreeMap::new();
        for p_id in REQUIRED_PACK_IDS {
            let p_info = &packs_map[p_id];
            let q_res = query_engine_for_case(&p_info.engine, case);
            q_map.insert(p_id.to_string(), q_res);
        }

        let seed_q = &q_map["seed"];
        let reviewed_q = &q_map["reviewed"];
        let experimental_q = &q_map["experimental-full"];

        let rev_class =
            classify_pairwise_comparison(seed_q, reviewed_q, &case.expectation, case.task);
        let exp_class =
            classify_pairwise_comparison(seed_q, experimental_q, &case.expectation, case.task);

        match rev_class {
            PairwiseComparisonClass::Improvement => reviewed_vs_seed_counts.improvement_count += 1,
            PairwiseComparisonClass::Regression => reviewed_vs_seed_counts.regression_count += 1,
            PairwiseComparisonClass::Unchanged => reviewed_vs_seed_counts.unchanged_count += 1,
        }

        match exp_class {
            PairwiseComparisonClass::Improvement => {
                experimental_vs_seed_counts.improvement_count += 1
            }
            PairwiseComparisonClass::Regression => {
                experimental_vs_seed_counts.regression_count += 1
            }
            PairwiseComparisonClass::Unchanged => experimental_vs_seed_counts.unchanged_count += 1,
        }

        case_results.push(CaseComparisonResult {
            case_id: case.case_id.clone(),
            task: case.task.as_str().to_string(),
            category: case.category.as_str().to_string(),
            input: case.input.clone(),
            expected_candidates: case.expectation.expected_candidates.clone(),
            forbidden_candidates: case.expectation.forbidden_candidates.clone(),
            expectation_accepted: case.expectation.accepted,
            packs: q_map,
            reviewed_vs_seed: rev_class,
            experimental_vs_seed: exp_class,
        });
    }

    // 4. Calculate metrics for each pack
    let mut pack_metrics = BTreeMap::new();
    for p_id in REQUIRED_PACK_IDS {
        let p_info = &packs_map[p_id];

        let kw_coverage = calculate_metric(&reviewed_cases, |c| {
            if c.task == BenchmarkTask::AcceptWord && c.expectation.accepted == Some(true) {
                let idx = reviewed_cases.iter().position(|r| r.case_id == c.case_id)?;
                Some(case_results[idx].packs[p_id].accepted)
            } else {
                None
            }
        });

        let fa_rate = calculate_metric(&reviewed_cases, |c| {
            if c.task == BenchmarkTask::AcceptWord && c.expectation.accepted == Some(false) {
                let idx = reviewed_cases.iter().position(|r| r.case_id == c.case_id)?;
                Some(case_results[idx].packs[p_id].accepted)
            } else {
                None
            }
        });

        let top_1 = calculate_metric(&reviewed_cases, |c| {
            if c.task == BenchmarkTask::CorrectWord && !c.expectation.expected_candidates.is_empty()
            {
                let idx = reviewed_cases.iter().position(|r| r.case_id == c.case_id)?;
                Some(case_results[idx].packs[p_id].best_expected_rank == Some(1))
            } else {
                None
            }
        });

        let top_3 = calculate_metric(&reviewed_cases, |c| {
            if c.task == BenchmarkTask::CorrectWord && !c.expectation.expected_candidates.is_empty()
            {
                let idx = reviewed_cases.iter().position(|r| r.case_id == c.case_id)?;
                Some(
                    case_results[idx].packs[p_id]
                        .best_expected_rank
                        .is_some_and(|r| r <= 3),
                )
            } else {
                None
            }
        });

        let top_5 = calculate_metric(&reviewed_cases, |c| {
            if c.task == BenchmarkTask::CorrectWord && !c.expectation.expected_candidates.is_empty()
            {
                let idx = reviewed_cases.iter().position(|r| r.case_id == c.case_id)?;
                Some(
                    case_results[idx].packs[p_id]
                        .best_expected_rank
                        .is_some_and(|r| r <= 5),
                )
            } else {
                None
            }
        });

        let mrr = calculate_mrr(&reviewed_cases, &case_results, p_id);

        let comp_recall = calculate_metric(&reviewed_cases, |c| {
            if c.task == BenchmarkTask::CompletePrefix
                && !c.expectation.expected_candidates.is_empty()
            {
                let idx = reviewed_cases.iter().position(|r| r.case_id == c.case_id)?;
                Some(case_results[idx].packs[p_id].best_expected_rank.is_some())
            } else {
                None
            }
        });

        let exact_pres = calculate_metric(&reviewed_cases, |c| {
            if c.task == BenchmarkTask::AcceptWord && c.expectation.preserve_exact == Some(true) {
                let idx = reviewed_cases.iter().position(|r| r.case_id == c.case_id)?;
                Some(case_results[idx].packs[p_id].accepted)
            } else {
                None
            }
        });

        let no_cand = calculate_metric(&reviewed_cases, |c| {
            if c.expectation.allow_no_candidate == Some(true) {
                let idx = reviewed_cases.iter().position(|r| r.case_id == c.case_id)?;
                let q = &case_results[idx].packs[p_id];
                match c.task {
                    BenchmarkTask::CorrectWord | BenchmarkTask::CompletePrefix => {
                        Some(q.suggestions.is_empty())
                    }
                    BenchmarkTask::AcceptWord => Some(q.suggestions.is_empty() && !q.accepted),
                }
            } else {
                None
            }
        });

        pack_metrics.insert(
            p_id.to_string(),
            SinglePackMetrics {
                binary_size_bytes: p_info.binary_size_bytes,
                binary_sha256: p_info.binary_sha256.clone(),
                known_word_coverage: kw_coverage,
                false_acceptance_rate: fa_rate,
                top_1_accuracy: top_1,
                top_3_accuracy: top_3,
                top_5_accuracy: top_5,
                mrr,
                completion_recall: comp_recall,
                exact_preservation_rate: exact_pres,
                no_candidate_rate: no_cand,
            },
        );
    }

    let mut pairwise_summaries = BTreeMap::new();
    pairwise_summaries.insert("reviewed_vs_seed".to_string(), reviewed_vs_seed_counts);
    pairwise_summaries.insert(
        "experimental_vs_seed".to_string(),
        experimental_vs_seed_counts,
    );

    let summary = ThreePackComparisonSummary {
        comparison_policy_version: COMPARISON_POLICY_VERSION.to_string(),
        benchmark_schema_version: BENCHMARK_CASE_SCHEMA_VERSION.to_string(),
        benchmark_ready,
        total_reviewed_cases: reviewed_cases.len(),
        reviewed_cases_sha256,
        current_pack_policy_sha256,
        candidate_limit_default: DEFAULT_CANDIDATE_LIMIT,
        review_decisions_sha256,
        review_queue_manifest_sha256,
        controlled_review_report_manifest_sha256,
        pack_manifest_sha256,
        binary_sha256,
        task_counts,
        category_counts,
        packs: pack_metrics,
        pairwise_summaries,
    };

    // 5. Write reports atomically under data/reports/pack-comparison/
    let stage_dir = root.join("data/reports/.pack-comparison.tmp-stage");
    let backup_dir = root.join("data/reports/.pack-comparison.tmp-backup");
    let out_dir = root.join("data/reports/pack-comparison");

    if stage_dir.exists() {
        remove_dir_or_file(&stage_dir).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&stage_dir).map_err(|e| e.to_string())?;

    // summary.json
    fs::write(
        stage_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    // case-results.jsonl
    let mut case_res_file =
        File::create(stage_dir.join("case-results.jsonl")).map_err(|e| e.to_string())?;
    for cr in &case_results {
        writeln!(
            case_res_file,
            "{}",
            serde_json::to_string(cr).map_err(|e| e.to_string())?
        )
        .map_err(|e| e.to_string())?;
    }

    // improvements.jsonl
    let mut imp_file =
        File::create(stage_dir.join("improvements.jsonl")).map_err(|e| e.to_string())?;
    for cr in &case_results {
        if cr.reviewed_vs_seed == PairwiseComparisonClass::Improvement
            || cr.experimental_vs_seed == PairwiseComparisonClass::Improvement
        {
            writeln!(
                imp_file,
                "{}",
                serde_json::to_string(cr).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())?;
        }
    }

    // regressions.jsonl
    let mut reg_file =
        File::create(stage_dir.join("regressions.jsonl")).map_err(|e| e.to_string())?;
    for cr in &case_results {
        if cr.reviewed_vs_seed == PairwiseComparisonClass::Regression
            || cr.experimental_vs_seed == PairwiseComparisonClass::Regression
        {
            writeln!(
                reg_file,
                "{}",
                serde_json::to_string(cr).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())?;
        }
    }

    // false-acceptances.jsonl
    let mut fa_file =
        File::create(stage_dir.join("false-acceptances.jsonl")).map_err(|e| e.to_string())?;
    for cr in &case_results {
        let is_fa = (cr.expectation_accepted == Some(false)
            && cr.packs.values().any(|p| p.accepted))
            || cr.packs.values().any(|p| !p.forbidden_hits.is_empty());
        if is_fa {
            writeln!(
                fa_file,
                "{}",
                serde_json::to_string(cr).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())?;
        }
    }

    // README.md
    let readme_content = format!(
        "# Three-Pack Benchmark Comparison Report\n\n- Comparison Policy Version: {}\n- Benchmark Schema Version: {}\n- Benchmark Ready: {}\n- Total Reviewed Cases: {}\n- Reviewed Cases SHA-256: {}\n- Current Pack Policy SHA-256: {}\n- Review Decisions SHA-256: {}\n- Review Queue Manifest SHA-256: {}\n- Controlled Review Report Manifest SHA-256: {}\n- Candidate Collection Limit: {}\n\n## MRR Policy\n\nMRR includes `correct-word` cases with one or more expected candidates. Each case uses the best rank among its expected candidates in the fixed candidate list. An eligible case with no expected candidate in that list contributes zero. MRR is unavailable when there are no eligible cases. `required_top_k` is evaluated separately and does not truncate the candidate list used by MRR or Top-1/3/5.\n",
        summary.comparison_policy_version,
        summary.benchmark_schema_version,
        summary.benchmark_ready,
        summary.total_reviewed_cases,
        summary.reviewed_cases_sha256,
        summary.current_pack_policy_sha256,
        summary.review_decisions_sha256,
        summary.review_queue_manifest_sha256,
        summary.controlled_review_report_manifest_sha256,
        summary.candidate_limit_default,
    );
    fs::write(stage_dir.join("README.md"), readme_content).map_err(|e| e.to_string())?;

    // artifacts.sha256
    let report_files = [
        "summary.json",
        "case-results.jsonl",
        "improvements.jsonl",
        "regressions.jsonl",
        "false-acceptances.jsonl",
        "README.md",
    ];
    let mut manifest_entries = Vec::new();
    for f in &report_files {
        let hash = calculate_file_sha256(stage_dir.join(f))?;
        manifest_entries.push(format!("{} data/reports/pack-comparison/{}", hash, f));
    }
    manifest_entries.sort();
    let manifest_bytes = manifest_entries.join("\n") + "\n";
    fs::write(stage_dir.join("artifacts.sha256"), manifest_bytes).map_err(|e| e.to_string())?;

    // Atomic Swap
    if backup_dir.exists() {
        remove_dir_or_file(&backup_dir).map_err(|e| e.to_string())?;
    }
    if out_dir.exists() {
        fs::rename(&out_dir, &backup_dir).map_err(|e| e.to_string())?;
    }

    match fs::rename(&stage_dir, &out_dir) {
        Ok(()) => {
            if backup_dir.exists() {
                let _ = remove_dir_or_file(&backup_dir);
            }
        }
        Err(err) => {
            if backup_dir.exists() {
                let _ = fs::rename(&backup_dir, &out_dir);
            }
            return Err(format!("Failed to install pack-comparison dir: {}", err));
        }
    }

    lock.release()?;
    Ok(summary)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateExperimentComparison {
    pub total_cases: usize,
    pub base_raw_pass_count: usize,
    pub candidate_raw_pass_count: usize,
    pub top_1_base: usize,
    pub top_1_cand: usize,
    pub top_3_base: usize,
    pub top_3_cand: usize,
    pub top_5_base: usize,
    pub top_5_cand: usize,
    pub mrr_base: f64,
    pub mrr_cand: f64,
    pub comp_recall_base: usize,
    pub comp_recall_cand: usize,
    pub kw_coverage_base: usize,
    pub kw_coverage_cand: usize,
    pub fa_rate_base: usize,
    pub fa_rate_cand: usize,
    pub total_eligible_correct_word: usize,
    pub total_eligible_complete_prefix: usize,
    pub total_eligible_accept_word: usize,
    pub total_eligible_false_acceptance: usize,
    pub improvements: Vec<(String, String)>,
    pub regressions: Vec<(String, String)>,
}

/// Evaluates a candidate experiment binary pack against a base binary pack using exact 357-case benchmark logic.
pub fn evaluate_candidate_experiment_pack<P: AsRef<Path>>(
    root_dir: P,
    base_binary_path: P,
    candidate_binary_path: P,
) -> Result<CandidateExperimentComparison, String> {
    let root = root_dir.as_ref();
    let base_p = base_binary_path.as_ref();
    let cand_p = candidate_binary_path.as_ref();

    let benchmark_path = root.join("evaluation/spelling/reviewed-cases.jsonl");
    let cases = super::reports::load_benchmark_cases(&benchmark_path)?;

    let base_bytes =
        fs::read(base_p).map_err(|e| format!("Failed to read base binary {:?}: {}", base_p, e))?;
    let cand_bytes = fs::read(cand_p)
        .map_err(|e| format!("Failed to read candidate binary {:?}: {}", cand_p, e))?;

    let mut base_engine = Engine::new();
    base_engine
        .load_binary_pack(&base_bytes)
        .map_err(|e| format!("Failed to load base engine: {}", e))?;

    let mut cand_engine = Engine::new();
    cand_engine
        .load_binary_pack(&cand_bytes)
        .map_err(|e| format!("Failed to load candidate engine: {}", e))?;

    let mut base_raw_pass = 0;
    let mut cand_raw_pass = 0;

    let mut top_1_b = 0;
    let mut top_1_c = 0;
    let mut top_3_b = 0;
    let mut top_3_c = 0;
    let mut top_5_b = 0;
    let mut top_5_c = 0;
    let mut mrr_b_sum = 0.0;
    let mut mrr_c_sum = 0.0;
    let mut comp_b = 0;
    let mut comp_c = 0;
    let mut kw_b = 0;
    let mut kw_c = 0;
    let mut fa_b = 0;
    let mut fa_c = 0;

    let mut el_cw = 0;
    let mut el_cp = 0;
    let mut el_kw = 0;
    let mut el_fa = 0;

    let mut improvements = Vec::new();
    let mut regressions = Vec::new();

    for case in &cases {
        let q_base = query_engine_for_case(&base_engine, case);
        let q_cand = query_engine_for_case(&cand_engine, case);

        let class = classify_pairwise_comparison(&q_base, &q_cand, &case.expectation, case.task);

        match class {
            PairwiseComparisonClass::Improvement => {
                improvements.push((case.case_id.clone(), case.input.clone()))
            }
            PairwiseComparisonClass::Regression => {
                regressions.push((case.case_id.clone(), case.input.clone()))
            }
            PairwiseComparisonClass::Unchanged => {}
        }

        // Raw Pass evaluation (matching PR #45 definition)
        let pass_b = match case.task {
            BenchmarkTask::AcceptWord => case.expectation.accepted == Some(q_base.accepted),
            BenchmarkTask::CorrectWord => q_base.best_expected_rank == Some(1),
            BenchmarkTask::CompletePrefix => q_base.best_expected_rank.is_some_and(|r| r <= 5),
        };

        let pass_c = match case.task {
            BenchmarkTask::AcceptWord => case.expectation.accepted == Some(q_cand.accepted),
            BenchmarkTask::CorrectWord => q_cand.best_expected_rank == Some(1),
            BenchmarkTask::CompletePrefix => q_cand.best_expected_rank.is_some_and(|r| r <= 5),
        };

        if pass_b {
            base_raw_pass += 1;
        }
        if pass_c {
            cand_raw_pass += 1;
        }

        match case.task {
            BenchmarkTask::CorrectWord => {
                if !case.expectation.expected_candidates.is_empty() {
                    el_cw += 1;
                    if q_base.best_expected_rank == Some(1) {
                        top_1_b += 1;
                    }
                    if q_cand.best_expected_rank == Some(1) {
                        top_1_c += 1;
                    }

                    if q_base.best_expected_rank.is_some_and(|r| r <= 3) {
                        top_3_b += 1;
                    }
                    if q_cand.best_expected_rank.is_some_and(|r| r <= 3) {
                        top_3_c += 1;
                    }

                    if q_base.best_expected_rank.is_some_and(|r| r <= 5) {
                        top_5_b += 1;
                    }
                    if q_cand.best_expected_rank.is_some_and(|r| r <= 5) {
                        top_5_c += 1;
                    }

                    if let Some(r) = q_base.best_expected_rank {
                        mrr_b_sum += 1.0 / (r as f64);
                    }
                    if let Some(r) = q_cand.best_expected_rank {
                        mrr_c_sum += 1.0 / (r as f64);
                    }
                }
            }
            BenchmarkTask::CompletePrefix => {
                if !case.expectation.expected_candidates.is_empty() {
                    el_cp += 1;
                    if q_base.best_expected_rank.is_some() {
                        comp_b += 1;
                    }
                    if q_cand.best_expected_rank.is_some() {
                        comp_c += 1;
                    }
                }
            }
            BenchmarkTask::AcceptWord => {
                if case.expectation.accepted == Some(true) {
                    el_kw += 1;
                    if q_base.accepted {
                        kw_b += 1;
                    }
                    if q_cand.accepted {
                        kw_c += 1;
                    }
                } else if case.expectation.accepted == Some(false) {
                    el_fa += 1;
                    if q_base.accepted {
                        fa_b += 1;
                    }
                    if q_cand.accepted {
                        fa_c += 1;
                    }
                }
            }
        }
    }

    let mrr_b = if el_cw == 0 {
        0.0
    } else {
        mrr_b_sum / (el_cw as f64)
    };
    let mrr_c = if el_cw == 0 {
        0.0
    } else {
        mrr_c_sum / (el_cw as f64)
    };

    Ok(CandidateExperimentComparison {
        total_cases: cases.len(),
        base_raw_pass_count: base_raw_pass,
        candidate_raw_pass_count: cand_raw_pass,
        top_1_base: top_1_b,
        top_1_cand: top_1_c,
        top_3_base: top_3_b,
        top_3_cand: top_3_c,
        top_5_base: top_5_b,
        top_5_cand: top_5_c,
        mrr_base: mrr_b,
        mrr_cand: mrr_c,
        comp_recall_base: comp_b,
        comp_recall_cand: comp_c,
        kw_coverage_base: kw_b,
        kw_coverage_cand: kw_c,
        fa_rate_base: fa_b,
        fa_rate_cand: fa_c,
        total_eligible_correct_word: el_cw,
        total_eligible_complete_prefix: el_cp,
        total_eligible_accept_word: el_kw,
        total_eligible_false_acceptance: el_fa,
        improvements,
        regressions,
    })
}
