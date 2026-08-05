//! Next-Word & Two-Word Context Prediction Evaluation module.

use kurmanci_engine::{Engine, PredictionSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// A single evaluation case in `evaluation/next-word/trigram-cases.jsonl` or `cases.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPredictionEvalCase {
    #[serde(default)]
    pub context: Vec<String>,
    #[serde(default)]
    pub expected: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
    pub category: String,
    #[serde(default)]
    pub expected_source: String,
    #[serde(default)]
    pub notes: String,
}

/// Result for a single context prediction evaluation case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPredictionEvalCaseResult {
    pub context: Vec<String>,
    pub expected: Vec<String>,
    pub category: String,
    pub expected_source: String,
    pub actual_source: String,
    pub source_matched: bool,
    pub top_1_matched: bool,
    pub top_3_matched: bool,
    pub top_5_matched: bool,
    pub reciprocal_rank: f64,
    pub returned_predictions: Vec<String>,
    pub canonical_order_passed: bool,
    pub forbidden_passed: bool,
}

/// Baseline bigram-only performance result for positive cases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineEvalSummary {
    pub positive_top_1_accuracy: f64,
    pub positive_top_3_accuracy: f64,
    pub positive_top_5_accuracy: f64,
    pub positive_mrr: f64,
}

/// Summary report emitted by `evaluate-next-word`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextWordEvalSummaryReport {
    pub total_cases: usize,
    pub overall_case_count: usize,
    pub positive_case_count: usize,
    pub trigram_hit_count: usize,
    pub bigram_backoff_count: usize,
    pub unknown_context_count: usize,
    pub top_1_accuracy: f64,
    pub top_3_accuracy: f64,
    pub top_5_accuracy: f64,
    pub mean_reciprocal_rank: f64,
    pub positive_top_1_accuracy: f64,
    pub positive_top_3_accuracy: f64,
    pub positive_top_5_accuracy: f64,
    pub positive_mean_reciprocal_rank: f64,
    pub source_selection_accuracy: f64,
    pub baseline_bigram_top_3_accuracy: f64,
    pub mandatory_categories_passed: bool,
    pub sentence_boundary_passed: bool,
    pub unknown_context_passed: bool,
    pub canonical_ordering_passed: bool,
    pub source_selection_passed: bool,
    pub pipeline_validation_passed: bool,
    pub model_quality_passed: bool,
    pub acceptance_passed: bool,
}

