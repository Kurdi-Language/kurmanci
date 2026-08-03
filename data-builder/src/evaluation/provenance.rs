//! Deterministic provenance overlap reporting for evaluation benchmark cases.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::corpus::importer::LockFileGuard;
use crate::corpus::partition::PartitionDocumentRecord;
use crate::corpus::tokenizer::tokenize_text;
use crate::evaluation::reports::{calculate_file_sha256, load_benchmark_cases};
use crate::validate::SourceLexiconEntry;

/// Provenance overlap record for an individual benchmark case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseProvenanceOverlapRecord {
    pub case_id: String,
    pub task: String,
    pub category: String,
    pub input: String,
    pub expected_candidates: Vec<String>,
    pub input_in_manual_seed: bool,
    pub expected_in_manual_seed: bool,
    pub input_in_hunspell: bool,
    pub expected_in_hunspell: bool,
    pub input_in_train_partition: Option<bool>,
    pub expected_in_train_partition: Option<bool>,
    pub input_in_dev_eval_partition: Option<bool>,
    pub expected_in_dev_eval_partition: Option<bool>,
}

/// Provenance report summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationProvenanceSummary {
    pub schema_version: String,
    pub total_benchmark_cases: usize,
    pub manual_seed_overlap_count: usize,
    pub hunspell_overlap_count: usize,
    pub train_partition_evaluated: bool,
    pub train_partition_overlap_count: usize,
    pub dev_eval_partition_evaluated: bool,
    pub dev_eval_partition_overlap_count: usize,
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

