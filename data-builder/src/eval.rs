use crate::compile::compile_binary_pack;
use crate::merge::merge_and_deduplicate;
use crate::validate::SourceLexiconEntry;
use kurmanci_engine::Engine;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconMetrics {
    pub dataset_name: String,
    pub entry_count: usize,
    pub binary_pack_size_bytes: usize,
    pub known_word_coverage_percent: f64,
    pub correction_top_1_accuracy_percent: f64,
    pub correction_top_k_accuracy_percent: f64,
    pub completion_recall_percent: f64,
    pub load_time_us: u64,
    pub avg_query_latency_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub source_id: String,
    pub timestamp: String,
    pub baseline_manual_seed: LexiconMetrics,
    pub combined_with_imported: LexiconMetrics,
    pub quality_note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BenchmarkItem {
    pub input: String,
    pub expected: String,
}

pub fn evaluate_lexicon_impact<P: AsRef<Path>>(
    imported_jsonl_path: P,
    root_dir: P,
) -> Result<EvaluationReport, String> {
    let root = root_dir.as_ref();
    let seed_path = root.join("data/reviewed/lexicon.jsonl");

    let seed_entries = load_jsonl_entries(&seed_path)?;

    let mut imported_entries = Vec::new();
    let imp_p = imported_jsonl_path.as_ref();
    if imp_p.exists() {
        imported_entries = load_jsonl_entries(imp_p)?;
    }

    let mut combined_entries = seed_entries.clone();
    combined_entries.extend(imported_entries);
    let combined_entries = merge_and_deduplicate(combined_entries);

    // Benchmark items
    let gold_path = root.join("data/benchmarks/spelling_gold.jsonl");
    if !gold_path.exists() {
        return Err(format!(
            "Benchmark file {:?} is missing; evaluation metrics cannot be computed",
            gold_path
        ));
    }
    let bench_items = load_benchmark_items(&gold_path)?;
    if bench_items.is_empty() {
        return Err(format!(
            "Benchmark file {:?} contains 0 valid items; evaluation metrics cannot be computed",
            gold_path
        ));
    }

    let baseline_metrics = evaluate_entries("baseline_manual_seed", &seed_entries, &bench_items)?;
    let combined_metrics =
        evaluate_entries("combined_with_imported", &combined_entries, &bench_items)?;

    let report = EvaluationReport {
        source_id: "kurdish-hunspell-kmr".to_string(),
        timestamp: "2026-07-30T00:00:00Z".to_string(),
        baseline_manual_seed: baseline_metrics,
        combined_with_imported: combined_metrics,
        quality_note: "Increasing entry count expands dictionary coverage but may affect edit-distance candidate ranking precision on unweighted datasets without corpus frequencies.".to_string(),
    };

    let report_dir = root.join("data/reports/kurdish-hunspell-kmr");
    fs::create_dir_all(&report_dir).map_err(|e| format!("Failed to create report dir: {}", e))?;
    let report_path = report_dir.join("evaluation-report.json");

    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("Failed to serialize eval report: {}", e))?;
    fs::write(&report_path, json)
        .map_err(|e| format!("Failed to write eval report {:?}: {}", report_path, e))?;

    Ok(report)
}

fn load_jsonl_entries(path: &Path) -> Result<Vec<SourceLexiconEntry>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open file {:?}: {}", path, e))?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for (idx, line_res) in reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("Line {}: read error: {}", idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: SourceLexiconEntry = serde_json::from_str(&line)
            .map_err(|e| format!("Line {}: json error: {}", idx + 1, e))?;
        entries.push(entry);
    }
    Ok(entries)
}

fn load_benchmark_items(path: &Path) -> Result<Vec<BenchmarkItem>, String> {
    let file =
        File::open(path).map_err(|e| format!("Failed to open benchmark {:?}: {}", path, e))?;
    let reader = BufReader::new(file);
    let mut items = Vec::new();

    for (index, line_result) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line_result.map_err(|e| {
            format!(
                "Benchmark {:?}, line {}: read error: {}",
                path, line_number, e
            )
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let item = serde_json::from_str::<BenchmarkItem>(&line).map_err(|e| {
            format!(
                "Benchmark {:?}, line {}: invalid JSON: {}",
                path, line_number, e
            )
        })?;

        items.push(item);
    }

    Ok(items)
}

fn evaluate_entries(
    name: &str,
    entries: &[SourceLexiconEntry],
    items: &[BenchmarkItem],
) -> Result<LexiconMetrics, String> {
    if items.is_empty() {
        return Err("Benchmark dataset contains 0 items".to_string());
    }

    let binary_bytes = compile_binary_pack(entries)?;
    let pack_size = binary_bytes.len();

    let start_load = Instant::now();
    let mut engine = Engine::new();
    engine
        .load_binary_pack(&binary_bytes)
        .map_err(|e| e.to_string())?;
    let load_time_us = start_load.elapsed().as_micros() as u64;

    let mut top_1_correct = 0;
    let mut top_k_correct = 0;
    let mut completion_recalled = 0;
    let mut known_coverage = 0;
    let mut total_latency_us = 0.0;

    let total_items = items.len();

    for item in items {
        let query_start = Instant::now();
        let suggestions = engine.suggest(&item.input, 5);
        total_latency_us += query_start.elapsed().as_micros() as f64;

        if engine.contains(&item.expected) {
            known_coverage += 1;
        }

        if !suggestions.is_empty() {
            if suggestions[0].text == item.expected {
                top_1_correct += 1;
            }
            if suggestions.iter().any(|s| s.text == item.expected) {
                top_k_correct += 1;
            }
        }

        // Prefix completion recall evaluation (3-char prefix)
        let prefix: String = item.expected.chars().take(3).collect();
        if !prefix.is_empty() {
            let completions = engine.complete(&prefix, 10);
            if completions.iter().any(|c| c.text == item.expected) {
                completion_recalled += 1;
            }
        }
    }

    let avg_latency = total_latency_us / total_items as f64;
    let cov_pct = (known_coverage as f64 / total_items as f64) * 100.0;
    let top1_pct = (top_1_correct as f64 / total_items as f64) * 100.0;
    let topk_pct = (top_k_correct as f64 / total_items as f64) * 100.0;
    let comp_pct = (completion_recalled as f64 / total_items as f64) * 100.0;

    Ok(LexiconMetrics {
        dataset_name: name.to_string(),
        entry_count: entries.len(),
        binary_pack_size_bytes: pack_size,
        known_word_coverage_percent: cov_pct,
        correction_top_1_accuracy_percent: top1_pct,
        correction_top_k_accuracy_percent: topk_pct,
        completion_recall_percent: comp_pct,
        load_time_us,
        avg_query_latency_us: avg_latency,
    })
}
