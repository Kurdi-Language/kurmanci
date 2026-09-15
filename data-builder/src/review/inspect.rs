//! Read-only word inspector for the review workflow (`data-builder inspect-word`).
//!
//! Answers, for the canonical review identity of a word (`normalize_text`), what the
//! repository knows about it: pack membership (seed / reviewed / experimental-full as the
//! authoritative resolver computes it), source membership (Hunspell import, Kuwiki model
//! statistics and review-batch candidates), and the human review history per source.
//!
//! Hard boundaries: this module never assigns, modifies or infers a review status, never
//! promotes a word, never generates lemma, part-of-speech or morphology, never reads corpus
//! text, document ids or context references, and never writes anything. Statuses are shown
//! per source and never flattened into one verdict.

use crate::corpus::registry::CorpusRegistry;
use crate::normalize::normalize_text;
use crate::pack::builder::resolve_authoritative_pack_lexicon;
use crate::pack::language_model::load_language_model;
use crate::pack::policy::PackPolicyConfig;
use crate::review::kuwiki_decisions::load_and_validate_all_kuwiki_decisions;
use crate::review::queues::{EntryQueueRecord, MetadataConflictGroupQueueRecord};
use crate::review::schema::{compute_entry_id, ReviewDecisionRecord};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

pub const HUNSPELL_SOURCE_ID: &str = "kurdish-hunspell-kmr";

/// Pack membership as resolved by the authoritative pack resolver.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Membership {
    pub seed: bool,
    pub reviewed: bool,
    pub experimental_full: bool,
    /// Display forms recorded for the word across the packs it belongs to.
    pub display_forms: Vec<String>,
    /// Source ids recorded on those pack entries.
    pub pack_sources: Vec<String>,
}

/// Presence in the imported Hunspell lexicon.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HunspellSource {
    pub entry_count: usize,
    pub display_forms: Vec<String>,
    pub source_lines: Vec<usize>,
}

/// TRAIN-partition statistics of the committed language model (vocabulary-restricted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KuwikiModelEvidence {
    pub model_id: String,
    pub train_token_count: u64,
    pub train_document_count: u64,
    pub zipf_milli: u32,
}

/// Whole-corpus statistics recorded on a review-batch candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KuwikiBatchEvidence {
    pub batch_id: String,
    pub batch_rank: usize,
    pub token_count: u64,
    pub document_count: u64,
    pub zipf_milli: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Sources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hunspell: Option<HunspellSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kuwiki_model: Option<KuwikiModelEvidence>,
    pub kuwiki_batches: Vec<KuwikiBatchEvidence>,
}

/// One review target (entry or conflict group) of one source, with its human decision if
/// any. `status` is `pending` when the target exists in a queue or batch but has no
/// decision; it is never derived from anything else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewHistoryEntry {
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub display: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_notes: Option<String>,
}

/// Everything the repository records about one word. No corpus text, document ids or
/// context references are part of this structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WordInspection {
    pub input: String,
    pub normalized: String,
    pub membership: Membership,
    pub sources: Sources,
    /// One entry per review target, per source, in source order; statuses are never merged.
    pub review_history: Vec<ReviewHistoryEntry>,
    /// True when at least one human decision exists for the word in any source.
    pub previously_assigned: bool,
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path).map_err(|e| format!("Failed to open {:?}: {}", path, e))?;
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

fn status_name(record: &ReviewDecisionRecord) -> String {
    serde_json::to_value(&record.review_status)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| format!("{:?}", record.review_status))
}

fn history_entry(
    source_id: &str,
    target_type: &str,
    target_id: &str,
    display: &str,
    decision: Option<&ReviewDecisionRecord>,
) -> ReviewHistoryEntry {
    ReviewHistoryEntry {
        source_id: source_id.to_string(),
        target_type: target_type.to_string(),
        target_id: target_id.to_string(),
        display: display.to_string(),
        status: decision
            .map(status_name)
            .unwrap_or_else(|| "pending".to_string()),
        reviewer_id: decision.and_then(|d| d.reviewer_id.clone()),
        review_date: decision.and_then(|d| d.review_date.clone()),
        review_notes: decision.and_then(|d| d.review_notes.clone()),
    }
}

