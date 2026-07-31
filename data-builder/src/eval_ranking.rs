//! Ranking Evaluation module for comparing baseline vs frequency-aware candidate suggestion ranking.

use kurmanci_engine::{Engine, RankingConfig};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// A single evaluation case in `evaluation/spelling/cases.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub input: String,
    pub expected: Vec<String>,
    pub category: String,
    #[serde(default)]
    pub notes: String,
}

/// Evaluation result for a single case under a ranking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCaseResult {
    pub input: String,
    pub expected: Vec<String>,
    pub category: String,
    pub top_1_matched: bool,
    pub top_3_matched: bool,
    pub top_5_matched: bool,
    pub reciprocal_rank: f64,
    pub returned_candidates: Vec<String>,
}

/// Summary report emitted by `evaluate-ranking`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingEvalSummaryReport {
    pub total_cases: usize,
    pub baseline_top_1_accuracy: f64,
    pub experiment_top_1_accuracy: f64,
    pub baseline_top_3_accuracy: f64,
    pub experiment_top_3_accuracy: f64,
    pub baseline_top_5_accuracy: f64,
    pub experiment_top_5_accuracy: f64,
    pub baseline_mrr: f64,
    pub experiment_mrr: f64,
    pub baseline_no_candidate_count: usize,
    pub experiment_no_candidate_count: usize,
    pub improved_cases_count: usize,
    pub regressed_cases_count: usize,
    pub unchanged_cases_count: usize,
    pub acceptance_passed: bool,
}

