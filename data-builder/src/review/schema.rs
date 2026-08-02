//! Schema definitions and ID calculation for `review-decision-v1`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::normalize::normalize_text;

pub const REVIEW_DECISION_SCHEMA_VERSION: &str = "review-decision-v1";
pub const REVIEW_ENTRY_DOMAIN_PREFIX: &[u8] = b"kurmanci-review-entry-v1";
pub const REVIEW_GROUP_DOMAIN_PREFIX: &[u8] = b"kurmanci-review-group-v1";

/// Target type for human review decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTargetType {
    Entry,
    ConflictGroup,
}

/// Status enum for human review decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecisionStatus {
    Unreviewed,
    Approved,
    ApprovedWithMetadataChange,
    RejectedFromDefaultPack,
    ExperimentalOnly,
    NeedsLinguist,
    NeedsSourceInvestigation,
}

/// Replacement metadata provided when status is `approved_with_metadata_change`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacementMetadata {
    pub display: String,
    pub normalized: String,
    pub flags: Option<String>,
    pub morphology: Option<Vec<String>>,
    pub part_of_speech: Option<String>,
}

/// Explicit resolution strategy for conflict-group decision records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GroupResolution {
    SelectMember {
        selected_entry_id: String,
    },
    ReplaceGroup {
        replacement_metadata: ReplacementMetadata,
    },
}

/// Human review decision record schema (`review-decision-v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDecisionRecord {
    pub schema_version: String,
    pub target_type: ReviewTargetType,
    pub target_id: String,
    pub source_id: String,
    pub review_status: ReviewDecisionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_metadata: Option<ReplacementMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_resolution: Option<GroupResolution>,
}

/// Helper function to update a SHA-256 hasher with a u64 big-endian length-prefixed field.
pub fn hash_field(hasher: &mut Sha256, value: &[u8]) -> Result<(), String> {
    let len = u64::try_from(value.len())
        .map_err(|_| format!("Field length overflow ({} bytes)", value.len()))?;
    hasher.update(len.to_be_bytes());
    hasher.update(value);
    Ok(())
}

