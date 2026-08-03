//! Report serialization and manifest checksum utilities for evaluation benchmark outputs.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::evaluation::schema::{validate_case_record, BenchmarkCaseRecord};

/// Loads and validates a JSONL benchmark case file (`draft-cases.jsonl` or `reviewed-cases.jsonl`).
pub fn load_benchmark_cases<P: AsRef<Path>>(path: P) -> Result<Vec<BenchmarkCaseRecord>, String> {
    let p = path.as_ref();
    if !p.exists() {
        return Err(format!("Benchmark case file missing at {:?}", p));
    }
    let file =
        File::open(p).map_err(|e| format!("Failed to open benchmark file {:?}: {}", p, e))?;
    let mut records = Vec::new();
    for (l_idx, line_res) in BufReader::new(file).lines().enumerate() {
        let line =
            line_res.map_err(|e| format!("Read error in {:?} line {}: {}", p, l_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: BenchmarkCaseRecord = serde_json::from_str(&line)
            .map_err(|e| format!("JSON error in {:?} line {}: {}", p, l_idx + 1, e))?;
        validate_case_record(&record)
            .map_err(|e| format!("Validation error in {:?} line {}: {}", p, l_idx + 1, e))?;
        records.push(record);
    }
    Ok(records)
}

/// Calculates SHA-256 hex string for a file.
pub fn calculate_file_sha256<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let p = path.as_ref();
    let bytes = std::fs::read(p).map_err(|e| format!("Read error for {:?}: {}", p, e))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}
