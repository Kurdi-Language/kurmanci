//! Analysis passes for the quality audit. Organized by population:
//! - Source-level analyses operate on replayed `.dic` records.
//! - Accepted-record analyses operate on the imported `lexicon.jsonl`.

use crate::audit::classify::{
    self, check_raw_line, classify_script, classify_shape, general_category_str,
    is_possible_proper_noun, is_unicode_letter, ScriptClass,
};
use crate::audit::input::{AuditInputs, AuditableSourceRecord};
use crate::normalize::normalize_text;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use unicode_script::UnicodeScript;

// ─── Cross-check ────────────────────────────────────────────────────────────

/// Result of cross-checking the audit's replayed counts against the importer
/// summary. The audit **fails** if semantically identical counters disagree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossCheckResult {
    /// Importer field semantics documentation.
    pub importer_field_semantics: BTreeMap<String, String>,

    // Importer values
    pub importer_physical_input_lines: usize,
    pub importer_parsed_entries: usize,
    pub importer_accepted_entries: usize,
    pub importer_rejected_entries: usize,
    pub importer_duplicate_surface_forms: usize,
    pub importer_conflicting_flag_sets: usize,

    // Audit reconstructed values
    pub audit_physical_line_count: usize,
    pub audit_successfully_parsed_records: usize,
    pub audit_rejected_records: usize,
    pub audit_dictionary_entry_lines: usize,
    pub audit_unique_normalized_forms: usize,
    pub audit_exact_duplicate_additional_records: usize,
    pub audit_metadata_conflict_additional_records: usize,
    pub audit_metadata_conflict_groups: usize,
    pub audit_metadata_conflicting_records_total: usize,

    pub cross_check_passed: bool,
    pub discrepancies: Vec<String>,
}

/// Cross-checks audit-reconstructed counts against the importer summary.
/// Fails with an error if semantically identical counters disagree.
pub fn cross_check(inputs: &AuditInputs) -> Result<CrossCheckResult, String> {
    let summary = &inputs.import_summary;

    // Reconstruct deduplication from replayed records
    let mut seen: BTreeMap<String, Vec<&AuditableSourceRecord>> = BTreeMap::new();
    for rec in &inputs.replayed_parsed {
        seen.entry(rec.normalized.clone()).or_default().push(rec);
    }

    let mut exact_dup_additional = 0usize;
    let mut conflict_additional = 0usize;
    let mut conflict_groups = 0usize;
    let mut conflict_records_total = 0usize;
    let unique_normalized = seen.len();

    for records in seen.values() {
        if records.len() <= 1 {
            continue;
        }
        // Compare first record with each subsequent
        let first = &records[0];
        let mut has_conflict = false;
        for other in &records[1..] {
            if first.word == other.word
                && first.flags == other.flags
                && first.morphology == other.morphology
                && first.part_of_speech == other.part_of_speech
            {
                exact_dup_additional += 1;
            } else {
                conflict_additional += 1;
                has_conflict = true;
            }
        }
        if has_conflict {
            conflict_groups += 1;
            conflict_records_total += records.len();
        }
    }

    let mut discrepancies = Vec::new();

    // Check: physical line count
    if inputs.physical_line_count != summary.physical_input_lines {
        discrepancies.push(format!(
            "Physical lines: audit={} vs importer={}",
            inputs.physical_line_count, summary.physical_input_lines
        ));
    }

    // Check: parsed entries (importer counts all non-blank, non-header lines as "parsed",
    // including lines that subsequently fail validation and become rejections)
    let audit_dictionary_entry_lines =
        inputs.replayed_parsed.len() + inputs.replayed_rejected.len();
    if audit_dictionary_entry_lines != summary.parsed_entries {
        discrepancies.push(format!(
            "Dictionary entry lines (parsed+rejected): audit={} vs importer={}",
            audit_dictionary_entry_lines, summary.parsed_entries
        ));
    }

    // Check: rejected entries
    if inputs.replayed_rejected.len() != summary.rejected_entries {
        discrepancies.push(format!(
            "Rejected entries: audit={} vs importer={}",
            inputs.replayed_rejected.len(),
            summary.rejected_entries
        ));
    }

    // Check: accepted entries (unique normalized forms)
    if unique_normalized != summary.accepted_entries {
        discrepancies.push(format!(
            "Accepted entries (unique normalized): audit={} vs importer={}",
            unique_normalized, summary.accepted_entries
        ));
    }

    // Check: duplicate surface forms
    if exact_dup_additional != summary.duplicate_surface_forms {
        discrepancies.push(format!(
            "Duplicate surface forms: audit={} vs importer={}",
            exact_dup_additional, summary.duplicate_surface_forms
        ));
    }

    // Check: conflicting flag sets (additional records whose normalized form
    // was already accepted but whose retained metadata differed)
    if conflict_additional != summary.conflicting_flag_sets {
        discrepancies.push(format!(
            "Conflicting flag sets (additional records): audit={} vs importer={}",
            conflict_additional, summary.conflicting_flag_sets
        ));
    }

    let cross_check_passed = discrepancies.is_empty();

    if !cross_check_passed {
        return Err(format!(
            "AUDIT CROSS-CHECK FAILED — reconstructed counts disagree with importer summary:\n  {}",
            discrepancies.join("\n  ")
        ));
    }

    let mut semantics = BTreeMap::new();
    semantics.insert(
        "conflicting_flag_sets".to_string(),
        "Additional records whose normalized form was already accepted but whose retained metadata (word, flags, morphology, or POS) differed".to_string(),
    );
    semantics.insert(
        "duplicate_surface_forms".to_string(),
        "Additional records with identical normalized form, word, flags, morphology, and POS as an already-accepted record".to_string(),
    );

    Ok(CrossCheckResult {
        importer_field_semantics: semantics,
        importer_physical_input_lines: summary.physical_input_lines,
        importer_parsed_entries: summary.parsed_entries,
        importer_accepted_entries: summary.accepted_entries,
        importer_rejected_entries: summary.rejected_entries,
        importer_duplicate_surface_forms: summary.duplicate_surface_forms,
        importer_conflicting_flag_sets: summary.conflicting_flag_sets,
        audit_physical_line_count: inputs.physical_line_count,
        audit_successfully_parsed_records: inputs.replayed_parsed.len(),
        audit_rejected_records: inputs.replayed_rejected.len(),
        audit_dictionary_entry_lines,
        audit_unique_normalized_forms: unique_normalized,
        audit_exact_duplicate_additional_records: exact_dup_additional,
        audit_metadata_conflict_additional_records: conflict_additional,
        audit_metadata_conflict_groups: conflict_groups,
        audit_metadata_conflicting_records_total: conflict_records_total,
        cross_check_passed,
        discrepancies,
    })
}

