//! Report serialization for the quality audit. Writes all JSON/JSONL output
//! files with proper error propagation (no unwrap).

use crate::audit::analysis::{AcceptedAnalysis, CrossCheckResult, SourceAnalysis};
use crate::audit::input::AuditInputs;
use crate::audit::sampling::ReviewSample;
use crate::audit::AUDIT_VERSION;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

// ─── Summary schema ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    pub source_id: String,
    pub source_revision: String,
    pub audit_version: String,
    pub unicode_dependencies: BTreeMap<String, String>,
    pub unicode_data_version: String,
    pub normalization_policy: String,
    pub total_imported_records: usize,
    pub importer_cross_check: CrossCheckResult,
    pub structural_validity: StructuralValiditySummary,
    pub unicode_analysis: UnicodeAnalysisSummary,
    pub script_analysis: ScriptAnalysisBrief,
    pub character_inventory_brief: CharacterInventoryBrief,
    pub length_distribution_brief: LengthDistributionBrief,
    pub shape_analysis_brief: ShapeAnalysisBrief,
    pub flag_analysis_brief: FlagAnalysisBrief,
    pub duplicate_analysis: DuplicateAnalysisBrief,
    pub rejection_analysis: RejectionAnalysisBrief,
    pub manual_seed_comparison_brief: ManualSeedBrief,
    pub benchmark_audit_brief: BenchmarkAuditBrief,
    pub suspicious_entries_count: usize,
    pub review_sample_size: usize,
    pub verdict: String,
    pub verdict_policy_version: String,
    pub verdict_is_automated: bool,
    pub verdict_explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralValiditySummary {
    pub population: String,
    pub clean_raw_lines: usize,
    pub lines_with_findings: usize,
    pub total_findings: usize,
    pub by_finding: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnicodeAnalysisSummary {
    pub population: String,
    pub total_records: usize,
    pub distinct_codepoints_in_normalized: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptAnalysisBrief {
    pub population: String,
    pub by_script: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterInventoryBrief {
    pub population: String,
    pub distinct_codepoints: usize,
    pub rare_codepoints_under_5: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LengthDistributionBrief {
    pub population: String,
    pub min_scalar: usize,
    pub max_scalar: usize,
    pub mean_scalar_numerator: u64,
    pub mean_scalar_denominator: u64,
    pub mean_scalar_display_4dp: String,
    pub median_scalar: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShapeAnalysisBrief {
    pub population: String,
    pub total: usize,
    pub punctuation_only: usize,
    pub symbol_only: usize,
    pub digit_only: usize,
    pub no_letters: usize,
    pub uppercase_only: usize,
    pub title_case: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagAnalysisBrief {
    pub population: String,
    pub entries_with_flags: usize,
    pub distinct_flag_strings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateAnalysisBrief {
    pub population: String,
    pub exact_duplicate_additional_records: usize,
    pub metadata_conflict_groups: usize,
    pub metadata_conflict_additional_records: usize,
    pub metadata_conflicting_records_total: usize,
    pub unique_normalized_forms: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectionAnalysisBrief {
    pub population: String,
    pub total_rejected: usize,
    pub by_reason: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualSeedBrief {
    pub seed_total: usize,
    pub hunspell_total: usize,
    pub normalized_overlap: usize,
    pub seed_only_count: usize,
    pub hunspell_only_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkAuditBrief {
    pub benchmark_size: usize,
    pub weaknesses: Vec<String>,
}

// ─── Report writer ──────────────────────────────────────────────────────────

/// Writes all audit reports to the output directory.
pub fn write_all_reports(
    output_dir: &Path,
    source_id: &str,
    inputs: &AuditInputs,
    cross_check: &CrossCheckResult,
    source_analysis: &SourceAnalysis,
    accepted_analysis: &AcceptedAnalysis,
    review_sample: &ReviewSample,
) -> Result<(), String> {
    // Write into a temporary stage directory first for atomic safety
    let stage_dir = output_dir.with_extension("tmp_stage");
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .map_err(|e| format!("Failed to clean existing stage dir {:?}: {}", stage_dir, e))?;
    }
    fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("Failed to create stage dir {:?}: {}", stage_dir, e))?;

    // ── summary.json ────────────────────────────────────────────────────
    let sa = &source_analysis;
    let distinct_lines_with_findings = sa.raw_line_findings.distinct_lines_with_findings;
    let total_findings = sa.raw_line_findings.total_findings;

    let mut by_finding = BTreeMap::new();
    if sa.raw_line_findings.lines_with_leading_whitespace > 0 {
        by_finding.insert(
            "leading_whitespace".to_string(),
            sa.raw_line_findings.lines_with_leading_whitespace,
        );
    }
    if sa.raw_line_findings.lines_with_trailing_whitespace > 0 {
        by_finding.insert(
            "trailing_whitespace".to_string(),
            sa.raw_line_findings.lines_with_trailing_whitespace,
        );
    }
    if sa.raw_line_findings.lines_with_tabs > 0 {
        by_finding.insert("tabs".to_string(), sa.raw_line_findings.lines_with_tabs);
    }
    if sa.raw_line_findings.lines_with_control_chars > 0 {
        by_finding.insert(
            "control_chars".to_string(),
            sa.raw_line_findings.lines_with_control_chars,
        );
    }
    if sa.raw_line_findings.lines_with_unexpected_cr > 0 {
        by_finding.insert(
            "unexpected_cr".to_string(),
            sa.raw_line_findings.lines_with_unexpected_cr,
        );
    }
    if sa.raw_line_findings.lines_with_null_bytes > 0 {
        by_finding.insert(
            "null_bytes".to_string(),
            sa.raw_line_findings.lines_with_null_bytes,
        );
    }
    if sa.raw_line_findings.lines_with_replacement_chars > 0 {
        by_finding.insert(
            "replacement_chars".to_string(),
            sa.raw_line_findings.lines_with_replacement_chars,
        );
    }

    let rej_by_reason: BTreeMap<String, usize> =
        sa.rejection_review
            .iter()
            .fold(BTreeMap::new(), |mut acc, r| {
                *acc.entry(r.reason_code.clone()).or_insert(0) += 1;
                acc
            });

    let rare_under_5 = accepted_analysis
        .character_inventory
        .iter()
        .filter(|c| c.entries_containing_in_normalized < 5)
        .count();

    let distinct_norm_codepoints = accepted_analysis
        .character_inventory
        .iter()
        .filter(|c| c.occurrences_in_normalized > 0)
        .count();

    let mut unicode_deps = BTreeMap::new();
    unicode_deps.insert("unicode_script_crate".to_string(), "0.5".to_string());
    unicode_deps.insert("unicode_segmentation_crate".to_string(), "1.10".to_string());
    unicode_deps.insert(
        "unicode_general_category_crate".to_string(),
        "1".to_string(),
    );

    let summary = AuditSummary {
        source_id: source_id.to_string(),
        source_revision: inputs.source_revision.clone(),
        audit_version: AUDIT_VERSION.to_string(),
        unicode_dependencies: unicode_deps,
        unicode_data_version: "Not directly exposed by crates; pinned via Cargo.lock for reproducibility".to_string(),
        normalization_policy: "NFC".to_string(),
        total_imported_records: inputs.imported_records.len(),
        importer_cross_check: cross_check.clone(),
        structural_validity: StructuralValiditySummary {
            population: "physical_source_records".to_string(),
            clean_raw_lines: sa
                .successfully_parsed_records
                .saturating_sub(distinct_lines_with_findings),
            lines_with_findings: distinct_lines_with_findings,
            total_findings,
            by_finding,
        },
        unicode_analysis: UnicodeAnalysisSummary {
            population: "accepted_records".to_string(),
            total_records: inputs.imported_records.len(),
            distinct_codepoints_in_normalized: distinct_norm_codepoints,
        },
        script_analysis: ScriptAnalysisBrief {
            population: "accepted_records".to_string(),
            by_script: accepted_analysis.script_analysis.by_script.clone(),
        },
        character_inventory_brief: CharacterInventoryBrief {
            population: "accepted_records".to_string(),
            distinct_codepoints: accepted_analysis.character_inventory.len(),
            rare_codepoints_under_5: rare_under_5,
        },
        length_distribution_brief: LengthDistributionBrief {
            population: "accepted_records".to_string(),
            min_scalar: accepted_analysis.length_distribution.min_scalar,
            max_scalar: accepted_analysis.length_distribution.max_scalar,
            mean_scalar_numerator: accepted_analysis.length_distribution.mean_scalar_numerator,
            mean_scalar_denominator: accepted_analysis.length_distribution.mean_scalar_denominator,
            mean_scalar_display_4dp: accepted_analysis
                .length_distribution
                .mean_scalar_display_4dp
                .clone(),
            median_scalar: accepted_analysis.length_distribution.median_scalar,
        },
        shape_analysis_brief: ShapeAnalysisBrief {
            population: "accepted_records".to_string(),
            total: accepted_analysis.shape_analysis.total,
            punctuation_only: accepted_analysis.shape_analysis.punctuation_only,
            symbol_only: accepted_analysis.shape_analysis.symbol_only,
            digit_only: accepted_analysis.shape_analysis.digit_only,
            no_letters: accepted_analysis.shape_analysis.no_letters,
            uppercase_only: accepted_analysis.shape_analysis.uppercase_only,
            title_case: accepted_analysis.shape_analysis.title_case,
        },
        flag_analysis_brief: FlagAnalysisBrief {
            population: "accepted_records".to_string(),
            entries_with_flags: accepted_analysis.flag_analysis.entries_with_flags,
            distinct_flag_strings: accepted_analysis.flag_analysis.distinct_flag_strings,
        },
        duplicate_analysis: DuplicateAnalysisBrief {
            population: "physical_source_records".to_string(),
            exact_duplicate_additional_records: cross_check
                .audit_exact_duplicate_additional_records,
            metadata_conflict_groups: cross_check.audit_metadata_conflict_groups,
            metadata_conflict_additional_records: cross_check
                .audit_metadata_conflict_additional_records,
            metadata_conflicting_records_total: cross_check
                .audit_metadata_conflicting_records_total,
            unique_normalized_forms: cross_check.audit_unique_normalized_forms,
        },
        rejection_analysis: RejectionAnalysisBrief {
            population: "physical_source_records".to_string(),
            total_rejected: sa.rejected_records,
            by_reason: rej_by_reason,
        },
        manual_seed_comparison_brief: ManualSeedBrief {
            seed_total: accepted_analysis.manual_seed_comparison.seed_total,
            hunspell_total: accepted_analysis.manual_seed_comparison.hunspell_total,
            normalized_overlap: accepted_analysis.manual_seed_comparison.normalized_overlap,
            seed_only_count: accepted_analysis.manual_seed_comparison.seed_only_count,
            hunspell_only_count: accepted_analysis.manual_seed_comparison.hunspell_only_count,
        },
        benchmark_audit_brief: BenchmarkAuditBrief {
            benchmark_size: accepted_analysis.benchmark_audit.benchmark_size,
            weaknesses: accepted_analysis.benchmark_audit.weaknesses.clone(),
        },
        suspicious_entries_count: accepted_analysis.suspicious_entries.len(),
        review_sample_size: review_sample.records_emitted,
        verdict: "A".to_string(),
        verdict_policy_version: "quality-audit-policy-0.1".to_string(),
        verdict_is_automated: false,
        verdict_explanation: "Suitable for controlled evaluation only. Manual linguistic review required before production inclusion.".to_string(),
    };

    write_json(&stage_dir, "summary.json", &summary)?;

    // ── character-inventory.json ────────────────────────────────────────
    write_json(
        &stage_dir,
        "character-inventory.json",
        &accepted_analysis.character_inventory,
    )?;

    // ── script-analysis.json ────────────────────────────────────────────
    write_json(
        &stage_dir,
        "script-analysis.json",
        &accepted_analysis.script_analysis,
    )?;

    // ── length-distribution.json ────────────────────────────────────────
    write_json(
        &stage_dir,
        "length-distribution.json",
        &accepted_analysis.length_distribution,
    )?;

    // ── shape-analysis.json ─────────────────────────────────────────────
    write_json(
        &stage_dir,
        "shape-analysis.json",
        &accepted_analysis.shape_analysis,
    )?;

    // ── flag-analysis.json ──────────────────────────────────────────────
    write_json(
        &stage_dir,
        "flag-analysis.json",
        &accepted_analysis.flag_analysis,
    )?;

    // ── morphology-analysis.json ────────────────────────────────────────
    write_json(
        &stage_dir,
        "morphology-analysis.json",
        &accepted_analysis.morphology_analysis,
    )?;

    // ── conflict-groups.jsonl ───────────────────────────────────────────
    write_jsonl(
        &stage_dir,
        "conflict-groups.jsonl",
        &source_analysis.conflict_groups,
    )?;

    // ── duplicate-groups.jsonl ──────────────────────────────────────────
    write_jsonl(
        &stage_dir,
        "duplicate-groups.jsonl",
        &source_analysis.duplicate_groups,
    )?;

    // ── rejection-review.jsonl ──────────────────────────────────────────
    write_jsonl(
        &stage_dir,
        "rejection-review.jsonl",
        &source_analysis.rejection_review,
    )?;

    // ── suspicious-entries.jsonl ────────────────────────────────────────
    write_jsonl(
        &stage_dir,
        "suspicious-entries.jsonl",
        &accepted_analysis.suspicious_entries,
    )?;

    // ── manual-seed-comparison.json ─────────────────────────────────────
    write_json(
        &stage_dir,
        "manual-seed-comparison.json",
        &accepted_analysis.manual_seed_comparison,
    )?;

    // ── benchmark-audit.json ────────────────────────────────────────────
    write_json(
        &stage_dir,
        "benchmark-audit.json",
        &accepted_analysis.benchmark_audit,
    )?;

    // ── review-sample.jsonl ─────────────────────────────────────────────
    write_jsonl(&stage_dir, "review-sample.jsonl", &review_sample.records)?;

    // ── README.md ───────────────────────────────────────────────────────
    write_readme(&stage_dir, &summary, source_analysis, accepted_analysis)?;

    // ── artifacts.sha256 ────────────────────────────────────────────────
    let report_files = [
        "benchmark-audit.json",
        "character-inventory.json",
        "conflict-groups.jsonl",
        "duplicate-groups.jsonl",
        "flag-analysis.json",
        "length-distribution.json",
        "manual-seed-comparison.json",
        "morphology-analysis.json",
        "README.md",
        "rejection-review.jsonl",
        "review-sample.jsonl",
        "script-analysis.json",
        "shape-analysis.json",
        "summary.json",
        "suspicious-entries.jsonl",
    ];

    let mut manifest_content = String::new();
    for file in &report_files {
        let content = fs::read(stage_dir.join(file))
            .map_err(|e| format!("Failed to read report file {} for manifest: {}", file, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        manifest_content.push_str(&format!("{}  {}\n", hash, file));
    }
    fs::write(stage_dir.join("artifacts.sha256"), manifest_content)
        .map_err(|e| format!("Failed to write artifacts.sha256 manifest: {}", e))?;

    // Atomic replacement: remove output_dir if present, then rename stage_dir -> output_dir
    if output_dir.exists() {
        fs::remove_dir_all(output_dir).map_err(|e| {
            format!(
                "Failed to clean existing audit output dir {:?}: {}",
                output_dir, e
            )
        })?;
    }
    fs::rename(&stage_dir, output_dir).map_err(|e| {
        format!(
            "Failed to move stage dir {:?} to output dir {:?}: {}",
            stage_dir, output_dir, e
        )
    })?;

    Ok(())
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn write_json<T: Serialize>(dir: &Path, filename: &str, data: &T) -> Result<(), String> {
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Failed to serialize {}: {}", filename, e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {:?}: {}", path, e))?;
    Ok(())
}

fn write_jsonl<T: Serialize>(dir: &Path, filename: &str, items: &[T]) -> Result<(), String> {
    let path = dir.join(filename);
    let mut file =
        File::create(&path).map_err(|e| format!("Failed to create {:?}: {}", path, e))?;
    for item in items {
        let line = serde_json::to_string(item)
            .map_err(|e| format!("Failed to serialize {} record: {}", filename, e))?;
        file.write_all(line.as_bytes())
            .map_err(|e| format!("Failed to write to {:?}: {}", path, e))?;
        file.write_all(b"\n")
            .map_err(|e| format!("Failed to write newline to {:?}: {}", path, e))?;
    }
    Ok(())
}

fn write_readme(
    dir: &Path,
    summary: &AuditSummary,
    _source_analysis: &SourceAnalysis,
    _accepted_analysis: &AcceptedAnalysis,
) -> Result<(), String> {
    let readme = format!(
        r#"# Quality Audit Report: {}

**Audit Version**: {}
**Source Revision**: {}
**Normalization Policy**: {}

## Cross-Check

- **Passed**: {}
- **Importer accepted**: {}
- **Audit unique normalized**: {}

## Key Statistics (accepted_records)

| Metric | Value |
|---|---|
| Total imported records | {} |
| Distinct code points | {} |
| Median scalar length | {} |
| Punctuation-only entries | {} |
| Symbol-only entries | {} |
| Digit-only entries | {} |
| Uppercase-only entries | {} |
| Title-case entries | {} |
| Entries with flags | {} |
| Distinct flag strings | {} |

## Duplicate / Conflict Analysis (physical_source_records)

| Metric | Value |
|---|---|
| Exact duplicate additional records | {} |
| Metadata conflict groups | {} |
| Metadata conflict additional records | {} |

## Rejection Analysis (physical_source_records)

| Metric | Value |
|---|---|
| Total rejected | {} |

## Manual Seed Comparison

| Metric | Value |
|---|---|
| Seed total | {} |
| Normalized overlap | {} |
| Seed-only forms | {} |

## Benchmark

| Metric | Value |
|---|---|
| Benchmark size | {} |
| Weaknesses | {} |

## Suspicious Entries

Total flagged: {}

## Verdict

**{}** — {}
"#,
        summary.source_id,
        summary.audit_version,
        summary.source_revision,
        summary.normalization_policy,
        summary.importer_cross_check.cross_check_passed,
        summary.importer_cross_check.importer_accepted_entries,
        summary.importer_cross_check.audit_unique_normalized_forms,
        summary.total_imported_records,
        summary.character_inventory_brief.distinct_codepoints,
        summary.length_distribution_brief.median_scalar,
        summary.shape_analysis_brief.punctuation_only,
        summary.shape_analysis_brief.symbol_only,
        summary.shape_analysis_brief.digit_only,
        summary.shape_analysis_brief.uppercase_only,
        summary.shape_analysis_brief.title_case,
        summary.flag_analysis_brief.entries_with_flags,
        summary.flag_analysis_brief.distinct_flag_strings,
        summary
            .duplicate_analysis
            .exact_duplicate_additional_records,
        summary.duplicate_analysis.metadata_conflict_groups,
        summary
            .duplicate_analysis
            .metadata_conflict_additional_records,
        summary.rejection_analysis.total_rejected,
        summary.manual_seed_comparison_brief.seed_total,
        summary.manual_seed_comparison_brief.normalized_overlap,
        summary.manual_seed_comparison_brief.seed_only_count,
        summary.benchmark_audit_brief.benchmark_size,
        summary.benchmark_audit_brief.weaknesses.join("; "),
        summary.suspicious_entries_count,
        summary.verdict,
        summary.verdict_explanation,
    );

    let path = dir.join("README.md");
    fs::write(&path, readme).map_err(|e| format!("Failed to write README {:?}: {}", path, e))?;
    Ok(())
}
