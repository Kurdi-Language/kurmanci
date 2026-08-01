//! Next-Word Prediction Evaluation module for benchmarking engine predict_next performance.

use kurmanci_engine::{Engine, NextWordPrediction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// A single evaluation case in `evaluation/next-word/cases.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextWordEvalCase {
    pub context: String,
    pub expected: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
    pub category: String,
    #[serde(default)]
    pub notes: String,
}

/// Result for a single next-word evaluation case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextWordEvalCaseResult {
    pub context: String,
    pub expected: Vec<String>,
    pub category: String,
    pub top_1_matched: bool,
    pub top_3_matched: bool,
    pub top_5_matched: bool,
    pub reciprocal_rank: f64,
    pub returned_predictions: Vec<String>,
    pub canonical_order_passed: bool,
}

/// Summary report emitted by `evaluate-next-word`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextWordEvalSummaryReport {
    pub total_cases: usize,
    pub overall_case_count: usize,
    pub positive_case_count: usize,
    pub top_1_accuracy: f64,
    pub top_3_accuracy: f64,
    pub top_5_accuracy: f64,
    pub mean_reciprocal_rank: f64,
    pub positive_top_1_accuracy: f64,
    pub positive_top_3_accuracy: f64,
    pub positive_top_5_accuracy: f64,
    pub positive_mean_reciprocal_rank: f64,
    pub unknown_context_count: usize,
    pub no_prediction_count: usize,
    pub mandatory_categories_passed: bool,
    pub sentence_boundary_passed: bool,
    pub unknown_context_passed: bool,
    pub canonical_ordering_passed: bool,
    pub pipeline_validation_passed: bool,
    pub model_quality_passed: bool,
    pub acceptance_passed: bool,
}