/// Inspects `word` under repository root `root`. Read-only.
pub fn inspect_word<P: AsRef<Path>>(root: P, word: &str) -> Result<WordInspection, String> {
    let root = root.as_ref();
    let normalized = normalize_text(word);
    if normalized.trim().is_empty() || normalized.chars().any(char::is_whitespace) {
        return Err(format!(
            "'{}' does not normalize to a single review token",
            word
        ));
    }

    // 1. Pack membership through the authoritative resolver.
    let mut membership = Membership::default();
    let mut display_forms: BTreeSet<String> = BTreeSet::new();
    let mut pack_sources: BTreeSet<String> = BTreeSet::new();
    for (pack_id, flag) in [
        ("seed", &mut membership.seed),
        ("reviewed", &mut membership.reviewed),
        ("experimental-full", &mut membership.experimental_full),
    ] {
        let entries = resolve_authoritative_pack_lexicon(pack_id, root)?;
        let hits: Vec<_> = entries
            .into_iter()
            .filter(|e| e.normalized == normalized)
            .collect();
        *flag = !hits.is_empty();
        for e in &hits {
            display_forms.insert(e.word.clone());
            pack_sources.extend(e.sources.iter().cloned());
        }
    }
    membership.display_forms = display_forms.into_iter().collect();
    membership.pack_sources = pack_sources.into_iter().collect();

    // 2. Sources.
    let mut sources = Sources::default();
    let hunspell_path = root.join(format!(
        "data/imported/{}/lexicon.jsonl",
        HUNSPELL_SOURCE_ID
    ));
    let hunspell_entries: Vec<serde_json::Value> = read_jsonl(&hunspell_path)?;
    let mut hunspell = HunspellSource::default();
    for e in &hunspell_entries {
        if e["normalized"].as_str() == Some(normalized.as_str()) {
            hunspell.entry_count += 1;
            if let Some(w) = e["word"].as_str() {
                if !hunspell.display_forms.contains(&w.to_string()) {
                    hunspell.display_forms.push(w.to_string());
                }
            }
            if let Some(n) = e["source_line_num"].as_u64() {
                hunspell.source_lines.push(n as usize);
            }
        }
    }
    if hunspell.entry_count > 0 {
        sources.hunspell = Some(hunspell);
    }

    let policy = PackPolicyConfig::load_from_file(root.join("data/pack-policy.toml"))?;
    let model_id = ["reviewed", "experimental-full", "seed"]
        .iter()
        .find_map(|p| policy.packs.get(*p).and_then(|d| d.language_model.clone()));
    if let Some(model_id) = model_id {
        let model = load_language_model(root, &model_id)?;
        if let Some(m) = model.unigrams.get(&normalized) {
            sources.kuwiki_model = Some(KuwikiModelEvidence {
                model_id,
                train_token_count: m.token_count,
                train_document_count: m.document_count,
                zipf_milli: m.zipf_milli,
            });
        }
    }

    // 3. Review history: Hunspell queues and decisions.
    let mut history = Vec::new();
    let decisions_path = root.join(format!(
        "data/review-decisions/{}/decisions.jsonl",
        HUNSPELL_SOURCE_ID
    ));
    let hunspell_decisions: Vec<ReviewDecisionRecord> = read_jsonl(&decisions_path)?;
    let by_target: BTreeMap<&str, &ReviewDecisionRecord> = hunspell_decisions
        .iter()
        .map(|d| (d.target_id.as_str(), d))
        .collect();
    let queues_dir = root.join(format!("data/review-queues/{}", HUNSPELL_SOURCE_ID));
    if queues_dir.exists() {
        let mut names: Vec<String> = fs::read_dir(&queues_dir)
            .map_err(|e| format!("Failed to read {:?}: {}", queues_dir, e))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".jsonl"))
            .collect();
        names.sort();
        let mut seen_targets: BTreeSet<String> = BTreeSet::new();
        for name in names {
            let path = queues_dir.join(&name);
            if name == "metadata-conflict-groups.jsonl" {
                for g in read_jsonl::<MetadataConflictGroupQueueRecord>(&path)? {
                    if g.normalized == normalized && seen_targets.insert(g.target_id.clone()) {
                        let displays: Vec<String> =
                            g.members.iter().map(|m| m.display.clone()).collect();
                        history.push(history_entry(
                            HUNSPELL_SOURCE_ID,
                            "conflict_group",
                            &g.target_id,
                            &displays.join(" | "),
                            by_target.get(g.target_id.as_str()).copied(),
                        ));
                    }
                }
            } else {
                for q in read_jsonl::<EntryQueueRecord>(&path)? {
                    if q.normalized == normalized && seen_targets.insert(q.target_id.clone()) {
                        history.push(history_entry(
                            HUNSPELL_SOURCE_ID,
                            "entry",
                            &q.target_id,
                            &q.display,
                            by_target.get(q.target_id.as_str()).copied(),
                        ));
                    }
                }
            }
        }
    }

    // 4. Review history and evidence: Kuwiki batches (validated snapshots).
    let registry_path = root.join("data/source-registry/corpora.toml");
    let kuwiki_registered = registry_path.exists()
        && CorpusRegistry::load_from_file(&registry_path)?
            .find_corpus("kuwiki")
            .is_some();
    if kuwiki_registered {
        for snapshot in load_and_validate_all_kuwiki_decisions(root)? {
            let by_target: BTreeMap<&str, &ReviewDecisionRecord> = snapshot
                .decisions
                .iter()
                .map(|d| (d.target_id.as_str(), d))
                .collect();
            for cand in snapshot
                .candidates
                .iter()
                .filter(|c| c.normalized_token == normalized)
            {
                sources.kuwiki_batches.push(KuwikiBatchEvidence {
                    batch_id: snapshot.batch_id.clone(),
                    batch_rank: cand.batch_rank,
                    token_count: cand.token_count,
                    document_count: cand.document_count,
                    zipf_milli: cand.zipf_milli,
                });
                let target_id = compute_entry_id(
                    &snapshot.batch_id,
                    &snapshot.candidate_artifact_sha256,
                    &cand.token,
                    &cand.normalized_token,
                    "",
                    &[],
                )?;
                history.push(history_entry(
                    &snapshot.batch_id,
                    "entry",
                    &target_id,
                    &cand.token,
                    by_target.get(target_id.as_str()).copied(),
                ));
            }
        }
    }

    let previously_assigned = history.iter().any(|h| h.status != "pending");
    Ok(WordInspection {
        input: word.to_string(),
        normalized,
        membership,
        sources,
        review_history: history,
        previously_assigned,
    })
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// Human-readable rendering.
pub fn render_text(r: &WordInspection) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "Token:      {}\nNormalized: {}\n\n",
        r.input, r.normalized
    ));
    s.push_str("Membership\n");
    s.push_str(&format!(
        "  seed:              {}\n",
        yes_no(r.membership.seed)
    ));
    s.push_str(&format!(
        "  reviewed:          {}\n",
        yes_no(r.membership.reviewed)
    ));
    s.push_str(&format!(
        "  experimental-full: {}\n",
        yes_no(r.membership.experimental_full)
    ));
    if !r.membership.display_forms.is_empty() {
        s.push_str(&format!(
            "  display forms:     {}\n",
            r.membership.display_forms.join(", ")
        ));
        s.push_str(&format!(
            "  pack sources:      {}\n",
            r.membership.pack_sources.join(", ")
        ));
    }
    s.push_str("\nSources\n");
    match &r.sources.hunspell {
        Some(h) => s.push_str(&format!(
            "  hunspell:          yes ({} entr{}: {}; source lines {})\n",
            h.entry_count,
            if h.entry_count == 1 { "y" } else { "ies" },
            h.display_forms.join(", "),
            h.source_lines
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        None => s.push_str("  hunspell:          no\n"),
    }
    match &r.sources.kuwiki_model {
        Some(k) => s.push_str(&format!(
            "  kuwiki:            yes (model {}: TRAIN tokens {}, TRAIN documents {}, zipf {:.2})\n",
            k.model_id,
            k.train_token_count,
            k.train_document_count,
            k.zipf_milli as f64 / 1000.0
        )),
        None => s.push_str("  kuwiki:            no model statistics (not in the vocabulary-restricted model)\n"),
    }
    for b in &r.sources.kuwiki_batches {
        s.push_str(&format!(
            "  {}: rank {}, corpus tokens {}, corpus documents {}, zipf {:.2}\n",
            b.batch_id,
            b.batch_rank,
            b.token_count,
            b.document_count,
            b.zipf_milli as f64 / 1000.0
        ));
    }
    s.push_str("\nReview history\n");
    s.push_str(&format!(
        "  previously assigned: {}\n",
        yes_no(r.previously_assigned)
    ));
    if r.review_history.is_empty() {
        s.push_str("  decisions:           none (not in any review queue or batch)\n");
    }
    for h in &r.review_history {
        s.push_str(&format!(
            "  {} [{} {}]: {}",
            h.source_id,
            h.target_type,
            &h.target_id[..12],
            h.status
        ));
        if h.display != r.normalized {
            s.push_str(&format!(" (display '{}')", h.display));
        }
        if let (Some(who), Some(when)) = (&h.reviewer_id, &h.review_date) {
            s.push_str(&format!(" by {} on {}", who, when));
        }
        s.push('\n');
        if let Some(notes) = &h.review_notes {
            s.push_str(&format!("      note: {}\n", notes));
        }
    }
    s
}

pub fn render_json(r: &WordInspection) -> String {
    serde_json::to_string_pretty(r).expect("inspection is always serializable")
}