// ─── Source-level analysis ──────────────────────────────────────────────────

/// Results from analyzing the raw source records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAnalysis {
    pub population: String,
    pub physical_lines: usize,
    pub declared_count_line: usize,
    pub blank_lines: usize,
    pub dictionary_entry_lines: usize,
    pub successfully_parsed_records: usize,
    pub rejected_records: usize,

    pub raw_line_findings: RawLineFindings,
    pub parsed_surface_findings: ParsedSurfaceFindings,

    pub conflict_groups: Vec<ConflictGroup>,
    pub duplicate_groups: Vec<DuplicateGroup>,
    pub rejection_review: Vec<RejectionReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawLineFindings {
    pub lines_with_leading_whitespace: usize,
    pub lines_with_trailing_whitespace: usize,
    pub lines_with_tabs: usize,
    pub lines_with_control_chars: usize,
    pub lines_with_unexpected_cr: usize,
    pub lines_with_null_bytes: usize,
    pub lines_with_replacement_chars: usize,
    pub distinct_lines_with_findings: usize,
    pub total_findings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedSurfaceFindings {
    pub words_with_control_chars: usize,
    pub words_with_null_bytes: usize,
    pub words_with_replacement_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictGroup {
    pub normalized: String,
    pub classification: String,
    pub records: Vec<ConflictGroupRecord>,
    pub differences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictGroupRecord {
    pub source_line_num: usize,
    pub word: String,
    pub flags: String,
    pub morphology: Vec<String>,
    pub part_of_speech: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectionReview {
    pub source_line_num: usize,
    pub raw_line: String,
    pub reason_code: String,
    pub explanation: String,
}

/// A group of exact duplicate records (identical normalized form AND metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub normalized: String,
    pub word: String,
    pub flags: String,
    pub morphology: Vec<String>,
    pub part_of_speech: String,
    pub occurrence_count: usize,
    pub source_line_nums: Vec<usize>,
}

/// Analyzes the replayed source records.
pub fn analyze_source_records(inputs: &AuditInputs) -> SourceAnalysis {
    let blank_lines = inputs
        .replayed_non_entries
        .iter()
        .filter(|ne| ne.kind == crate::audit::input::NonEntryKind::Blank)
        .count();
    let declared_count_line = inputs
        .replayed_non_entries
        .iter()
        .filter(|ne| matches!(ne.kind, crate::audit::input::NonEntryKind::DeclaredCount(_)))
        .count();

    // Raw-line structural checks on ALL replayed parsed records
    let mut rlf = RawLineFindings {
        lines_with_leading_whitespace: 0,
        lines_with_trailing_whitespace: 0,
        lines_with_tabs: 0,
        lines_with_control_chars: 0,
        lines_with_unexpected_cr: 0,
        lines_with_null_bytes: 0,
        lines_with_replacement_chars: 0,
        distinct_lines_with_findings: 0,
        total_findings: 0,
    };

    let mut lines_with_findings_set = BTreeSet::new();

    for rec in &inputs.replayed_parsed {
        let findings = check_raw_line(&rec.raw_line);
        let mut line_has_finding = false;
        if findings.has_leading_whitespace {
            rlf.lines_with_leading_whitespace += 1;
            rlf.total_findings += 1;
            line_has_finding = true;
        }
        if findings.has_trailing_whitespace {
            rlf.lines_with_trailing_whitespace += 1;
            rlf.total_findings += 1;
            line_has_finding = true;
        }
        if findings.has_tab {
            rlf.lines_with_tabs += 1;
            rlf.total_findings += 1;
            line_has_finding = true;
        }
        if findings.has_control_char {
            rlf.lines_with_control_chars += 1;
            rlf.total_findings += 1;
            line_has_finding = true;
        }
        if findings.has_unexpected_cr {
            rlf.lines_with_unexpected_cr += 1;
            rlf.total_findings += 1;
            line_has_finding = true;
        }
        if findings.has_null_byte {
            rlf.lines_with_null_bytes += 1;
            rlf.total_findings += 1;
            line_has_finding = true;
        }
        if findings.has_replacement_char {
            rlf.lines_with_replacement_chars += 1;
            rlf.total_findings += 1;
            line_has_finding = true;
        }
        if line_has_finding {
            lines_with_findings_set.insert(rec.source_line_num);
        }
    }
    rlf.distinct_lines_with_findings = lines_with_findings_set.len();

    // Parsed surface findings
    let mut psf = ParsedSurfaceFindings {
        words_with_control_chars: 0,
        words_with_null_bytes: 0,
        words_with_replacement_chars: 0,
    };
    for rec in &inputs.replayed_parsed {
        if rec.word.chars().any(classify::is_unicode_control) {
            psf.words_with_control_chars += 1;
        }
        if rec.word.contains('\0') {
            psf.words_with_null_bytes += 1;
        }
        if rec.word.contains('\u{FFFD}') {
            psf.words_with_replacement_chars += 1;
        }
    }

    // Build conflict groups from replayed source records
    let mut grouped: BTreeMap<String, Vec<&AuditableSourceRecord>> = BTreeMap::new();
    for rec in &inputs.replayed_parsed {
        grouped.entry(rec.normalized.clone()).or_default().push(rec);
    }

    let mut conflict_groups = Vec::new();
    for (norm, records) in &grouped {
        if records.len() <= 1 {
            continue;
        }

        let first = &records[0];
        let mut has_real_conflict = false;
        for other in &records[1..] {
            if first.word != other.word
                || first.flags != other.flags
                || first.morphology != other.morphology
                || first.part_of_speech != other.part_of_speech
            {
                has_real_conflict = true;
                break;
            }
        }

        if !has_real_conflict {
            continue;
        }

        let group_records: Vec<ConflictGroupRecord> = records
            .iter()
            .map(|r| ConflictGroupRecord {
                source_line_num: r.source_line_num,
                word: r.word.clone(),
                flags: r.flags.clone(),
                morphology: r.morphology.clone(),
                part_of_speech: r.part_of_speech.clone(),
            })
            .collect();

        // Determine which fields differ
        let mut diffs = BTreeSet::new();
        for other in &records[1..] {
            if first.word != other.word {
                diffs.insert("word");
            }
            if first.flags != other.flags {
                diffs.insert("flags");
            }
            if first.morphology != other.morphology {
                diffs.insert("morphology");
            }
            if first.part_of_speech != other.part_of_speech {
                diffs.insert("part_of_speech");
            }
        }

        conflict_groups.push(ConflictGroup {
            normalized: norm.clone(),
            classification: "metadata_conflict".to_string(),
            records: group_records,
            differences: diffs.into_iter().map(|s| s.to_string()).collect(),
        });
    }

    // Build duplicate groups (exact duplicates: same normalized + same metadata)
    let mut duplicate_groups = Vec::new();
    for (norm, records) in &grouped {
        if records.len() <= 1 {
            continue;
        }

        let first = &records[0];
        let all_identical = records[1..].iter().all(|other| {
            first.word == other.word
                && first.flags == other.flags
                && first.morphology == other.morphology
                && first.part_of_speech == other.part_of_speech
        });

        if all_identical {
            duplicate_groups.push(DuplicateGroup {
                normalized: norm.clone(),
                word: first.word.clone(),
                flags: first.flags.clone(),
                morphology: first.morphology.clone(),
                part_of_speech: first.part_of_speech.clone(),
                occurrence_count: records.len(),
                source_line_nums: records.iter().map(|r| r.source_line_num).collect(),
            });
        }
    }

    // Rejection review
    let rejection_review: Vec<RejectionReview> = inputs
        .replayed_rejected
        .iter()
        .map(|r| RejectionReview {
            source_line_num: r.source_line_num,
            raw_line: r.raw_line.clone(),
            reason_code: r.reason_code.clone(),
            explanation: r.explanation.clone(),
        })
        .collect();

    SourceAnalysis {
        population: "physical_source_records".to_string(),
        physical_lines: inputs.physical_line_count,
        declared_count_line,
        blank_lines,
        dictionary_entry_lines: inputs.replayed_parsed.len() + inputs.replayed_rejected.len(),
        successfully_parsed_records: inputs.replayed_parsed.len(),
        rejected_records: inputs.replayed_rejected.len(),
        raw_line_findings: rlf,
        parsed_surface_findings: psf,
        conflict_groups,
        duplicate_groups,
        rejection_review,
    }
}

// ─── Accepted-record analysis ───────────────────────────────────────────────

/// Character inventory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterEntry {
    pub codepoint: String,
    pub character: String,
    pub general_category: String,
    pub script: String,
    pub occurrences_in_original_word: usize,
    pub occurrences_in_normalized: usize,
    pub entries_containing_in_original: usize,
    pub entries_containing_in_normalized: usize,
}

/// Length distribution statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LengthDistribution {
    pub population: String,
    pub min_scalar: usize,
    pub max_scalar: usize,
    pub mean_scalar_numerator: u64,
    pub mean_scalar_denominator: u64,
    pub mean_scalar_display_4dp: String,
    pub median_scalar: usize,
    pub p10_scalar: usize,
    pub p25_scalar: usize,
    pub p75_scalar: usize,
    pub p90_scalar: usize,
    pub p99_scalar: usize,
    pub min_grapheme: usize,
    pub max_grapheme: usize,
    pub mean_grapheme_numerator: u64,
    pub mean_grapheme_denominator: u64,
    pub mean_grapheme_display_4dp: String,
    pub median_grapheme: usize,
    pub histogram_scalar: BTreeMap<usize, usize>,
}

/// Shape analysis summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShapeAnalysis {
    pub population: String,
    pub total: usize,
    pub punctuation_only: usize,
    pub symbol_only: usize,
    pub digit_only: usize,
    pub no_letters: usize,
    pub uppercase_only: usize,
    pub title_case: usize,
    pub mixed_case: usize,
    pub contains_digits: usize,
    pub contains_hyphen: usize,
    pub contains_apostrophe: usize,
    pub multiword: usize,
    pub very_short_le1: usize,
    pub very_long_gt25: usize,
    pub very_long_gt40: usize,
    pub possible_proper_noun: usize,
}