/// Runs the next-word prediction evaluation benchmark.
pub fn run_next_word_evaluation<P: AsRef<Path>>(
    root_dir: P,
) -> Result<NextWordEvalSummaryReport, String> {
    let root = root_dir.as_ref();
    let cases_path = root.join("evaluation/next-word/cases.jsonl");
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
        .map_err(|e| format!("Failed to load binary pack: {e}"))?;

    let file =
        File::open(&cases_path).map_err(|e| format!("Failed to open {:?}: {}", cases_path, e))?;
    let reader = BufReader::new(file);

    let mut cases = Vec::new();
    let mut observed_categories = BTreeSet::new();

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("Read error on line {}: {}", line_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let case: NextWordEvalCase = serde_json::from_str(&line)
            .map_err(|e| format!("Line {}: invalid JSON in cases.jsonl: {}", line_idx + 1, e))?;
        observed_categories.insert(case.category.clone());
        cases.push(case);
    }

    if cases.is_empty() {
        return Err("Next-word evaluation dataset is empty".to_string());
    }

    let mandatory_categories = [
        "common-context",
        "unknown-context",
        "sentence-boundary",
        "tie-breaking",
    ];
    let mandatory_categories_passed = mandatory_categories
        .iter()
        .all(|cat| observed_categories.contains(*cat));

    let mut results = Vec::new();
    let mut failures = Vec::new();

    let mut unknown_context_count = 0usize;
    let mut no_prediction_count = 0usize;
    let mut sentence_boundary_passed = true;
    let mut unknown_context_passed = true;
    let mut canonical_ordering_passed = true;

    for case in &cases {
        let preds = engine.predict_next(&case.context, 5);

        if case.category == "unknown-context" {
            unknown_context_count += 1;
            if !preds.is_empty() {
                unknown_context_passed = false;
            }
        }

        if preds.is_empty() {
            no_prediction_count += 1;
        }

        if case.category == "sentence-boundary" {
            let returned_words: Vec<String> = preds.iter().map(|p| p.word.clone()).collect();
            for forbidden in &case.forbidden {
                if returned_words.contains(forbidden) {
                    sentence_boundary_passed = false;
                }
            }
        }

        // Check canonical ordering: prob DESC, count DESC, word ASC
        let mut sorted_check = preds.clone();
        sorted_check.sort_by(|a, b| {
            b.probability_millionths
                .cmp(&a.probability_millionths)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.word.cmp(&b.word))
        });
        let order_passed = preds == sorted_check;
        if !order_passed {
            canonical_ordering_passed = false;
        }

        let case_res = evaluate_single_next_word_case(case, &preds, order_passed);

        if !case.expected.is_empty() && !case_res.top_1_matched {
            failures.push(serde_json::json!({
                "context": case.context,
                "category": case.category,
                "expected": case.expected,
                "returned": case_res.returned_predictions,
            }));
        }

        results.push(case_res);
    }

    let total_cases = cases.len();
    let top1_count = results.iter().filter(|r| r.top_1_matched).count();
    let top3_count = results.iter().filter(|r| r.top_3_matched).count();
    let top5_count = results.iter().filter(|r| r.top_5_matched).count();

    let mrr_sum: f64 = results.iter().map(|r| r.reciprocal_rank).sum();

    let positive_results: Vec<_> = results.iter().filter(|r| !r.expected.is_empty()).collect();
    let positive_case_count = positive_results.len();

    let (pos_top1_count, pos_top3_count, pos_top5_count, pos_mrr_sum) = if positive_case_count > 0 {
        let t1 = positive_results.iter().filter(|r| r.top_1_matched).count();
        let t3 = positive_results.iter().filter(|r| r.top_3_matched).count();
        let t5 = positive_results.iter().filter(|r| r.top_5_matched).count();
        let mrr: f64 = positive_results.iter().map(|r| r.reciprocal_rank).sum();
        (t1, t3, t5, mrr)
    } else {
        (0, 0, 0, 0.0)
    };

    let model_quality_passed =
        !positive_results.is_empty() && positive_results.iter().any(|r| r.top_5_matched);

    let pipeline_validation_passed = mandatory_categories_passed
        && sentence_boundary_passed
        && unknown_context_passed
        && canonical_ordering_passed;

    let acceptance_passed = pipeline_validation_passed && model_quality_passed;

    let pos_denom = if positive_case_count > 0 {
        positive_case_count as f64
    } else {
        1.0
    };

    let summary = NextWordEvalSummaryReport {
        total_cases,
        overall_case_count: total_cases,
        positive_case_count,
        top_1_accuracy: ((top1_count as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        top_3_accuracy: ((top3_count as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        top_5_accuracy: ((top5_count as f64 / total_cases as f64) * 10000.0).round() / 100.0,
        mean_reciprocal_rank: ((mrr_sum / total_cases as f64) * 10000.0).round() / 10000.0,
        positive_top_1_accuracy: ((pos_top1_count as f64 / pos_denom) * 10000.0).round() / 100.0,
        positive_top_3_accuracy: ((pos_top3_count as f64 / pos_denom) * 10000.0).round() / 100.0,
        positive_top_5_accuracy: ((pos_top5_count as f64 / pos_denom) * 10000.0).round() / 100.0,
        positive_mean_reciprocal_rank: ((pos_mrr_sum / pos_denom) * 10000.0).round() / 10000.0,
        unknown_context_count,
        no_prediction_count,
        mandatory_categories_passed,
        sentence_boundary_passed,
        unknown_context_passed,
        canonical_ordering_passed,
        pipeline_validation_passed,
        model_quality_passed,
        acceptance_passed,
    };

    write_next_word_eval_reports(root, &summary, &results, &failures)?;

    Ok(summary)
}

fn evaluate_single_next_word_case(
    case: &NextWordEvalCase,
    predictions: &[NextWordPrediction],
    canonical_order_passed: bool,
) -> NextWordEvalCaseResult {
    let returned: Vec<String> = predictions.iter().map(|p| p.word.clone()).collect();

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

    NextWordEvalCaseResult {
        context: case.context.clone(),
        expected: case.expected.clone(),
        category: case.category.clone(),
        top_1_matched,
        top_3_matched,
        top_5_matched,
        reciprocal_rank,
        returned_predictions: returned,
        canonical_order_passed,
    }
}

fn write_next_word_eval_reports(
    root: &Path,
    summary: &NextWordEvalSummaryReport,
    results: &[NextWordEvalCaseResult],
    failures: &[serde_json::Value],
) -> Result<(), String> {
    let output_dir = root.join("data/reports/next-word-evaluation");
    let stage_dir = output_dir.with_extension(format!(
        "tmp_stage_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));

    fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("Failed to create stage report dir {:?}: {}", stage_dir, e))?;

    // 1. summary.json
    let summary_json = serde_json::to_string_pretty(summary)
        .map_err(|e| format!("Failed to serialize summary report: {}", e))?;
    fs::write(stage_dir.join("summary.json"), summary_json)
        .map_err(|e| format!("Failed to write summary.json: {}", e))?;

    // 2. results.jsonl
    let results_path = stage_dir.join("results.jsonl");
    let mut results_file = File::create(&results_path)
        .map_err(|e| format!("Failed to create results.jsonl: {}", e))?;
    for res in results {
        let line = serde_json::to_string(res)
            .map_err(|e| format!("Failed to serialize eval result: {}", e))?;
        writeln!(results_file, "{}", line)
            .map_err(|e| format!("Failed to write results.jsonl: {}", e))?;
    }

    // 3. failures.jsonl
    let failures_path = stage_dir.join("failures.jsonl");
    let mut failures_file = File::create(&failures_path)
        .map_err(|e| format!("Failed to create failures.jsonl: {}", e))?;
    for fail in failures {
        let line = serde_json::to_string(fail)
            .map_err(|e| format!("Failed to serialize failure: {}", e))?;
        writeln!(failures_file, "{}", line)
            .map_err(|e| format!("Failed to write failures.jsonl: {}", e))?;
    }

    // 4. README.md
    let readme = format!(
        r#"# Kurmancî Next-Word Prediction Evaluation Report

- **Overall Case Count**: {}
- **Positive Case Count**: {}
- **Positive Top-1 Accuracy**: {:.2}%
- **Positive Top-3 Accuracy**: {:.2}%
- **Positive Top-5 Accuracy**: {:.2}%
- **Positive Mean Reciprocal Rank (MRR)**: {:.4}
- **Overall Top-1 Accuracy**: {:.2}%
- **Overall Mean Reciprocal Rank (MRR)**: {:.4}
- **Unknown Context Cases**: {}
- **No-Prediction Cases**: {}
- **Acceptance Passed**: {}

## Acceptance Criteria Status
- **Pipeline Validation Passed**: {}
- **Model Quality Passed**: {}
- **Mandatory Categories Present**: {}
- **Sentence Boundary Isolation**: {}
- **Unknown Context Policy**: {}
- **Canonical Deterministic Ordering**: {}
"#,
        summary.overall_case_count,
        summary.positive_case_count,
        summary.positive_top_1_accuracy,
        summary.positive_top_3_accuracy,
        summary.positive_top_5_accuracy,
        summary.positive_mean_reciprocal_rank,
        summary.top_1_accuracy,
        summary.mean_reciprocal_rank,
        summary.unknown_context_count,
        summary.no_prediction_count,
        summary.acceptance_passed,
        summary.pipeline_validation_passed,
        summary.model_quality_passed,
        summary.mandatory_categories_passed,
        summary.sentence_boundary_passed,
        summary.unknown_context_passed,
        summary.canonical_ordering_passed,
    );
    fs::write(stage_dir.join("README.md"), readme)
        .map_err(|e| format!("Failed to write README.md: {}", e))?;

    // 5. artifacts.sha256 manifest
    let report_files = [
        "summary.json",
        "results.jsonl",
        "failures.jsonl",
        "README.md",
    ];
    let mut manifest_content = String::new();
    for file in &report_files {
        let content = fs::read(stage_dir.join(file))
            .map_err(|e| format!("Failed to read report file {} for manifest: {}", file, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        manifest_content.push_str(&format!(
            "{}  data/reports/next-word-evaluation/{}\n",
            hash, file
        ));
    }
    fs::write(stage_dir.join("artifacts.sha256"), manifest_content)
        .map_err(|e| format!("Failed to write artifacts.sha256: {}", e))?;

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
            Err(format!("Failed to install next-word eval reports: {}", err))
        }
    }
}