/// Generates provenance overlap analysis under `data/reports/evaluation-provenance/`.
pub fn generate_provenance_report<P: AsRef<Path>>(
    root_dir: P,
) -> Result<EvaluationProvenanceSummary, String> {
    let root = root_dir.as_ref();

    let data_dir = root.join("data");
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).map_err(|e| format!("Failed to create data dir: {}", e))?;
    }

    let lock_path = root.join("data/eval-provenance.lock");
    let lock = LockFileGuard::acquire(&lock_path)?;

    // 1. Load manual seed normalized words strictly with SourceLexiconEntry (MANDATORY FILE)
    let seed_file = root.join("data/reviewed/lexicon.jsonl");
    if !seed_file.exists() {
        return Err(format!(
            "Required seed lexicon file missing at '{:?}'. Run `cargo run -p kurmanci-data-builder -- build` or ensure seed lexicon exists.",
            seed_file
        ));
    }
    let mut manual_seed_words = BTreeSet::new();
    let f_seed = File::open(&seed_file).map_err(|e| e.to_string())?;
    for (l_idx, line_res) in BufReader::new(f_seed).lines().enumerate() {
        let line = line_res.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: SourceLexiconEntry = serde_json::from_str(&line).map_err(|e| {
            format!(
                "Typed parsing error in seed lexicon line {}: {}",
                l_idx + 1,
                e
            )
        })?;
        manual_seed_words.insert(entry.normalized);
    }

    // 2. Load Hunspell imported normalized words strictly with SourceLexiconEntry (MANDATORY FILE)
    let hunspell_file = root.join("data/imported/kurdish-hunspell-kmr/lexicon.jsonl");
    if !hunspell_file.exists() {
        return Err(format!(
            "Required Hunspell lexicon file missing at '{:?}'. Run `cargo run -p kurmanci-data-builder -- import-hunspell kurdish-hunspell-kmr` to import Hunspell source data.",
            hunspell_file
        ));
    }
    let mut hunspell_words = BTreeSet::new();
    let f_hunspell = File::open(&hunspell_file).map_err(|e| e.to_string())?;
    for (l_idx, line_res) in BufReader::new(f_hunspell).lines().enumerate() {
        let line = line_res.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: SourceLexiconEntry = serde_json::from_str(&line).map_err(|e| {
            format!(
                "Typed parsing error in Hunspell lexicon line {}: {}",
                l_idx + 1,
                e
            )
        })?;
        hunspell_words.insert(entry.normalized);
    }

    // 3. Load corpus partitions strictly with PartitionDocumentRecord (OPTIONAL FILES)
    let train_file = root.join("data/build/corpus-partitions/train.jsonl");
    let dev_file = root.join("data/build/corpus-partitions/development.jsonl");
    let eval_file = root.join("data/build/corpus-partitions/evaluation.jsonl");

    let mut train_words = BTreeSet::new();
    let train_evaluated = train_file.exists();
    if train_evaluated {
        let f = File::open(&train_file).map_err(|e| e.to_string())?;
        for (l_idx, line_res) in BufReader::new(f).lines().enumerate() {
            let line = line_res.map_err(|e| e.to_string())?;
            if line.trim().is_empty() {
                continue;
            }
            let doc: PartitionDocumentRecord = serde_json::from_str(&line).map_err(|e| {
                format!(
                    "Typed parsing error in train partition line {}: {}",
                    l_idx + 1,
                    e
                )
            })?;
            let tokens = tokenize_text(&doc.text);
            for t in tokens {
                train_words.insert(t);
            }
        }
    }

    let mut dev_eval_words = BTreeSet::new();
    let dev_eval_evaluated = dev_file.exists() || eval_file.exists();
    if dev_file.exists() {
        let f = File::open(&dev_file).map_err(|e| e.to_string())?;
        for (l_idx, line_res) in BufReader::new(f).lines().enumerate() {
            let line = line_res.map_err(|e| e.to_string())?;
            if line.trim().is_empty() {
                continue;
            }
            let doc: PartitionDocumentRecord = serde_json::from_str(&line).map_err(|e| {
                format!(
                    "Typed parsing error in dev partition line {}: {}",
                    l_idx + 1,
                    e
                )
            })?;
            let tokens = tokenize_text(&doc.text);
            for t in tokens {
                dev_eval_words.insert(t);
            }
        }
    }
    if eval_file.exists() {
        let f = File::open(&eval_file).map_err(|e| e.to_string())?;
        for (l_idx, line_res) in BufReader::new(f).lines().enumerate() {
            let line = line_res.map_err(|e| e.to_string())?;
            if line.trim().is_empty() {
                continue;
            }
            let doc: PartitionDocumentRecord = serde_json::from_str(&line).map_err(|e| {
                format!(
                    "Typed parsing error in eval partition line {}: {}",
                    l_idx + 1,
                    e
                )
            })?;
            let tokens = tokenize_text(&doc.text);
            for t in tokens {
                dev_eval_words.insert(t);
            }
        }
    }

    // 4. Load benchmark cases
    let mut cases = Vec::new();
    let draft_path = root.join("evaluation/spelling/draft-cases.jsonl");
    let reviewed_path = root.join("evaluation/spelling/reviewed-cases.jsonl");
    if draft_path.exists() {
        cases.extend(load_benchmark_cases(&draft_path)?);
    }
    if reviewed_path.exists() {
        cases.extend(load_benchmark_cases(&reviewed_path)?);
    }

    let mut overlap_records = Vec::new();
    let mut manual_seed_count = 0;
    let mut hunspell_count = 0;
    let mut train_count = 0;
    let mut dev_eval_count = 0;

    for case in &cases {
        let input_norm = crate::normalize::normalize_text(&case.input);
        let in_seed = manual_seed_words.contains(&input_norm);
        let exp_in_seed = case
            .expectation
            .expected_candidates
            .iter()
            .any(|c| manual_seed_words.contains(&crate::normalize::normalize_text(c)));

        let in_hunspell = hunspell_words.contains(&input_norm);
        let exp_in_hunspell = case
            .expectation
            .expected_candidates
            .iter()
            .any(|c| hunspell_words.contains(&crate::normalize::normalize_text(c)));

        if in_seed || exp_in_seed {
            manual_seed_count += 1;
        }
        if in_hunspell || exp_in_hunspell {
            hunspell_count += 1;
        }

        let (in_train_opt, exp_train_opt) = if train_evaluated {
            let in_tr = train_words.contains(&input_norm);
            let exp_tr = case
                .expectation
                .expected_candidates
                .iter()
                .any(|c| train_words.contains(&crate::normalize::normalize_text(c)));
            if in_tr || exp_tr {
                train_count += 1;
            }
            (Some(in_tr), Some(exp_tr))
        } else {
            (None, None)
        };

        let (in_dev_opt, exp_dev_opt) = if dev_eval_evaluated {
            let in_de = dev_eval_words.contains(&input_norm);
            let exp_de = case
                .expectation
                .expected_candidates
                .iter()
                .any(|c| dev_eval_words.contains(&crate::normalize::normalize_text(c)));
            if in_de || exp_de {
                dev_eval_count += 1;
            }
            (Some(in_de), Some(exp_de))
        } else {
            (None, None)
        };

        overlap_records.push(CaseProvenanceOverlapRecord {
            case_id: case.case_id.clone(),
            task: case.task.as_str().to_string(),
            category: case.category.as_str().to_string(),
            input: case.input.clone(),
            expected_candidates: case.expectation.expected_candidates.clone(),
            input_in_manual_seed: in_seed,
            expected_in_manual_seed: exp_in_seed,
            input_in_hunspell: in_hunspell,
            expected_in_hunspell: exp_in_hunspell,
            input_in_train_partition: in_train_opt,
            expected_in_train_partition: exp_train_opt,
            input_in_dev_eval_partition: in_dev_opt,
            expected_in_dev_eval_partition: exp_dev_opt,
        });
    }

    // Write reports atomically
    let stage_dir = root.join("data/reports/.evaluation-provenance.tmp-stage");
    let backup_dir = root.join("data/reports/.evaluation-provenance.tmp-backup");
    let out_dir = root.join("data/reports/evaluation-provenance");

    if stage_dir.exists() {
        remove_dir_or_file(&stage_dir).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&stage_dir).map_err(|e| e.to_string())?;

    let summary = EvaluationProvenanceSummary {
        schema_version: "evaluation-provenance-v1".to_string(),
        total_benchmark_cases: cases.len(),
        manual_seed_overlap_count: manual_seed_count,
        hunspell_overlap_count: hunspell_count,
        train_partition_evaluated: train_evaluated,
        train_partition_overlap_count: train_count,
        dev_eval_partition_evaluated: dev_eval_evaluated,
        dev_eval_partition_overlap_count: dev_eval_count,
    };

    let summary_path = stage_dir.join("summary.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let cases_path = stage_dir.join("case-overlap.jsonl");
    let mut c_file = File::create(&cases_path).map_err(|e| e.to_string())?;
    for rec in &overlap_records {
        let line = serde_json::to_string(rec).map_err(|e| e.to_string())?;
        writeln!(c_file, "{}", line).map_err(|e| e.to_string())?;
    }

    let readme_path = stage_dir.join("README.md");
    fs::write(
        &readme_path,
        "# Benchmark Provenance Overlap Report\n\nDiagnostic analysis of benchmark case overlap with lexical sources.\n",
    )
    .map_err(|e| e.to_string())?;

    // artifacts.sha256
    let report_files = ["summary.json", "case-overlap.jsonl", "README.md"];
    let mut manifest_entries = Vec::new();
    for f in &report_files {
        let hash = calculate_file_sha256(stage_dir.join(f))?;
        manifest_entries.push(format!("{} data/reports/evaluation-provenance/{}", hash, f));
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
            return Err(format!(
                "Failed to install evaluation-provenance dir: {}",
                err
            ));
        }
    }

    lock.release()?;
    Ok(summary)
}