/// Runs the evaluation benchmark for baseline vs frequency-aware ranking.
pub fn run_ranking_evaluation<P: AsRef<Path>>(
    root_dir: P,
) -> Result<RankingEvalSummaryReport, String> {
    let root = root_dir.as_ref();
    let cases_path = root.join("evaluation/spelling/cases.jsonl");
    let pack_path = root.join("data/build/lexicon.bin");

    if !cases_path.exists() {
        return Err(format!("Evaluation dataset missing at {:?}", cases_path));
    }
    if !pack_path.exists() {
        return Err(format!("Compiled binary pack missing at {:?}", pack_path));
    }

    let pack_bytes = fs::read(&pack_path)
        .map_err(|e| format!("Failed to read binary pack {:?}: {}", pack_path, e))?;

    let mut engine = Engine::new();
    engine
        .load_binary_pack(&pack_bytes)
        .map_err(|e| format!("Failed to load binary pack: {}", e))?;

    // Read cases
    let file =
        File::open(&cases_path).map_err(|e| format!("Failed to open {:?}: {}", cases_path, e))?;
    let reader = BufReader::new(file);

    let mut cases = Vec::new();
    for (line_idx, line_res) in reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("Read error on line {}: {}", line_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let case: EvalCase = serde_json::from_str(&line)
            .map_err(|e| format!("Line {}: invalid JSON in cases.jsonl: {}", line_idx + 1, e))?;
        cases.push(case);
    }

    if cases.is_empty() {
        return Err("Evaluation dataset is empty".to_string());
    }

    let baseline_config = RankingConfig::disabled();
    let experiment_config = RankingConfig::default();

    let mut baseline_results = Vec::new();
    let mut experiment_results = Vec::new();
    let mut changed_rankings = Vec::new();
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();

    for case in &cases {
        let b_sugs = engine.suggest_with_config(&case.input, 5, &baseline_config);
        let e_sugs = engine.suggest_with_config(&case.input, 5, &experiment_config);

        let b_res = evaluate_single_case(case, &b_sugs);
        let e_res = evaluate_single_case(case, &e_sugs);

        if b_res.returned_candidates != e_res.returned_candidates {
            changed_rankings.push(serde_json::json!({
                "input": case.input,
                "category": case.category,
                "expected": case.expected,
                "baseline_candidates": b_res.returned_candidates,
                "frequency_candidates": e_res.returned_candidates,
            }));
        }

        if e_res.reciprocal_rank > b_res.reciprocal_rank {
            improvements.push(serde_json::json!({
                "input": case.input,
                "category": case.category,
                "expected": case.expected,
                "baseline": b_res.returned_candidates,
                "frequency": e_res.returned_candidates,
            }));
        } else if e_res.reciprocal_rank < b_res.reciprocal_rank {
            regressions.push(serde_json::json!({
                "input": case.input,
                "category": case.category,
                "expected": case.expected,
                "baseline": b_res.returned_candidates,
                "frequency": e_res.returned_candidates,
            }));
        }

        baseline_results.push(b_res);
        experiment_results.push(e_res);
    }

    let total_cases = cases.len();
    let b_top1 = baseline_results.iter().filter(|r| r.top_1_matched).count();
    let e_top1 = experiment_results
        .iter()
        .filter(|r| r.top_1_matched)
        .count();

    let b_top3 = baseline_results.iter().filter(|r| r.top_3_matched).count();
    let e_top3 = experiment_results
        .iter()
        .filter(|r| r.top_3_matched)
        .count();

    let b_top5 = baseline_results.iter().filter(|r| r.top_5_matched).count();
    let e_top5 = experiment_results
        .iter()
        .filter(|r| r.top_5_matched)
        .count();

    let b_mrr: f64 = baseline_results
        .iter()
        .map(|r| r.reciprocal_rank)
        .sum::<f64>()
        / total_cases as f64;
    let e_mrr: f64 = experiment_results
        .iter()
        .map(|r| r.reciprocal_rank)
        .sum::<f64>()
        / total_cases as f64;

    let b_no_cand = baseline_results
        .iter()
        .filter(|r| r.returned_candidates.is_empty())
        .count();
    let e_no_cand = experiment_results
        .iter()
        .filter(|r| r.returned_candidates.is_empty())
        .count();

    let exact_cases_passed = experiment_results
        .iter()
        .filter(|result| result.category == "exact-word-preservation")
        .all(|result| {
            result.top_1_matched
                && result
                    .returned_candidates
                    .first()
                    .map(|first| {
                        kurmanci_engine::normalize(first)
                            == kurmanci_engine::normalize(&result.input)
                    })
                    .unwrap_or(false)
        });

    let acceptance_passed =
        e_top1 >= b_top1 && e_top3 >= b_top3 && regressions.is_empty() && exact_cases_passed;

    let summary = RankingEvalSummaryReport {
        total_cases,
        baseline_top_1_accuracy: ((b_top1 as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        experiment_top_1_accuracy: ((e_top1 as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        baseline_top_3_accuracy: ((b_top3 as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        experiment_top_3_accuracy: ((e_top3 as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        baseline_top_5_accuracy: ((b_top5 as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        experiment_top_5_accuracy: ((e_top5 as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        baseline_mrr: (b_mrr * 10000.0).round() / 10000.0,
        experiment_mrr: (e_mrr * 10000.0).round() / 10000.0,
        baseline_no_candidate_count: b_no_cand,
        experiment_no_candidate_count: e_no_cand,
        improved_cases_count: improvements.len(),
        regressed_cases_count: regressions.len(),
        unchanged_cases_count: total_cases - (improvements.len() + regressions.len()),
        acceptance_passed,
    };

    // Staged write to data/reports/ranking-evaluation/
    write_eval_reports(
        root,
        &summary,
        &baseline_results,
        &experiment_results,
        &changed_rankings,
        &regressions,
        &improvements,
    )?;

    Ok(summary)
}

fn evaluate_single_case(
    case: &EvalCase,
    suggestions: &[kurmanci_engine::Suggestion],
) -> EvalCaseResult {
    let returned: Vec<String> = suggestions.iter().map(|s| s.text.clone()).collect();

    let mut reciprocal_rank = 0.0;
    let mut top_1_matched = false;
    let mut top_3_matched = false;
    let mut top_5_matched = false;

    for (idx, text) in returned.iter().enumerate() {
        if case.expected.contains(text) {
            if idx == 0 {
                top_1_matched = true;
            }
            if idx < 3 {
                top_3_matched = true;
            }
            if idx < 5 {
                top_5_matched = true;
            }
            if reciprocal_rank == 0.0 {
                reciprocal_rank = 1.0 / (idx + 1) as f64;
            }
        }
    }

    EvalCaseResult {
        input: case.input.clone(),
        expected: case.expected.clone(),
        category: case.category.clone(),
        top_1_matched,
        top_3_matched,
        top_5_matched,
        reciprocal_rank,
        returned_candidates: returned,
    }
}

fn write_eval_reports(
    root: &Path,
    summary: &RankingEvalSummaryReport,
    baseline_results: &[EvalCaseResult],
    experiment_results: &[EvalCaseResult],
    changed_rankings: &[serde_json::Value],
    regressions: &[serde_json::Value],
    improvements: &[serde_json::Value],
) -> Result<(), String> {
    let output_dir = root.join("data/reports/ranking-evaluation");
    let stage_dir = output_dir.with_extension(format!(
        "tmp_stage_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));

    if stage_dir.exists() {
        let _ = fs::remove_dir_all(&stage_dir);
    }
    fs::create_dir_all(&stage_dir).map_err(|e| {
        format!(
            "Failed to create stage eval report dir {:?}: {}",
            stage_dir, e
        )
    })?;

    // 1. summary.json
    let json = serde_json::to_string_pretty(summary)
        .map_err(|e| format!("Failed to serialize summary report: {}", e))?;
    fs::write(stage_dir.join("summary.json"), json)
        .map_err(|e| format!("Failed to write summary.json: {}", e))?;

    // 2. baseline-results.jsonl
    write_jsonl(&stage_dir.join("baseline-results.jsonl"), baseline_results)?;

    // 3. frequency-results.jsonl
    write_jsonl(
        &stage_dir.join("frequency-results.jsonl"),
        experiment_results,
    )?;

    // 4. changed-rankings.jsonl
    write_jsonl(&stage_dir.join("changed-rankings.jsonl"), changed_rankings)?;

    // 5. regressions.jsonl
    write_jsonl(&stage_dir.join("regressions.jsonl"), regressions)?;

    // 6. improvements.jsonl
    write_jsonl(&stage_dir.join("improvements.jsonl"), improvements)?;

    // 7. README.md
    let readme_content = format!(
        r#"# Kurmancî Ranking Evaluation Report

- **Total Cases**: {}
- **Baseline Top-1 Accuracy**: {:.2}%
- **Frequency Top-1 Accuracy**: {:.2}%
- **Baseline Top-3 Accuracy**: {:.2}%
- **Frequency Top-3 Accuracy**: {:.2}%
- **Baseline MRR**: {:.4}
- **Frequency MRR**: {:.4}
- **Improved Cases**: {}
- **Regressed Cases**: {}
- **Unchanged Cases**: {}
- **Acceptance Passed**: {}

## Determinism & Manifest
Generated artifacts are 100% reproducible and recorded in `artifacts.sha256`.
"#,
        summary.total_cases,
        summary.baseline_top_1_accuracy,
        summary.experiment_top_1_accuracy,
        summary.baseline_top_3_accuracy,
        summary.experiment_top_3_accuracy,
        summary.baseline_mrr,
        summary.experiment_mrr,
        summary.improved_cases_count,
        summary.regressed_cases_count,
        summary.unchanged_cases_count,
        summary.acceptance_passed
    );
    fs::write(stage_dir.join("README.md"), readme_content)
        .map_err(|e| format!("Failed to write README.md: {}", e))?;

    // 8. artifacts.sha256
    let report_files = [
        "summary.json",
        "baseline-results.jsonl",
        "frequency-results.jsonl",
        "changed-rankings.jsonl",
        "regressions.jsonl",
        "improvements.jsonl",
        "README.md",
    ];

    let mut manifest_content = String::new();
    for file in &report_files {
        let content = fs::read(stage_dir.join(file))
            .map_err(|e| format!("Failed to read report file {} for manifest: {}", file, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        manifest_content.push_str(&format!(
            "{}  data/reports/ranking-evaluation/{}\n",
            hash, file
        ));
    }
    fs::write(stage_dir.join("artifacts.sha256"), manifest_content)
        .map_err(|e| format!("Failed to write artifacts.sha256 manifest: {}", e))?;

    // Atomic backup and rollback swap
    let backup_dir = output_dir.with_extension(format!(
        "tmp_backup_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));

    if backup_dir.exists() {
        let _ = fs::remove_dir_all(&backup_dir);
    }

    if output_dir.exists() {
        fs::rename(&output_dir, &backup_dir).map_err(|e| {
            format!(
                "Failed to move output dir {:?} to backup: {}",
                output_dir, e
            )
        })?;
    }

    match fs::rename(&stage_dir, &output_dir) {
        Ok(()) => {
            if backup_dir.exists() {
                let _ = fs::remove_dir_all(&backup_dir);
            }
            Ok(())
        }
        Err(err) => {
            if backup_dir.exists() {
                let _ = fs::rename(&backup_dir, &output_dir);
            }
            Err(format!(
                "Failed to install ranking evaluation reports: {}",
                err
            ))
        }
    }
}

fn write_jsonl<T: Serialize>(path: &Path, items: &[T]) -> Result<(), String> {
    let mut file = File::create(path).map_err(|e| format!("Failed to create {:?}: {}", path, e))?;
    for item in items {
        let line =
            serde_json::to_string(item).map_err(|e| format!("Serialization error: {}", e))?;
        writeln!(file, "{}", line).map_err(|e| format!("Write error to {:?}: {}", path, e))?;
    }
    Ok(())
}
