//! Collision resolution and `collision-report.jsonl` generation.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::pack::selection::{EntryPopulation, SelectedCandidate};

/// Details of a competing entry in collision report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetingEntryInfo {
    pub entry_id: String,
    pub display: String,
    pub source_id: String,
    pub source_line: Option<usize>,
    pub population: String,
    pub flags: String,
    pub morphology: Vec<String>,
    pub part_of_speech: String,
    pub status: String,
}

/// Record written to `collision-report.jsonl` for auditability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionReportRecord {
    pub normalized: String,
    pub competing_entries_count: usize,
    pub selected_entry_id: String,
    pub selected_display: String,
    pub selected_population: String,
    pub precedence_rule_used: String,
    pub resolution_type: String,
    pub linguistically_approved: bool,
    pub discarded_entry_ids: Vec<String>,
    pub competing_entries: Vec<CompetingEntryInfo>,
}

/// Result of collision resolution.
#[derive(Debug, Clone)]
pub struct CollisionResolutionResult {
    pub resolved_entries: Vec<SelectedCandidate>,
    pub collision_report_records: Vec<CollisionReportRecord>,
    pub collision_count: usize,
    pub external_discarded_by_collision_count: usize,
}

/// Resolves collisions per normalized key using explicit precedence rules.
pub fn resolve_collisions(
    pack_id: &str,
    candidates: Vec<SelectedCandidate>,
) -> Result<CollisionResolutionResult, String> {
    let mut grouped_by_norm: BTreeMap<String, Vec<SelectedCandidate>> = BTreeMap::new();
    for cand in candidates {
        grouped_by_norm
            .entry(cand.normalized.clone())
            .or_default()
            .push(cand);
    }

    let mut resolved_entries = Vec::new();
    let mut collision_report_records = Vec::new();
    let mut collision_count = 0;
    let mut external_discarded_count = 0;

    for (norm, mut items) in grouped_by_norm {
        if items.len() == 1 {
            resolved_entries.push(items.remove(0));
            continue;
        }

        collision_count += 1;

        // Sort items by precedence
        // Precedence order:
        // 1. ManualSeed (1)
        // 2. SeedMetadataChange (2)
        // 3. ExternalApprovedMetadataChange (3)
        // 4. ExternalApproved (4)
        // 5. ExternalExperimentalOnly (5)
        // 6. ExternalUnreviewed (6)
        // Secondary tie-break: source_line -> display -> flags -> morphology -> entry_id
        items.sort_by(|a, b| {
            a.population
                .cmp(&b.population)
                .then_with(|| a.source_lines.first().cmp(&b.source_lines.first()))
                .then_with(|| a.display.cmp(&b.display))
                .then_with(|| a.flags.cmp(&b.flags))
                .then_with(|| a.morphology.cmp(&b.morphology))
                .then_with(|| a.entry_id.cmp(&b.entry_id))
        });

        // Check if top 2 items have equal priority
        let top_pop = items[0].population;
        let second_pop = items[1].population;

        if top_pop == second_pop && pack_id == "reviewed" {
            return Err(format!(
                "Unresolved equal-priority collision for normalized key '{}' in reviewed pack build ('{}' vs '{}')",
                norm, items[0].entry_id, items[1].entry_id
            ));
        }

        let competing_infos: Vec<CompetingEntryInfo> = items
            .iter()
            .map(|x| CompetingEntryInfo {
                entry_id: x.entry_id.clone(),
                display: x.display.clone(),
                source_id: x.source_id.clone(),
                source_line: x.source_lines.first().copied(),
                population: format!("{:?}", x.population),
                flags: x.flags.clone(),
                morphology: x.morphology.clone(),
                part_of_speech: x.part_of_speech.clone(),
                status: x.status.clone(),
            })
            .collect();

        let selected = items.remove(0);
        let discarded_ids: Vec<String> = items.iter().map(|x| x.entry_id.clone()).collect();

        // Count discarded external entries
        for discarded in items {
            if discarded.source_id != "manual-seed" {
                external_discarded_count += 1;
            }
        }

        let (precedence_rule, res_type, approved) = match selected.population {
            EntryPopulation::ManualSeed => {
                ("manual_seed_priority", "seed_overrides_external", true)
            }
            EntryPopulation::SeedMetadataChange => (
                "seed_metadata_change_priority",
                "seed_metadata_override",
                true,
            ),
            EntryPopulation::ExternalApprovedMetadataChange => (
                "approved_metadata_change_priority",
                "approved_metadata_override",
                true,
            ),
            EntryPopulation::ExternalApproved => {
                ("approved_entry_priority", "approved_entry_selected", true)
            }
            EntryPopulation::ExternalExperimentalOnly | EntryPopulation::ExternalUnreviewed => (
                "experimental_deterministic_tiebreak",
                "experimental_deterministic_tiebreak",
                false,
            ),
        };

        collision_report_records.push(CollisionReportRecord {
            normalized: norm,
            competing_entries_count: competing_infos.len(),
            selected_entry_id: selected.entry_id.clone(),
            selected_display: selected.display.clone(),
            selected_population: format!("{:?}", selected.population),
            precedence_rule_used: precedence_rule.to_string(),
            resolution_type: res_type.to_string(),
            linguistically_approved: approved,
            discarded_entry_ids: discarded_ids,
            competing_entries: competing_infos,
        });

        resolved_entries.push(selected);
    }

    // Sort final entries canonically by normalized then display
    resolved_entries.sort_by(|a, b| {
        a.normalized
            .cmp(&b.normalized)
            .then_with(|| a.display.cmp(&b.display))
    });

    Ok(CollisionResolutionResult {
        resolved_entries,
        collision_report_records,
        collision_count,
        external_discarded_by_collision_count: external_discarded_count,
    })
}

/// Writes `collision-report.jsonl` to destination path.
pub fn write_collision_report<P: AsRef<Path>>(
    path: P,
    records: &[CollisionReportRecord],
) -> Result<(), String> {
    let p = path.as_ref();
    let mut file =
        File::create(p).map_err(|e| format!("Failed to create collision report {:?}: {}", p, e))?;
    for rec in records {
        let json = serde_json::to_string(rec).map_err(|e| e.to_string())?;
        writeln!(file, "{}", json)
            .map_err(|e| format!("Failed to write to collision report {:?}: {}", p, e))?;
    }
    Ok(())
}