/// Script analysis summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptAnalysisSummary {
    pub population: String,
    pub by_script: BTreeMap<String, usize>,
}

/// Flag analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagAnalysis {
    pub population: String,
    pub entries_with_flags: usize,
    pub entries_without_flags: usize,
    pub distinct_flag_strings: usize,
    pub flag_frequency: BTreeMap<String, usize>,
}

/// Morphology analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MorphologyAnalysis {
    pub population: String,
    pub entries_with_morphology: usize,
    pub entries_without_morphology: usize,
    pub distinct_morph_keys: usize,
    pub morph_key_frequency: BTreeMap<String, usize>,
    pub pos_frequency: BTreeMap<String, usize>,
}

/// Manual seed comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualSeedComparison {
    pub seed_total: usize,
    pub hunspell_total: usize,
    pub normalized_overlap: usize,
    pub seed_only_forms: Vec<String>,
    pub seed_only_count: usize,
    pub hunspell_only_count: usize,
}

/// Benchmark audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkAudit {
    pub benchmark_size: usize,
    pub unique_inputs: usize,
    pub unique_expected: usize,
    pub expected_in_hunspell: usize,
    pub expected_in_seed: usize,
    pub expected_in_neither: usize,
    pub weaknesses: Vec<String>,
}