/// Runs the next-word & context prediction evaluation benchmark.
pub fn run_next_word_evaluation<P: AsRef<Path>>(
    root_dir: P,
) -> Result<NextWordEvalSummaryReport, String> {
    let root = root_dir.as_ref();
    let trigram_cases_path = root.join("evaluation/next-word/trigram-cases.jsonl");
    let bigram_cases_path = root.join("evaluation/next-word/cases.jsonl");
    let pack_path = root.join("data/build/lexicon.bin");

    let cases_path = if trigram_cases_path.exists() {
        trigram_cases_path
    } else if bigram_cases_path.exists() {
        bigram_cases_path
    } else {
        return Err("No evaluation dataset found in evaluation/next-word/".to_string());
    };

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

    let mut raw_cases = Vec::new();
    let mut observed_categories = BTreeSet::new();

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("Read error on line {}: {}", line_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }

        // Support string or array context representations seamlessly
        if let Ok(case) = serde_json::from_str::<ContextPredictionEvalCase>(&line) {
            observed_categories.insert(case.category.clone());
            raw_cases.push(case);
        } else if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
            let ctx = if let Some(arr) = val.get("context").and_then(|v| v.as_array()) {
                arr.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            } else if let Some(s) = val.get("context").and_then(|v| v.as_str()) {
                vec![s.to_string()]
            } else {
                vec![]
            };

            let expected = val
                .get("expected")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let forbidden = val
                .get("forbidden")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let category = val
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let expected_source = val
                .get("expected_source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let notes = val
                .get("notes")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            observed_categories.insert(category.clone());
            raw_cases.push(ContextPredictionEvalCase {
                context: ctx,
                expected,
                forbidden,
                category,
                expected_source,
                notes,
            });
        }
    }

    let mandatory_categories = vec![
        "trigram-hit",
        "trigram-preferred",
        "bigram-backoff",
        "unknown-context",
        "sentence-boundary",
        "document-boundary",
        "deterministic-tie",
    ];

    let mut mandatory_categories_passed = true;
    for cat in &mandatory_categories {
        if !observed_categories.contains(*cat) {
            mandatory_categories_passed = false;
        }
    }

    let mut results = Vec::new();
    let mut failures = Vec::new();
    let mut source_selection_errors = Vec::new();

    let mut trigram_hit_count = 0;
    let mut bigram_backoff_count = 0;
    let mut unknown_context_count = 0;
    let mut source_selection_matches = 0;

    let mut total_top_1 = 0;
    let mut total_top_3 = 0;
    let mut total_top_5 = 0;
    let mut total_mrr = 0.0;

    let mut pos_top_1 = 0;
    let mut pos_top_3 = 0;
    let mut pos_top_5 = 0;
    let mut pos_mrr = 0.0;
    let mut positive_case_count = 0;

    let mut baseline_pos_top_3 = 0;

    let mut sentence_boundary_passed = true;
    let mut unknown_context_passed = true;
    let mut canonical_ordering_passed = true;

    for case in &raw_cases {
        let (preds, actual_source) = if case.context.len() == 2 {
            let res = engine.predict_next_with_context(&case.context[0], &case.context[1], 5);
            let src_str = match res.source {
                Some(PredictionSource::Trigram) => "trigram",
                Some(PredictionSource::BigramBackoff) => "bigram-backoff",
                Some(PredictionSource::Bigram) => "bigram",
                Some(PredictionSource::None) | None => "none",
            };
            (res.predictions, src_str.to_string())
        } else if case.context.len() == 1 {
            let res = engine.predict_next(&case.context[0], 5);
            (res, "bigram".to_string())
        } else {
            (vec![], "none".to_string())
        };

        if actual_source == "trigram" {
            trigram_hit_count += 1;
        } else if actual_source == "bigram-backoff" {
            bigram_backoff_count += 1;
        } else if actual_source == "none" {
            unknown_context_count += 1;
        }

        let source_matched = if case.expected_source.is_empty() {
            true
        } else {
            case.expected_source == actual_source
        };

        if source_matched {
            source_selection_matches += 1;
        } else {
            source_selection_errors.push(case.clone());
        }

        let returned_words: Vec<String> = preds.iter().map(|p| p.word.clone()).collect();

        // Check forbidden predictions
        let mut forbidden_passed = true;
        for f in &case.forbidden {
            if returned_words.contains(f) {
                forbidden_passed = false;
                if case.category == "sentence-boundary" || case.category == "document-boundary" {
                    sentence_boundary_passed = false;
                }
            }
        }

        if case.category == "unknown-context" && !returned_words.is_empty() {
            unknown_context_passed = false;
        }

        // Check canonical ordering (prob DESC, count DESC, word ASC)
        let mut is_canon = true;
        if preds.len() > 1 {
            for i in 0..(preds.len() - 1) {
                let p1 = &preds[i];
                let p2 = &preds[i + 1];
                if p1.probability_millionths < p2.probability_millionths
                    || (p1.probability_millionths == p2.probability_millionths
                        && (p1.count < p2.count || (p1.count == p2.count && p1.word >= p2.word)))
                {
                    is_canon = false;
                }
            }
        }
        if !is_canon {
            canonical_ordering_passed = false;
        }

        let top_1_matched = !case.expected.is_empty()
            && returned_words
                .first()
                .is_some_and(|w| case.expected.contains(w));
        let top_3_matched = !case.expected.is_empty()
            && returned_words
                .iter()
                .take(3)
                .any(|w| case.expected.contains(w));
        let top_5_matched = !case.expected.is_empty()
            && returned_words
                .iter()
                .take(5)
                .any(|w| case.expected.contains(w));

        let mut rr = 0.0;
        if !case.expected.is_empty() {
            for (rank, w) in returned_words.iter().enumerate() {
                if case.expected.contains(w) {
                    rr = 1.0 / ((rank + 1) as f64);
                    break;
                }
            }
        }

        if top_1_matched {
            total_top_1 += 1;
        }
        if top_3_matched {
            total_top_3 += 1;
        }
        if top_5_matched {
            total_top_5 += 1;
        }
        total_mrr += rr;

        if !case.expected.is_empty() {
            positive_case_count += 1;
            if top_1_matched {
                pos_top_1 += 1;
            }
            if top_3_matched {
                pos_top_3 += 1;
            }
            if top_5_matched {
                pos_top_5 += 1;
            }
            pos_mrr += rr;

            // Calculate dynamic bigram-only baseline for positive cases
            let prev1 = case.context.last().map(|s| s.as_str()).unwrap_or("");
            let baseline_preds = engine.predict_next(prev1, 5);
            let baseline_words: Vec<String> = baseline_preds.into_iter().map(|p| p.word).collect();
            if baseline_words
                .iter()
                .take(3)
                .any(|w| case.expected.contains(w))
            {
                baseline_pos_top_3 += 1;
            }
        }

        let res = ContextPredictionEvalCaseResult {
            context: case.context.clone(),
            expected: case.expected.clone(),
            category: case.category.clone(),
            expected_source: case.expected_source.clone(),
            actual_source,
            source_matched,
            top_1_matched,
            top_3_matched,
            top_5_matched,
            reciprocal_rank: rr,
            returned_predictions: returned_words,
            canonical_order_passed: is_canon,
            forbidden_passed,
        };

        if !top_5_matched && !case.expected.is_empty() {
            failures.push(res.clone());
        }
        results.push(res);
    }

    let n_total = raw_cases.len();
    let n_pos = positive_case_count.max(1) as f64;

    let overall_t1 = (total_top_1 as f64 / n_total as f64) * 100.0;
    let overall_t3 = (total_top_3 as f64 / n_total as f64) * 100.0;
    let overall_t5 = (total_top_5 as f64 / n_total as f64) * 100.0;
    let overall_mrr = total_mrr / n_total as f64;

    let pos_t1 = (pos_top_1 as f64 / n_pos) * 100.0;
    let pos_t3 = (pos_top_3 as f64 / n_pos) * 100.0;
    let pos_t5 = (pos_top_5 as f64 / n_pos) * 100.0;
    let pos_mrr_val = pos_mrr / n_pos;

    let source_acc = (source_selection_matches as f64 / n_total as f64) * 100.0;
    let baseline_t3 = (baseline_pos_top_3 as f64 / n_pos) * 100.0;

    let source_selection_passed = source_selection_matches == n_total;

    let pipeline_validation_passed = mandatory_categories_passed
        && sentence_boundary_passed
        && unknown_context_passed
        && canonical_ordering_passed
        && source_selection_passed;

    let model_quality_passed = positive_case_count > 0
        && pos_t3 >= baseline_t3
        && results
            .iter()
            .filter(|r| !r.expected.is_empty())
            .any(|r| r.top_5_matched);

    let acceptance_passed = pipeline_validation_passed && model_quality_passed;

    let summary = NextWordEvalSummaryReport {
        total_cases: n_total,
        overall_case_count: n_total,
        positive_case_count,
        trigram_hit_count,
        bigram_backoff_count,
        unknown_context_count,
        top_1_accuracy: overall_t1,
        top_3_accuracy: overall_t3,
        top_5_accuracy: overall_t5,
        mean_reciprocal_rank: overall_mrr,
        positive_top_1_accuracy: pos_t1,
        positive_top_3_accuracy: pos_t3,
        positive_top_5_accuracy: pos_t5,
        positive_mean_reciprocal_rank: pos_mrr_val,
        source_selection_accuracy: source_acc,
        baseline_bigram_top_3_accuracy: baseline_t3,
        mandatory_categories_passed,
        sentence_boundary_passed,
        unknown_context_passed,
        canonical_ordering_passed,
        source_selection_passed,
        pipeline_validation_passed,
        model_quality_passed,
        acceptance_passed,
    };

    write_eval_outputs(
        root,
        &summary,
        &results,
        &failures,
        &source_selection_errors,
    )?;

    Ok(summary)
}

