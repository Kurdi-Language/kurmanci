//! Controlled population selection and conflict group resolution.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::review::queues::{EntryQueueRecord, MetadataConflictGroupQueueRecord};
use crate::review::schema::{
    GroupResolution, ReviewDecisionRecord, ReviewDecisionStatus, ReviewTargetType,
};
use crate::validate::SourceLexiconEntry;

/// Population origin for a candidate entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EntryPopulation {
    ManualSeed = 1,
    SeedMetadataChange = 2,
    ExternalApprovedMetadataChange = 3,
    ExternalApproved = 4,
    ExternalExperimentalOnly = 5,
    ExternalUnreviewed = 6,
}

/// Detailed counts collected during candidate selection.
#[derive(Debug, Clone, Default)]
pub struct SelectionCounts {
    pub manual_seed_selected: usize,
    pub external_approved_selected: usize,
    pub external_metadata_replacement_selected: usize,
    pub external_experimental_selected: usize,
    pub external_unreviewed_selected: usize,
    pub external_excluded_by_status_count: usize,
}

/// A selected candidate record before final collision resolution.
#[derive(Debug, Clone)]
pub struct SelectedCandidate {
    pub entry_id: String,
    pub display: String,
    pub normalized: String,
    pub population: EntryPopulation,
    pub source_id: String,
    pub source_lines: Vec<usize>,
    pub flags: String,
    pub morphology: Vec<String>,
    pub part_of_speech: String,
    pub status: String,
}

impl SelectedCandidate {
    pub fn to_source_lexicon_entry(&self) -> SourceLexiconEntry {
        SourceLexiconEntry {
            word: self.display.clone(),
            lemma: self.display.clone(),
            normalized: self.normalized.clone(),
            part_of_speech: self.part_of_speech.clone(),
            frequency: 0,
            status: self.status.clone(),
            variants: vec![],
            sources: vec![self.source_id.clone()],
            regions: vec!["general".to_string()],
            frequency_metadata: None,
        }
    }
}

