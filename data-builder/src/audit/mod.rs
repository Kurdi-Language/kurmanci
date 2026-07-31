//! Quality audit module for the Kurmancî lexical data pipeline.
//!
//! Reads all importer outputs, replays the preserved source through the shared
//! parser, and produces deterministic analysis reports.

pub mod analysis;
pub mod classify;
pub mod input;
pub mod reports;
pub mod sampling;

use std::path::Path;

/// Top-level audit version, embedded in all reports.
pub const AUDIT_VERSION: &str = "0.1.0";

/// Executes the full quality audit pipeline for a given source_id.
pub fn run_quality_audit<P: AsRef<Path>>(source_id: &str, root_dir: P) -> Result<(), String> {
    let root = root_dir.as_ref();

    println!("=== Kurmancî Lexical Data Quality Audit ===");
    println!("  Source ID: {}", source_id);

    // ── 1. Load all inputs ──────────────────────────────────────────────
    println!("  [1/6] Loading audit inputs...");
    let inputs = input::load_all_inputs(source_id, root)?;

    // ── 2. Cross-check reconstructed counts against importer summary ────
    println!("  [2/6] Cross-checking against importer summary...");
    let cross_check = analysis::cross_check(&inputs)?;

    // ── 3. Run source-level analyses ────────────────────────────────────
    println!("  [3/6] Analyzing source records...");
    let source_analysis = analysis::analyze_source_records(&inputs);

    // ── 4. Run accepted-record analyses ─────────────────────────────────
    println!("  [4/6] Analyzing accepted records...");
    let accepted_analysis = analysis::analyze_accepted_records(&inputs);

    // ── 5. Generate samples ─────────────────────────────────────────────
    println!("  [5/6] Generating review samples...");
    let review_sample = sampling::generate_review_sample(&inputs, &accepted_analysis);

    // ── 6. Write reports ────────────────────────────────────────────────
    println!("  [6/6] Writing audit reports...");
    let output_dir = root
        .join("data/reports")
        .join(source_id)
        .join("quality-audit");

    reports::write_all_reports(
        &output_dir,
        source_id,
        &inputs,
        &cross_check,
        &source_analysis,
        &accepted_analysis,
        &review_sample,
    )?;

    println!("⚡ AUDIT COMPLETED!");
    println!("  Reports written to: {:?}", output_dir);
    Ok(())
}