fn write_eval_outputs<P: AsRef<Path>>(
    root: P,
    summary: &NextWordEvalSummaryReport,
    results: &[ContextPredictionEvalCaseResult],
    failures: &[ContextPredictionEvalCaseResult],
    source_errors: &[ContextPredictionEvalCase],
) -> Result<(), String> {
    let reports_dir = root
        .as_ref()
        .join("data/reports/context-prediction-evaluation");
    fs::create_dir_all(&reports_dir)
        .map_err(|e| format!("Failed to create {:?}: {}", reports_dir, e))?;

    write_json(&reports_dir.join("summary.json"), summary)?;

    let results_file = reports_dir.join("results.jsonl");
    let mut file = File::create(&results_file)
        .map_err(|e| format!("Failed to create {:?}: {}", results_file, e))?;
    for res in results {
        let json_line =
            serde_json::to_string(res).map_err(|e| format!("Serialize error: {}", e))?;
        writeln!(file, "{}", json_line).map_err(|e| format!("Write error: {}", e))?;
    }

    write_json(&reports_dir.join("failures.jsonl"), failures)?;
    write_json(
        &reports_dir.join("source-selection-errors.jsonl"),
        source_errors,
    )?;

    let readme = format!(
        "# Context Prediction Evaluation Report\n\n\
        - **Total Cases**: {}\n\
        - **Positive Cases**: {}\n\
        - **Trigram Hits**: {}\n\
        - **Bigram Backoffs**: {}\n\
        - **Unknown Contexts**: {}\n\
        - **Positive Top-1 Accuracy**: {:.2}%\n\
        - **Positive Top-3 Accuracy**: {:.2}%\n\
        - **Positive Top-5 Accuracy**: {:.2}%\n\
        - **Positive MRR**: {:.4}\n\
        - **Baseline Bigram Top-3 Accuracy**: {:.2}%\n\
        - **Source Selection Accuracy**: {:.2}%\n\
        - **Pipeline Validated**: {}\n\
        - **Model Quality Passed**: {}\n\
        - **Acceptance Passed**: {}\n",
        summary.total_cases,
        summary.positive_case_count,
        summary.trigram_hit_count,
        summary.bigram_backoff_count,
        summary.unknown_context_count,
        summary.positive_top_1_accuracy,
        summary.positive_top_3_accuracy,
        summary.positive_top_5_accuracy,
        summary.positive_mean_reciprocal_rank,
        summary.baseline_bigram_top_3_accuracy,
        summary.source_selection_accuracy,
        summary.pipeline_validation_passed,
        summary.model_quality_passed,
        summary.acceptance_passed
    );
    fs::write(reports_dir.join("README.md"), readme)
        .map_err(|e| format!("Write README.md error: {}", e))?;

    generate_manifest(&reports_dir)?;

    Ok(())
}

fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let json_bytes =
        serde_json::to_vec_pretty(value).map_err(|e| format!("JSON serialize error: {}", e))?;
    fs::write(path, json_bytes).map_err(|e| format!("Write error for {:?}: {}", path, e))
}

fn generate_manifest(reports_dir: &Path) -> Result<(), String> {
    let manifest_path = reports_dir.join("artifacts.sha256");
    let root = reports_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or_else(|| "Failed to find workspace root".to_string())?;

    let mut file_entries = Vec::new();
    let read_dir = fs::read_dir(reports_dir)
        .map_err(|e| format!("Read dir error {:?}: {}", reports_dir, e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
        let path = entry.path();
        if path.is_file() && path.file_name().is_some_and(|n| n != "artifacts.sha256") {
            let rel_path = path
                .strip_prefix(root)
                .map_err(|_| "Failed strip prefix")?
                .to_str()
                .ok_or_else(|| "UTF8 path conversion error".to_string())?
                .to_string();

            let bytes =
                fs::read(&path).map_err(|e| format!("Read file error {:?}: {}", path, e))?;
            let hash = format!("{:x}", Sha256::digest(&bytes));
            file_entries.push((rel_path, hash));
        }
    }

    file_entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut manifest_content = String::new();
    for (rel_path, hash) in file_entries {
        manifest_content.push_str(&format!("{}  {}\n", hash, rel_path));
    }

    fs::write(&manifest_path, manifest_content)
        .map_err(|e| format!("Write manifest error {:?}: {}", manifest_path, e))
}