/// Suspicious entry for review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspiciousEntry {
    pub normalized: String,
    pub word: String,
    pub source_line_num: usize,
    pub flags: String,
    pub morphology: Vec<String>,
    pub category: String,
    pub reason_code: String,
    pub severity: String,
    pub confidence: String,
    pub explanation: String,
    pub evidence: String,
    pub suggested_action: String,
}

/// All accepted-record analysis results.
pub struct AcceptedAnalysis {
    pub character_inventory: Vec<CharacterEntry>,
    pub length_distribution: LengthDistribution,
    pub shape_analysis: ShapeAnalysis,
    pub script_analysis: ScriptAnalysisSummary,
    pub flag_analysis: FlagAnalysis,
    pub morphology_analysis: MorphologyAnalysis,
    pub manual_seed_comparison: ManualSeedComparison,
    pub benchmark_audit: BenchmarkAudit,
    pub suspicious_entries: Vec<SuspiciousEntry>,
}

/// Runs all analyses on the accepted imported records.
pub fn analyze_accepted_records(inputs: &AuditInputs) -> AcceptedAnalysis {
    let records = &inputs.imported_records;

    // ── Character inventory ─────────────────────────────────────────────
    let mut orig_char_count: BTreeMap<char, usize> = BTreeMap::new();
    let mut norm_char_count: BTreeMap<char, usize> = BTreeMap::new();
    let mut orig_char_entries: BTreeMap<char, BTreeSet<usize>> = BTreeMap::new();
    let mut norm_char_entries: BTreeMap<char, BTreeSet<usize>> = BTreeMap::new();

    for (idx, rec) in records.iter().enumerate() {
        for ch in rec.word.chars() {
            *orig_char_count.entry(ch).or_insert(0) += 1;
            orig_char_entries.entry(ch).or_default().insert(idx);
        }
        for ch in rec.normalized.chars() {
            *norm_char_count.entry(ch).or_insert(0) += 1;
            norm_char_entries.entry(ch).or_default().insert(idx);
        }
    }

    let all_chars: BTreeSet<char> = orig_char_count
        .keys()
        .chain(norm_char_count.keys())
        .copied()
        .collect();

    let character_inventory: Vec<CharacterEntry> = all_chars
        .iter()
        .map(|&ch| CharacterEntry {
            codepoint: format!("U+{:04X}", ch as u32),
            character: ch.to_string(),
            general_category: general_category_str(ch).to_string(),
            script: format!("{:?}", ch.script()),
            occurrences_in_original_word: *orig_char_count.get(&ch).unwrap_or(&0),
            occurrences_in_normalized: *norm_char_count.get(&ch).unwrap_or(&0),
            entries_containing_in_original: orig_char_entries.get(&ch).map_or(0, |s| s.len()),
            entries_containing_in_normalized: norm_char_entries.get(&ch).map_or(0, |s| s.len()),
        })
        .collect();

    // ── Length distribution ──────────────────────────────────────────────
    let length_distribution = compute_length_distribution(records);

    // ── Shape analysis ──────────────────────────────────────────────────
    let mut shape = ShapeAnalysis {
        population: "accepted_records".to_string(),
        total: records.len(),
        punctuation_only: 0,
        symbol_only: 0,
        digit_only: 0,
        no_letters: 0,
        uppercase_only: 0,
        title_case: 0,
        mixed_case: 0,
        contains_digits: 0,
        contains_hyphen: 0,
        contains_apostrophe: 0,
        multiword: 0,
        very_short_le1: 0,
        very_long_gt25: 0,
        very_long_gt40: 0,
        possible_proper_noun: 0,
    };

    for rec in records {
        let flags = classify_shape(&rec.word, &rec.normalized);
        if flags.is_punctuation_only {
            shape.punctuation_only += 1;
        }
        if flags.is_symbol_only {
            shape.symbol_only += 1;
        }
        if flags.is_digit_only {
            shape.digit_only += 1;
        }
        if flags.has_no_letters {
            shape.no_letters += 1;
        }
        if flags.is_uppercase_only {
            shape.uppercase_only += 1;
        }
        if flags.is_title_case {
            shape.title_case += 1;
        }
        if flags.is_mixed_case {
            shape.mixed_case += 1;
        }
        if flags.contains_digits {
            shape.contains_digits += 1;
        }
        if flags.contains_hyphen {
            shape.contains_hyphen += 1;
        }
        if flags.contains_apostrophe {
            shape.contains_apostrophe += 1;
        }
        if flags.is_multiword {
            shape.multiword += 1;
        }
        if flags.is_very_short {
            shape.very_short_le1 += 1;
        }
        if flags.is_very_long_25 {
            shape.very_long_gt25 += 1;
        }
        if flags.is_very_long_40 {
            shape.very_long_gt40 += 1;
        }
        if is_possible_proper_noun(&rec.word) {
            shape.possible_proper_noun += 1;
        }
    }

    // ── Script analysis ─────────────────────────────────────────────────
    let mut script_counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in records {
        let class = classify_script(&rec.normalized);
        *script_counts.entry(class.as_str()).or_insert(0) += 1;
    }
    let script_analysis = ScriptAnalysisSummary {
        population: "accepted_records".to_string(),
        by_script: script_counts,
    };

    // ── Flag analysis ───────────────────────────────────────────────────
    let mut flag_freq: BTreeMap<String, usize> = BTreeMap::new();
    let mut entries_with_flags = 0usize;
    let mut entries_without_flags = 0usize;
    for rec in records {
        if rec.flags.is_empty() {
            entries_without_flags += 1;
        } else {
            entries_with_flags += 1;
            *flag_freq.entry(rec.flags.clone()).or_insert(0) += 1;
        }
    }
    let flag_analysis = FlagAnalysis {
        population: "accepted_records".to_string(),
        entries_with_flags,
        entries_without_flags,
        distinct_flag_strings: flag_freq.len(),
        flag_frequency: flag_freq,
    };

    // ── Morphology analysis ─────────────────────────────────────────────
    let mut morph_key_freq: BTreeMap<String, usize> = BTreeMap::new();
    let mut pos_freq: BTreeMap<String, usize> = BTreeMap::new();
    let mut entries_with_morph = 0usize;
    let mut entries_without_morph = 0usize;
    for rec in records {
        if rec.morphology.is_empty() {
            entries_without_morph += 1;
        } else {
            entries_with_morph += 1;
            for key in &rec.morphology {
                *morph_key_freq.entry(key.clone()).or_insert(0) += 1;
            }
        }
        *pos_freq.entry(rec.part_of_speech.clone()).or_insert(0) += 1;
    }
    let morphology_analysis = MorphologyAnalysis {
        population: "accepted_records".to_string(),
        entries_with_morphology: entries_with_morph,
        entries_without_morphology: entries_without_morph,
        distinct_morph_keys: morph_key_freq.len(),
        morph_key_frequency: morph_key_freq,
        pos_frequency: pos_freq,
    };

    // ── Manual-seed comparison ──────────────────────────────────────────
    let seed_normalized: BTreeSet<String> = inputs
        .manual_seed
        .iter()
        .map(|s| normalize_text(&s.normalized))
        .collect();
    let hunspell_normalized: BTreeSet<String> =
        records.iter().map(|r| r.normalized.clone()).collect();

    let overlap: BTreeSet<&String> = seed_normalized.intersection(&hunspell_normalized).collect();
    let seed_only: Vec<String> = seed_normalized
        .difference(&hunspell_normalized)
        .cloned()
        .collect();
    let hunspell_only_count = hunspell_normalized.difference(&seed_normalized).count();

    let manual_seed_comparison = ManualSeedComparison {
        seed_total: inputs.manual_seed.len(),
        hunspell_total: records.len(),
        normalized_overlap: overlap.len(),
        seed_only_count: seed_only.len(),
        seed_only_forms: seed_only,
        hunspell_only_count,
    };

    // ── Benchmark audit ─────────────────────────────────────────────────
    let benchmark_size = inputs.benchmark_items.len();
    let unique_inputs: BTreeSet<&str> = inputs
        .benchmark_items
        .iter()
        .map(|b| b.input.as_str())
        .collect();
    let unique_expected: BTreeSet<&str> = inputs
        .benchmark_items
        .iter()
        .map(|b| b.expected.as_str())
        .collect();

    let expected_in_hunspell = unique_expected
        .iter()
        .filter(|e| hunspell_normalized.contains(normalize_text(e).as_str()))
        .count();
    let expected_in_seed = unique_expected
        .iter()
        .filter(|e| seed_normalized.contains(normalize_text(e).as_str()))
        .count();
    let expected_in_neither = unique_expected
        .iter()
        .filter(|e| {
            let n = normalize_text(e);
            !hunspell_normalized.contains(n.as_str()) && !seed_normalized.contains(n.as_str())
        })
        .count();

    let mut weaknesses = Vec::new();
    if benchmark_size < 50 {
        weaknesses.push("Sample size too small for statistical significance".to_string());
    }
    if expected_in_seed == unique_expected.len() {
        weaknesses.push("All expected answers present in manual seed".to_string());
    }
    if unique_inputs.len() < benchmark_size {
        weaknesses.push("Benchmark contains duplicate inputs".to_string());
    }

    let benchmark_audit = BenchmarkAudit {
        benchmark_size,
        unique_inputs: unique_inputs.len(),
        unique_expected: unique_expected.len(),
        expected_in_hunspell,
        expected_in_seed,
        expected_in_neither,
        weaknesses,
    };

    // ── Suspicious entries ──────────────────────────────────────────────
    let mut suspicious = Vec::new();

    // Build rare-char set (< 5 entries in normalized field)
    let rare_chars_in_normalized: BTreeSet<char> = norm_char_entries
        .iter()
        .filter(|(_, entries)| entries.len() < 5)
        .map(|(&ch, _)| ch)
        .collect();

    for rec in records {
        let shape_flags = classify_shape(&rec.word, &rec.normalized);
        let script_class = classify_script(&rec.normalized);

        // Punctuation-only
        if shape_flags.is_punctuation_only {
            suspicious.push(SuspiciousEntry {
                normalized: rec.normalized.clone(),
                word: rec.word.clone(),
                source_line_num: rec.source_line_num,
                flags: rec.flags.clone(),
                morphology: rec.morphology.clone(),
                category: "PUNCTUATION_ONLY".to_string(),
                reason_code: "PUNCTUATION_ONLY_ACCEPTED".to_string(),
                severity: "review".to_string(),
                confidence: "high".to_string(),
                explanation: "Entry contains only punctuation characters (Unicode P*)".to_string(),
                evidence: rec
                    .normalized
                    .chars()
                    .map(|c| format!("U+{:04X}", c as u32))
                    .collect::<Vec<_>>()
                    .join(" "),
                suggested_action: "manual_review".to_string(),
            });
        }

        // Symbol-only
        if shape_flags.is_symbol_only {
            suspicious.push(SuspiciousEntry {
                normalized: rec.normalized.clone(),
                word: rec.word.clone(),
                source_line_num: rec.source_line_num,
                flags: rec.flags.clone(),
                morphology: rec.morphology.clone(),
                category: "SYMBOL_ONLY".to_string(),
                reason_code: "SYMBOL_ONLY_ACCEPTED".to_string(),
                severity: "review".to_string(),
                confidence: "high".to_string(),
                explanation: "Entry contains only symbol characters (Unicode S*)".to_string(),
                evidence: rec
                    .normalized
                    .chars()
                    .map(|c| format!("U+{:04X}", c as u32))
                    .collect::<Vec<_>>()
                    .join(" "),
                suggested_action: "manual_review".to_string(),
            });
        }

        // Digit-only
        if shape_flags.is_digit_only {
            suspicious.push(SuspiciousEntry {
                normalized: rec.normalized.clone(),
                word: rec.word.clone(),
                source_line_num: rec.source_line_num,
                flags: rec.flags.clone(),
                morphology: rec.morphology.clone(),
                category: "DIGIT_ONLY".to_string(),
                reason_code: "DIGIT_ONLY_ACCEPTED".to_string(),
                severity: "review".to_string(),
                confidence: "high".to_string(),
                explanation: "Entry contains only digit characters (Unicode N*)".to_string(),
                evidence: rec.normalized.clone(),
                suggested_action: "manual_review".to_string(),
            });
        }

        // No letters (but not already caught above)
        if shape_flags.has_no_letters
            && !shape_flags.is_punctuation_only
            && !shape_flags.is_symbol_only
            && !shape_flags.is_digit_only
        {
            suspicious.push(SuspiciousEntry {
                normalized: rec.normalized.clone(),
                word: rec.word.clone(),
                source_line_num: rec.source_line_num,
                flags: rec.flags.clone(),
                morphology: rec.morphology.clone(),
                category: "NO_LETTERS".to_string(),
                reason_code: "NO_LETTERS_ACCEPTED".to_string(),
                severity: "review".to_string(),
                confidence: "medium".to_string(),
                explanation: "Entry contains no Unicode letter (L*) characters".to_string(),
                evidence: rec.normalized.clone(),
                suggested_action: "manual_review".to_string(),
            });
        }

        // Mixed-script entries
        match &script_class {
            ScriptClass::LatinArabicMixed
            | ScriptClass::LatinCyrillicMixed
            | ScriptClass::OtherMixedScript => {
                suspicious.push(SuspiciousEntry {
                    normalized: rec.normalized.clone(),
                    word: rec.word.clone(),
                    source_line_num: rec.source_line_num,
                    flags: rec.flags.clone(),
                    morphology: rec.morphology.clone(),
                    category: script_class.as_str(),
                    reason_code: "MIXED_SCRIPT_ACCEPTED".to_string(),
                    severity: "review".to_string(),
                    confidence: "medium".to_string(),
                    explanation: format!(
                        "Entry contains letters from multiple scripts: {}",
                        script_class.as_str()
                    ),
                    evidence: rec.normalized.clone(),
                    suggested_action: "manual_review".to_string(),
                });
            }
            _ => {}
        }

        // Very long (grapheme > 25)
        if shape_flags.is_very_long_25 {
            suspicious.push(SuspiciousEntry {
                normalized: rec.normalized.clone(),
                word: rec.word.clone(),
                source_line_num: rec.source_line_num,
                flags: rec.flags.clone(),
                morphology: rec.morphology.clone(),
                category: "VERY_LONG".to_string(),
                reason_code: "VERY_LONG_ENTRY".to_string(),
                severity: "info".to_string(),
                confidence: "low".to_string(),
                explanation: format!(
                    "Entry has {} grapheme clusters (>{} threshold)",
                    shape_flags.grapheme_length, 25
                ),
                evidence: format!(
                    "unicode_scalar_length={}, grapheme_length={}",
                    shape_flags.unicode_scalar_length, shape_flags.grapheme_length
                ),
                suggested_action: "manual_review".to_string(),
            });
        }

        // Rare codepoint in normalized form
        let rare_in_entry: Vec<char> = rec
            .normalized
            .chars()
            .filter(|c| rare_chars_in_normalized.contains(c) && is_unicode_letter(*c))
            .collect();
        if !rare_in_entry.is_empty() {
            let evidence: Vec<String> = rare_in_entry
                .iter()
                .map(|c| format!("U+{:04X} ({})", *c as u32, general_category_str(*c)))
                .collect();
            suspicious.push(SuspiciousEntry {
                normalized: rec.normalized.clone(),
                word: rec.word.clone(),
                source_line_num: rec.source_line_num,
                flags: rec.flags.clone(),
                morphology: rec.morphology.clone(),
                category: "RARE_CODEPOINT".to_string(),
                reason_code: "RARE_LETTER_CODEPOINT".to_string(),
                severity: "info".to_string(),
                confidence: "medium".to_string(),
                explanation: "Entry contains letter code point(s) appearing in <5 entries in normalized field".to_string(),
                evidence: evidence.join(", "),
                suggested_action: "manual_review".to_string(),
            });
        }
    }

    // Sort suspicious entries deterministically
    suspicious.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.normalized.cmp(&b.normalized))
            .then_with(|| a.source_line_num.cmp(&b.source_line_num))
    });

    AcceptedAnalysis {
        character_inventory,
        length_distribution,
        shape_analysis: shape,
        script_analysis,
        flag_analysis,
        morphology_analysis,
        manual_seed_comparison,
        benchmark_audit,
        suspicious_entries: suspicious,
    }
}

