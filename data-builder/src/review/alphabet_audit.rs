//! Lightweight, diagnostic-only character audit against the approved Kurmancî alphabet.
//!
//! Reports, with provenance, where lexical forms outside the 31 letters occur: authoritative
//! decisions that admit such a form as default vocabulary (a contradiction the resolver
//! refuses; must be zero), the Hunspell review pool and its policy-excluded evidence queue,
//! every Kuwiki batch, the built packs. It decides nothing and modifies nothing: nothing it
//! lists becomes a keyboard requirement. Output: `data/reports/alphabet-audit/`.

use crate::alphabet::{
    describe_out_of_alphabet, out_of_alphabet_chars, KURMANCI_ALPHABET, WORD_INTERNAL_PUNCTUATION,
};
use crate::pack::builder::resolve_authoritative_pack_lexicon;
use crate::review::kuwiki_decisions::load_and_validate_all_kuwiki_decisions;
use crate::review::queues::EntryQueueRecord;
use crate::review::schema::{compute_entry_id, ReviewDecisionRecord, ReviewDecisionStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub const ALPHABET_AUDIT_SCHEMA_VERSION: &str = "alphabet-audit-v2";
pub const ALPHABET_AUDIT_DIR: &str = "data/reports/alphabet-audit";
const HUNSPELL_SOURCE_ID: &str = "kurdish-hunspell-kmr";

/// An authoritative decision that admits an out-of-alphabet form as default vocabulary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidApproval {
    pub source_id: String,
    pub target_id: String,
    pub normalized: String,
    pub review_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_date: Option<String>,
    pub outside_characters: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KuwikiOutsideCandidate {
    pub batch_rank: usize,
    pub normalized: String,
    pub review_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_date: Option<String>,
    pub outside_characters: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KuwikiBatchAudit {
    pub batch_id: String,
    pub candidates: usize,
    pub outside_candidates: Vec<KuwikiOutsideCandidate>,
    pub by_review_status: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackAudit {
    pub entries: usize,
    pub outside_forms: usize,
    pub by_character: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HunspellQueueAudit {
    /// Records in the ordinary review pool (`hunspell-only.jsonl`) outside the alphabet;
    /// must be zero after `generate-review-queues`.
    pub review_pool_outside_forms: usize,
    /// Records kept as evidence in `alphabet-policy-excluded.jsonl`.
    pub policy_excluded_records: usize,
    pub policy_excluded_by_character: BTreeMap<String, usize>,
    /// Hyphen/apostrophe forms in the review pool (left to a separate policy; informational).
    pub review_pool_word_internal_punctuation_forms: usize,
    /// Records held for linguist review in `punctuation-policy-needs-linguist.jsonl`
    /// (word-punctuation policy, 2026-09-19).
    #[serde(default)]
    pub punctuation_policy_held_records: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlphabetAuditReport {
    pub schema_version: String,
    pub alphabet: String,
    pub word_internal_punctuation_exempt: String,
    pub policy: String,
    /// Must be empty; the authoritative resolver refuses to build while it is not.
    pub invalid_approvals: Vec<InvalidApproval>,
    pub hunspell_queues: Option<HunspellQueueAudit>,
    pub kuwiki_batches: Vec<KuwikiBatchAudit>,
    pub packs: BTreeMap<String, PackAudit>,
    /// Pack ids whose authoritative resolution failed, with the error (empty when clean).
    pub pack_resolution_errors: BTreeMap<String, String>,
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let file = fs::File::open(path).map_err(|e| format!("Failed to open {:?}: {}", path, e))?;
    let mut out = Vec::new();
    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| format!("Read error in {:?} line {}: {}", path, idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str(&line)
                .map_err(|e| format!("JSON error in {:?} line {}: {}", path, idx + 1, e))?,
        );
    }
    Ok(out)
}

fn character_key(c: char) -> String {
    format!("{} (U+{:04X})", c, c as u32)
}

fn status_name(s: &ReviewDecisionStatus) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

fn admits_default(s: &ReviewDecisionStatus) -> bool {
    matches!(
        s,
        ReviewDecisionStatus::Approved | ReviewDecisionStatus::ApprovedWithMetadataChange
    )
}

fn audit_forms<'a>(forms: impl Iterator<Item = &'a str>) -> PackAudit {
    let mut audit = PackAudit {
        entries: 0,
        outside_forms: 0,
        by_character: BTreeMap::new(),
    };
    for normalized in forms {
        audit.entries += 1;
        let outside = out_of_alphabet_chars(normalized);
        if !outside.is_empty() {
            audit.outside_forms += 1;
            for c in &outside {
                *audit.by_character.entry(character_key(*c)).or_default() += 1;
            }
        }
    }
    audit
}

/// Builds the report from the repository at `root` without modifying anything.
pub fn audit_alphabet<P: AsRef<Path>>(root: P) -> Result<AlphabetAuditReport, String> {
    let root = root.as_ref();
    let mut invalid_approvals = Vec::new();

    // Hunspell: queue records give target id -> normalized form; decisions are checked
    // against them; the review pool must contain no out-of-alphabet form.
    let queues_dir = root.join(format!("data/review-queues/{}", HUNSPELL_SOURCE_ID));
    let mut hunspell_queues = None;
    if queues_dir.is_dir() {
        let mut by_target: BTreeMap<String, String> = BTreeMap::new();
        let mut names: Vec<String> = fs::read_dir(&queues_dir)
            .map_err(|e| format!("Failed to read {:?}: {}", queues_dir, e))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".jsonl") && n != "metadata-conflict-groups.jsonl")
            .collect();
        names.sort();
        let mut audit = HunspellQueueAudit {
            review_pool_outside_forms: 0,
            policy_excluded_records: 0,
            policy_excluded_by_character: BTreeMap::new(),
            review_pool_word_internal_punctuation_forms: 0,
            punctuation_policy_held_records: 0,
        };
        for name in names {
            for rec in read_jsonl::<EntryQueueRecord>(&queues_dir.join(&name))? {
                if name == "hunspell-only.jsonl" {
                    if !out_of_alphabet_chars(&rec.normalized).is_empty() {
                        audit.review_pool_outside_forms += 1;
                    }
                    if rec
                        .normalized
                        .chars()
                        .any(|c| WORD_INTERNAL_PUNCTUATION.contains(&c))
                    {
                        audit.review_pool_word_internal_punctuation_forms += 1;
                    }
                } else if name == crate::alphabet::PUNCTUATION_HELD_QUEUE_FILE {
                    audit.punctuation_policy_held_records += 1;
                } else if name == "alphabet-policy-excluded.jsonl" {
                    audit.policy_excluded_records += 1;
                    for c in out_of_alphabet_chars(&rec.normalized) {
                        *audit
                            .policy_excluded_by_character
                            .entry(character_key(c))
                            .or_default() += 1;
                    }
                }
                by_target.insert(rec.target_id, rec.normalized);
            }
        }
        let decisions_path = root.join(format!(
            "data/review-decisions/{}/decisions.jsonl",
            HUNSPELL_SOURCE_ID
        ));
        if decisions_path.is_file() {
            for d in read_jsonl::<ReviewDecisionRecord>(&decisions_path)? {
                if !admits_default(&d.review_status) {
                    continue;
                }
                let normalized = match d
                    .replacement_metadata
                    .as_ref()
                    .map(|r| r.normalized.clone())
                    .or_else(|| by_target.get(&d.target_id).cloned())
                {
                    Some(n) => n,
                    None => continue,
                };
                let outside = out_of_alphabet_chars(&normalized);
                if !outside.is_empty() {
                    invalid_approvals.push(InvalidApproval {
                        source_id: HUNSPELL_SOURCE_ID.to_string(),
                        target_id: d.target_id.clone(),
                        normalized,
                        review_status: status_name(&d.review_status),
                        reviewer_id: d.reviewer_id.clone(),
                        review_date: d.review_date.clone(),
                        outside_characters: describe_out_of_alphabet(&outside),
                    });
                }
            }
        }
        hunspell_queues = Some(audit);
    }

    // Kuwiki batches: every candidate outside the alphabet with the decision it carries.
    let mut kuwiki_batches = Vec::new();
    for s in load_and_validate_all_kuwiki_decisions(root)? {
        let decisions: BTreeMap<&str, &ReviewDecisionRecord> = s
            .decisions
            .iter()
            .map(|d| (d.target_id.as_str(), d))
            .collect();
        let mut outside_candidates = Vec::new();
        let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
        for c in &s.candidates {
            let outside = out_of_alphabet_chars(&c.normalized_token);
            if outside.is_empty() {
                continue;
            }
            let id = compute_entry_id(
                &s.batch_id,
                &s.candidate_artifact_sha256,
                &c.token,
                &c.normalized_token,
                "",
                &[],
            )?;
            let (status, reviewer, date) = match decisions.get(id.as_str()) {
                Some(d) => {
                    if admits_default(&d.review_status) {
                        invalid_approvals.push(InvalidApproval {
                            source_id: s.batch_id.clone(),
                            target_id: id.clone(),
                            normalized: c.normalized_token.clone(),
                            review_status: status_name(&d.review_status),
                            reviewer_id: d.reviewer_id.clone(),
                            review_date: d.review_date.clone(),
                            outside_characters: describe_out_of_alphabet(&outside),
                        });
                    }
                    (
                        status_name(&d.review_status),
                        d.reviewer_id.clone(),
                        d.review_date.clone(),
                    )
                }
                None => ("undecided".to_string(), None, None),
            };
            *by_status.entry(status.clone()).or_default() += 1;
            outside_candidates.push(KuwikiOutsideCandidate {
                batch_rank: c.batch_rank,
                normalized: c.normalized_token.clone(),
                review_status: status,
                reviewer_id: reviewer,
                review_date: date,
                outside_characters: describe_out_of_alphabet(&outside),
            });
        }
        outside_candidates.sort_by_key(|c| c.batch_rank);
        kuwiki_batches.push(KuwikiBatchAudit {
            batch_id: s.batch_id.clone(),
            candidates: s.candidates.len(),
            outside_candidates,
            by_review_status: by_status,
        });
    }
    kuwiki_batches.sort_by(|a, b| a.batch_id.cmp(&b.batch_id));
    invalid_approvals.sort_by(|a, b| {
        a.source_id
            .cmp(&b.source_id)
            .then(a.normalized.cmp(&b.normalized))
    });

    // Packs: resolution may legitimately fail while a contradiction exists; report it.
    let mut packs = BTreeMap::new();
    let mut pack_resolution_errors = BTreeMap::new();
    for pack_id in ["seed", "reviewed", "experimental-full"] {
        match resolve_authoritative_pack_lexicon(pack_id, root) {
            Ok(entries) => {
                packs.insert(
                    pack_id.to_string(),
                    audit_forms(entries.iter().map(|e| e.normalized.as_str())),
                );
            }
            Err(e) => {
                pack_resolution_errors.insert(pack_id.to_string(), e);
            }
        }
    }

    Ok(AlphabetAuditReport {
        schema_version: ALPHABET_AUDIT_SCHEMA_VERSION.to_string(),
        alphabet: KURMANCI_ALPHABET.iter().collect(),
        word_internal_punctuation_exempt: WORD_INTERNAL_PUNCTUATION.iter().collect(),
        policy: "Default-pack alphabet policy (explicit human project policy, 2026-09-17): a lexical form containing characters outside the approved 31-letter Kurmancî alphabet is ineligible for the default/reviewed pack; such forms are excluded before ordinary review, and an authoritative decision that admits one is a contradiction the resolver refuses. Source records and the experimental-full evidence reservoir keep their evidence. Word-punctuation policy (2026-09-19): a form containing a hyphen or an apostrophe is held for linguist review (punctuation-policy-needs-linguist.jsonl) and is not approved into the default pack until then; possible duplicates that differ only by that punctuation are flagged, never merged. This report is diagnostic only; nothing it lists is a keyboard requirement.".to_string(),
        invalid_approvals,
        hunspell_queues,
        kuwiki_batches,
        packs,
        pack_resolution_errors,
    })
}

/// Renders the Markdown companion of the report.
pub fn render_alphabet_audit_markdown(r: &AlphabetAuditReport) -> String {
    let mut s = String::new();
    s.push_str("# Alphabet audit (diagnostic only)\n\n");
    s.push_str(&format!(
        "Alphabet: `{}`; exempt word-internal punctuation: `{}`.\n\n{}\n\n",
        r.alphabet, r.word_internal_punctuation_exempt, r.policy
    ));
    s.push_str(&format!(
        "## Authoritative decisions admitting an out-of-alphabet form: {}\n\n",
        r.invalid_approvals.len()
    ));
    if !r.invalid_approvals.is_empty() {
        s.push_str("| Form | Source | Decision | Characters |\n|---|---|---|---|\n");
        for x in &r.invalid_approvals {
            s.push_str(&format!(
                "| `{}` | {} | {} ({}, {}) | {} |\n",
                x.normalized,
                x.source_id,
                x.review_status,
                x.reviewer_id.as_deref().unwrap_or("?"),
                x.review_date.as_deref().unwrap_or("?"),
                x.outside_characters
            ));
        }
        s.push('\n');
    }
    if let Some(h) = &r.hunspell_queues {
        s.push_str(&format!(
            "## Hunspell review queues\n\nOrdinary review pool forms outside the alphabet: {} (must be 0). Policy-excluded evidence records: {}{}. Review-pool forms with word-internal hyphen/apostrophe (separate policy): {}.\n\n",
            h.review_pool_outside_forms,
            h.policy_excluded_records,
            if h.policy_excluded_by_character.is_empty() {
                String::new()
            } else {
                format!(
                    " by character: {}",
                    h.policy_excluded_by_character
                        .iter()
                        .map(|(c, n)| format!("{} ×{}", c, n))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
            h.review_pool_word_internal_punctuation_forms
        ));
    }
    for b in &r.kuwiki_batches {
        s.push_str(&format!(
            "## Kuwiki {} ({} candidates)\n\n{} candidates outside the alphabet; by review status: {}\n\n",
            b.batch_id,
            b.candidates,
            b.outside_candidates.len(),
            b.by_review_status
                .iter()
                .map(|(k, v)| format!("{} {}", k, v))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for (pack_id, p) in &r.packs {
        s.push_str(&format!(
            "## Pack {}\n\n{} entries, {} with characters outside the alphabet{}\n\n",
            pack_id,
            p.entries,
            p.outside_forms,
            if p.by_character.is_empty() {
                String::new()
            } else {
                format!(
                    ": {}",
                    p.by_character
                        .iter()
                        .map(|(c, n)| format!("{} ×{}", c, n))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        ));
    }
    for (pack_id, e) in &r.pack_resolution_errors {
        s.push_str(&format!(
            "## Pack {}: resolution refused\n\n```\n{}\n```\n\n",
            pack_id, e
        ));
    }
    s
}

/// Writes `report.json` and `report.md` under `data/reports/alphabet-audit/`; nothing else.
pub fn write_alphabet_audit<P: AsRef<Path>>(root: P) -> Result<AlphabetAuditReport, String> {
    let root = root.as_ref();
    let report = audit_alphabet(root)?;
    let dir = root.join(ALPHABET_AUDIT_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {:?}: {}", dir, e))?;
    fs::write(
        dir.join("report.json"),
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| format!("Failed to write report.json: {}", e))?;
    fs::write(
        dir.join("report.md"),
        render_alphabet_audit_markdown(&report),
    )
    .map_err(|e| format!("Failed to write report.md: {}", e))?;
    Ok(report)
}