/// Evaluates human review decisions and filters entries for pack `pack_id`.
pub fn select_candidates_for_pack(
    pack_id: &str,
    manual_seed_entries: &[SourceLexiconEntry],
    entry_queues: &BTreeMap<String, EntryQueueRecord>,
    conflict_group_queues: &BTreeMap<String, MetadataConflictGroupQueueRecord>,
    decisions: &[ReviewDecisionRecord],
    valid_queue_targets: &BTreeSet<(String, String)>,
    source_id: &str,
) -> Result<(Vec<SelectedCandidate>, SelectionCounts), String> {
    let mut candidates = Vec::new();
    let mut consumed_entry_ids = BTreeSet::new();
    let mut counts = SelectionCounts::default();

    // 1. Load manual seed entries (Always included)
    for seed in manual_seed_entries {
        let entry_id = format!("manual-seed:{}", seed.normalized);
        candidates.push(SelectedCandidate {
            entry_id,
            display: seed.word.clone(),
            normalized: seed.normalized.clone(),
            population: EntryPopulation::ManualSeed,
            source_id: "manual-seed".to_string(),
            source_lines: vec![],
            flags: String::new(),
            morphology: vec![],
            part_of_speech: seed.part_of_speech.clone(),
            status: "approved".to_string(),
        });
        counts.manual_seed_selected += 1;
    }

    if pack_id == "seed" {
        return Ok((candidates, counts));
    }

    // Map decision target_id -> ReviewDecisionRecord
    let mut entry_decisions: BTreeMap<String, &ReviewDecisionRecord> = BTreeMap::new();
    let mut group_decisions: BTreeMap<String, &ReviewDecisionRecord> = BTreeMap::new();

    for dec in decisions {
        let t_key = match dec.target_type {
            ReviewTargetType::Entry => ("entry".to_string(), dec.target_id.clone()),
            ReviewTargetType::ConflictGroup => {
                ("conflict_group".to_string(), dec.target_id.clone())
            }
        };

        // Decision must exist in valid queue targets to affect pack
        if !valid_queue_targets.contains(&t_key) {
            continue;
        }

        match dec.target_type {
            ReviewTargetType::Entry => {
                entry_decisions.insert(dec.target_id.clone(), dec);
            }
            ReviewTargetType::ConflictGroup => {
                group_decisions.insert(dec.target_id.clone(), dec);
            }
        }
    }

    // 2. Process ALL conflict groups independently using complete evidence
    for (group_id, group_rec) in conflict_group_queues {
        // Mark all member IDs of this group as consumed so ordinary entry processing skips them
        for m_id in &group_rec.member_entry_ids {
            consumed_entry_ids.insert(m_id.clone());
        }

        let dec_opt = group_decisions.get(group_id).copied();

        if let Some(dec) = dec_opt {
            match dec.review_status {
                ReviewDecisionStatus::Approved => {
                    let res = dec.group_resolution.as_ref().ok_or_else(|| {
                        format!("Ambiguous approved decision for conflict group '{}': missing group_resolution", group_id)
                    })?;

                    match res {
                        GroupResolution::SelectMember { selected_entry_id } => {
                            let member = group_rec
                                .members
                                .iter()
                                .find(|m| &m.entry_id == selected_entry_id)
                                .ok_or_else(|| {
                                    format!(
                                        "Group decision '{}' selected member '{}' which does not belong to group members",
                                        group_id, selected_entry_id
                                    )
                                })?;

                            candidates.push(SelectedCandidate {
                                entry_id: member.entry_id.clone(),
                                display: member.display.clone(),
                                normalized: crate::normalize::normalize_text(&member.display),
                                population: EntryPopulation::ExternalApproved,
                                source_id: source_id.to_string(),
                                source_lines: member.source_lines.clone(),
                                flags: member.flags.clone(),
                                morphology: member.morphology.clone(),
                                part_of_speech: member
                                    .part_of_speech
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                status: "approved".to_string(),
                            });
                            counts.external_approved_selected += 1;
                            counts.external_excluded_by_status_count +=
                                group_rec.members.len().saturating_sub(1);
                        }
                        GroupResolution::ReplaceGroup {
                            replacement_metadata,
                        } => {
                            let repl = replacement_metadata;
                            candidates.push(SelectedCandidate {
                                entry_id: dec.target_id.clone(),
                                display: repl.display.clone(),
                                normalized: repl.normalized.clone(),
                                population: EntryPopulation::ExternalApprovedMetadataChange,
                                source_id: source_id.to_string(),
                                source_lines: vec![],
                                flags: repl.flags.clone().unwrap_or_default(),
                                morphology: repl.morphology.clone().unwrap_or_default(),
                                part_of_speech: repl
                                    .part_of_speech
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                status: "approved_with_metadata_change".to_string(),
                            });
                            counts.external_metadata_replacement_selected += 1;
                            counts.external_excluded_by_status_count += group_rec.members.len();
                        }
                    }
                }
                ReviewDecisionStatus::ApprovedWithMetadataChange => {
                    return Err(format!(
                        "Conflict group decision '{}' uses ApprovedWithMetadataChange; must use Approved with GroupResolution::ReplaceGroup",
                        group_id
                    ));
                }
                ReviewDecisionStatus::ExperimentalOnly => {
                    if pack_id == "experimental-full" {
                        for member in &group_rec.members {
                            candidates.push(SelectedCandidate {
                                entry_id: member.entry_id.clone(),
                                display: member.display.clone(),
                                normalized: crate::normalize::normalize_text(&member.display),
                                population: EntryPopulation::ExternalExperimentalOnly,
                                source_id: source_id.to_string(),
                                source_lines: member.source_lines.clone(),
                                flags: member.flags.clone(),
                                morphology: member.morphology.clone(),
                                part_of_speech: member
                                    .part_of_speech
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                status: "experimental_only".to_string(),
                            });
                            counts.external_experimental_selected += 1;
                        }
                    } else {
                        counts.external_excluded_by_status_count += group_rec.members.len();
                    }
                }
                ReviewDecisionStatus::Unreviewed => {
                    if pack_id == "experimental-full" {
                        for member in &group_rec.members {
                            candidates.push(SelectedCandidate {
                                entry_id: member.entry_id.clone(),
                                display: member.display.clone(),
                                normalized: crate::normalize::normalize_text(&member.display),
                                population: EntryPopulation::ExternalUnreviewed,
                                source_id: source_id.to_string(),
                                source_lines: member.source_lines.clone(),
                                flags: member.flags.clone(),
                                morphology: member.morphology.clone(),
                                part_of_speech: member
                                    .part_of_speech
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                status: "unreviewed".to_string(),
                            });
                            counts.external_unreviewed_selected += 1;
                        }
                    } else {
                        counts.external_excluded_by_status_count += group_rec.members.len();
                    }
                }
                ReviewDecisionStatus::RejectedFromDefaultPack
                | ReviewDecisionStatus::NeedsLinguist
                | ReviewDecisionStatus::NeedsSourceInvestigation => {
                    counts.external_excluded_by_status_count += group_rec.members.len();
                }
            }
        } else {
            // Undecided conflict group (no explicit decision)
            if pack_id == "experimental-full" {
                for member in &group_rec.members {
                    candidates.push(SelectedCandidate {
                        entry_id: member.entry_id.clone(),
                        display: member.display.clone(),
                        normalized: crate::normalize::normalize_text(&member.display),
                        population: EntryPopulation::ExternalUnreviewed,
                        source_id: source_id.to_string(),
                        source_lines: member.source_lines.clone(),
                        flags: member.flags.clone(),
                        morphology: member.morphology.clone(),
                        part_of_speech: member
                            .part_of_speech
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "unreviewed".to_string(),
                    });
                    counts.external_unreviewed_selected += 1;
                }
            } else {
                counts.external_excluded_by_status_count += group_rec.members.len();
            }
        }
    }

    // 3. Process remaining entry queue items using entry_queues as authoritative evidence
    for (entry_id, item) in entry_queues {
        // Skip if entry was already consumed by a conflict group decision
        if consumed_entry_ids.contains(entry_id) {
            continue;
        }

        let entry_decision_opt = entry_decisions.get(entry_id).copied();
        let review_status = entry_decision_opt
            .map(|d| d.review_status.clone())
            .unwrap_or(ReviewDecisionStatus::Unreviewed);

        match pack_id {
            "reviewed" => match review_status {
                ReviewDecisionStatus::Approved => {
                    candidates.push(SelectedCandidate {
                        entry_id: entry_id.clone(),
                        display: item.display.clone(),
                        normalized: item.normalized.clone(),
                        population: EntryPopulation::ExternalApproved,
                        source_id: source_id.to_string(),
                        source_lines: item.source_lines.clone(),
                        flags: item.flags.clone(),
                        morphology: item.morphology.clone(),
                        part_of_speech: item
                            .part_of_speech
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "approved".to_string(),
                    });
                    counts.external_approved_selected += 1;
                }
                ReviewDecisionStatus::ApprovedWithMetadataChange => {
                    let dec = entry_decision_opt.ok_or_else(|| {
                        format!("Missing decision record for approved entry '{}'", entry_id)
                    })?;
                    let repl = dec.replacement_metadata.as_ref().ok_or_else(|| {
                        format!("ApprovedWithMetadataChange decision for '{}' missing replacement_metadata", entry_id)
                    })?;
                    candidates.push(SelectedCandidate {
                        entry_id: entry_id.clone(),
                        display: repl.display.clone(),
                        normalized: repl.normalized.clone(),
                        population: EntryPopulation::ExternalApprovedMetadataChange,
                        source_id: source_id.to_string(),
                        source_lines: item.source_lines.clone(),
                        flags: repl.flags.clone().unwrap_or_else(|| item.flags.clone()),
                        morphology: repl
                            .morphology
                            .clone()
                            .unwrap_or_else(|| item.morphology.clone()),
                        part_of_speech: repl
                            .part_of_speech
                            .clone()
                            .or_else(|| item.part_of_speech.clone())
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "approved_with_metadata_change".to_string(),
                    });
                    counts.external_metadata_replacement_selected += 1;
                }
                _ => {
                    counts.external_excluded_by_status_count += 1;
                }
            },
            "experimental-full" => match review_status {
                ReviewDecisionStatus::Approved => {
                    candidates.push(SelectedCandidate {
                        entry_id: entry_id.clone(),
                        display: item.display.clone(),
                        normalized: item.normalized.clone(),
                        population: EntryPopulation::ExternalApproved,
                        source_id: source_id.to_string(),
                        source_lines: item.source_lines.clone(),
                        flags: item.flags.clone(),
                        morphology: item.morphology.clone(),
                        part_of_speech: item
                            .part_of_speech
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "approved".to_string(),
                    });
                    counts.external_approved_selected += 1;
                }
                ReviewDecisionStatus::ApprovedWithMetadataChange => {
                    let dec = entry_decision_opt.ok_or_else(|| {
                        format!("Missing decision record for approved entry '{}'", entry_id)
                    })?;
                    let repl = dec.replacement_metadata.as_ref().ok_or_else(|| {
                        format!("ApprovedWithMetadataChange decision for '{}' missing replacement_metadata", entry_id)
                    })?;
                    candidates.push(SelectedCandidate {
                        entry_id: entry_id.clone(),
                        display: repl.display.clone(),
                        normalized: repl.normalized.clone(),
                        population: EntryPopulation::ExternalApprovedMetadataChange,
                        source_id: source_id.to_string(),
                        source_lines: item.source_lines.clone(),
                        flags: repl.flags.clone().unwrap_or_else(|| item.flags.clone()),
                        morphology: repl
                            .morphology
                            .clone()
                            .unwrap_or_else(|| item.morphology.clone()),
                        part_of_speech: repl
                            .part_of_speech
                            .clone()
                            .or_else(|| item.part_of_speech.clone())
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "approved_with_metadata_change".to_string(),
                    });
                    counts.external_metadata_replacement_selected += 1;
                }
                ReviewDecisionStatus::ExperimentalOnly => {
                    candidates.push(SelectedCandidate {
                        entry_id: entry_id.clone(),
                        display: item.display.clone(),
                        normalized: item.normalized.clone(),
                        population: EntryPopulation::ExternalExperimentalOnly,
                        source_id: source_id.to_string(),
                        source_lines: item.source_lines.clone(),
                        flags: item.flags.clone(),
                        morphology: item.morphology.clone(),
                        part_of_speech: item
                            .part_of_speech
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "experimental_only".to_string(),
                    });
                    counts.external_experimental_selected += 1;
                }
                ReviewDecisionStatus::Unreviewed => {
                    candidates.push(SelectedCandidate {
                        entry_id: entry_id.clone(),
                        display: item.display.clone(),
                        normalized: item.normalized.clone(),
                        population: EntryPopulation::ExternalUnreviewed,
                        source_id: source_id.to_string(),
                        source_lines: item.source_lines.clone(),
                        flags: item.flags.clone(),
                        morphology: item.morphology.clone(),
                        part_of_speech: item
                            .part_of_speech
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        status: "unreviewed".to_string(),
                    });
                    counts.external_unreviewed_selected += 1;
                }
                _ => {
                    counts.external_excluded_by_status_count += 1;
                }
            },
            _ => return Err(format!("Unknown pack_id '{}'", pack_id)),
        }
    }

    Ok((candidates, counts))
}