// ─── Length distribution helpers ─────────────────────────────────────────────

fn compute_length_distribution(
    records: &[crate::importers::ImportedLexiconRecord],
) -> LengthDistribution {
    use unicode_segmentation::UnicodeSegmentation;

    let mut scalar_lengths: Vec<usize> = records
        .iter()
        .map(|r| r.normalized.chars().count())
        .collect();
    let mut grapheme_lengths: Vec<usize> = records
        .iter()
        .map(|r| r.normalized.graphemes(true).count())
        .collect();

    scalar_lengths.sort_unstable();
    grapheme_lengths.sort_unstable();

    let count = records.len() as u64;
    let scalar_sum: u64 = scalar_lengths.iter().map(|&l| l as u64).sum();
    let grapheme_sum: u64 = grapheme_lengths.iter().map(|&l| l as u64).sum();

    let mut histogram: BTreeMap<usize, usize> = BTreeMap::new();
    for &len in &scalar_lengths {
        *histogram.entry(len).or_insert(0) += 1;
    }

    let mean_scalar_display = if count > 0 {
        format!("{:.4}", scalar_sum as f64 / count as f64)
    } else {
        "0.0000".to_string()
    };
    let mean_grapheme_display = if count > 0 {
        format!("{:.4}", grapheme_sum as f64 / count as f64)
    } else {
        "0.0000".to_string()
    };

    LengthDistribution {
        population: "accepted_records".to_string(),
        min_scalar: *scalar_lengths.first().unwrap_or(&0),
        max_scalar: *scalar_lengths.last().unwrap_or(&0),
        mean_scalar_numerator: scalar_sum,
        mean_scalar_denominator: count,
        mean_scalar_display_4dp: mean_scalar_display,
        median_scalar: percentile(&scalar_lengths, 50),
        p10_scalar: percentile(&scalar_lengths, 10),
        p25_scalar: percentile(&scalar_lengths, 25),
        p75_scalar: percentile(&scalar_lengths, 75),
        p90_scalar: percentile(&scalar_lengths, 90),
        p99_scalar: percentile(&scalar_lengths, 99),
        min_grapheme: *grapheme_lengths.first().unwrap_or(&0),
        max_grapheme: *grapheme_lengths.last().unwrap_or(&0),
        mean_grapheme_numerator: grapheme_sum,
        mean_grapheme_denominator: count,
        mean_grapheme_display_4dp: mean_grapheme_display,
        median_grapheme: percentile(&grapheme_lengths, 50),
        histogram_scalar: histogram,
    }
}

/// Deterministic percentile using nearest-rank method on a sorted slice.
fn percentile(sorted: &[usize], pct: usize) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (pct * sorted.len() / 100).min(sorted.len() - 1);
    sorted[idx]
}