/// Computes deterministic mechanical SHA-256 `entry_id` for a source record.
///
/// Hashing sequence:
/// 1. Domain prefix `b"kurmanci-review-entry-v1"`
/// 2. `source_id`
/// 3. `source_revision`
/// 4. `display` (Unicode NFC)
/// 5. `normalized` (`normalize_text` pipeline)
/// 6. `flags`
/// 7. Sorted canonical morphology tags
pub fn compute_entry_id(
    source_id: &str,
    source_revision: &str,
    display: &str,
    normalized: &str,
    flags: &str,
    morphology: &[String],
) -> Result<String, String> {
    let nfc_display: String = display.nfc().collect();
    let norm_word = normalize_text(normalized);

    let mut sorted_morph = morphology.to_vec();
    sorted_morph.sort();

    let mut hasher = Sha256::new();
    hash_field(&mut hasher, REVIEW_ENTRY_DOMAIN_PREFIX)?;
    hash_field(&mut hasher, source_id.as_bytes())?;
    hash_field(&mut hasher, source_revision.as_bytes())?;
    hash_field(&mut hasher, nfc_display.as_bytes())?;
    hash_field(&mut hasher, norm_word.as_bytes())?;
    hash_field(&mut hasher, flags.as_bytes())?;
    for m in &sorted_morph {
        hash_field(&mut hasher, m.as_bytes())?;
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Computes deterministic mechanical SHA-256 `group_id` for a metadata conflict group.
///
/// Hashing sequence:
/// 1. Domain prefix `b"kurmanci-review-group-v1"`
/// 2. `normalized` (`normalize_text` pipeline)
/// 3. Sorted member `entry_id`s
pub fn compute_conflict_group_id(
    normalized: &str,
    member_entry_ids: &[String],
) -> Result<String, String> {
    let norm_word = normalize_text(normalized);
    let mut sorted_members = member_entry_ids.to_vec();
    sorted_members.sort();

    let mut hasher = Sha256::new();
    hash_field(&mut hasher, REVIEW_GROUP_DOMAIN_PREFIX)?;
    hash_field(&mut hasher, norm_word.as_bytes())?;
    for member_id in &sorted_members {
        hash_field(&mut hasher, member_id.as_bytes())?;
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Validates a calendar date string formatted as YYYY-MM-DD.
pub fn validate_review_date(date_str: &str) -> Result<(), String> {
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return Err(format!(
            "Invalid date format '{}': expected YYYY-MM-DD",
            date_str
        ));
    }
    let year: u32 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid year in date '{}'", date_str))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| format!("Invalid month in date '{}'", date_str))?;
    let day: u32 = parts[2]
        .parse()
        .map_err(|_| format!("Invalid day in date '{}'", date_str))?;

    if !(2000..=2100).contains(&year) {
        return Err(format!("Year out of bounds in date '{}'", date_str));
    }
    if !(1..=12).contains(&month) {
        return Err(format!("Invalid month in date '{}'", date_str));
    }
    let max_days = match month {
        2 => {
            let is_leap =
                (year / 4 * 4 == year && year / 100 * 100 != year) || (year / 400 * 400 == year);
            if is_leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=max_days).contains(&day) {
        return Err(format!("Invalid day in date '{}'", date_str));
    }
    Ok(())
}

/// Validates a `ReviewDecisionRecord` against schema rules.
pub fn validate_decision_record(record: &ReviewDecisionRecord) -> Result<(), String> {
    if record.schema_version != REVIEW_DECISION_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported schema_version '{}': expected '{}'",
            record.schema_version, REVIEW_DECISION_SCHEMA_VERSION
        ));
    }

    if record.target_id.trim().is_empty() {
        return Err("Target ID must not be empty".to_string());
    }
    if record.source_id.trim().is_empty() {
        return Err("Source ID must not be empty".to_string());
    }

    match record.review_status {
        ReviewDecisionStatus::Unreviewed => {
            if record.reviewer_id.is_some() {
                return Err("Unreviewed decision must not contain reviewer_id".to_string());
            }
            if record.review_date.is_some() {
                return Err("Unreviewed decision must not contain review_date".to_string());
            }
            if record.review_notes.is_some() {
                return Err("Unreviewed decision must not contain review_notes".to_string());
            }
            if record.replacement_metadata.is_some() {
                return Err("Unreviewed decision must not contain replacement_metadata".to_string());
            }
        }
        _ => {
            let reviewer = record.reviewer_id.as_deref().unwrap_or("");
            if reviewer.trim().is_empty() {
                return Err(format!(
                    "Reviewed status '{:?}' requires a non-empty reviewer_id",
                    record.review_status
                ));
            }
            let date = record.review_date.as_deref().ok_or_else(|| {
                format!(
                    "Reviewed status '{:?}' requires review_date (YYYY-MM-DD)",
                    record.review_status
                )
            })?;
            validate_review_date(date)?;
        }
    }

    if record.target_type == ReviewTargetType::ConflictGroup {
        if record.review_status == ReviewDecisionStatus::ApprovedWithMetadataChange {
            return Err(
                "Conflict group decisions must not use ApprovedWithMetadataChange; use Approved with GroupResolution::ReplaceGroup inside group_resolution instead"
                    .to_string(),
            );
        }
        if record.replacement_metadata.is_some() {
            return Err(
                "Conflict group decisions must not use top-level replacement_metadata; use GroupResolution::ReplaceGroup inside group_resolution instead"
                    .to_string(),
            );
        }
        if record.review_status == ReviewDecisionStatus::Approved
            && record.group_resolution.is_none()
        {
            return Err(
                "Conflict group approved decision must specify group_resolution (SelectMember or ReplaceGroup)"
                    .to_string(),
            );
        }
    }

    if record.review_status != ReviewDecisionStatus::ApprovedWithMetadataChange
        && record.replacement_metadata.is_some()
    {
        return Err(format!(
            "Status '{:?}' prohibits replacement_metadata (only approved_with_metadata_change permits replacement fields)",
            record.review_status
        ));
    }

    match record.review_status {
        ReviewDecisionStatus::Approved => {}
        ReviewDecisionStatus::ApprovedWithMetadataChange => {
            let repl = record.replacement_metadata.as_ref().ok_or_else(|| {
                "approved_with_metadata_change requires replacement_metadata".to_string()
            })?;

            if repl.display.trim().is_empty() {
                return Err("replacement_metadata.display must not be empty".to_string());
            }
            let expected_norm = normalize_text(&repl.display);
            if repl.normalized != expected_norm {
                return Err(format!(
                    "replacement_metadata.normalized '{}' inconsistent with normalized display '{}'",
                    repl.normalized, expected_norm
                ));
            }

            let test_entry = crate::validate::SourceLexiconEntry {
                word: repl.display.clone(),
                lemma: repl.display.clone(),
                normalized: repl.normalized.clone(),
                part_of_speech: repl
                    .part_of_speech
                    .clone()
                    .unwrap_or_else(|| "noun".to_string()),
                frequency: 0,
                status: "approved_with_metadata_change".to_string(),
                variants: vec![],
                sources: vec![record.source_id.clone()],
                regions: vec!["general".to_string()],
                frequency_metadata: None,
            };
            crate::validate::validate_entry(&test_entry, 1)
                .map_err(|e| format!("Replacement metadata failed lexicon validation: {}", e))?;
        }
        ReviewDecisionStatus::RejectedFromDefaultPack => {
            let has_notes = record
                .review_notes
                .as_ref()
                .map(|n| !n.trim().is_empty())
                .unwrap_or(false);
            let has_evidence = !record.evidence.is_empty();
            if !has_notes && !has_evidence {
                return Err(
                    "rejected_from_default_pack requires non-empty review_notes or evidence"
                        .to_string(),
                );
            }
        }
        _ => {}
    }

    Ok(())
}
