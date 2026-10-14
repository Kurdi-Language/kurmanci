//! Evaluation-only language pack built from corpus evidence (`evaluation-pack-v1`).
//!
//! Assembles one population for measurement and nothing else: the entries a human approved,
//! plus the still-undecided entries of a source whose normalized form occurs in the canonical
//! TRAIN document set of the committed language model. It exists to answer whether corpus
//! attestation is a good enough membership rule to ship vocabulary that no human has reviewed
//! one by one, by comparing this population against `reviewed` and `experimental-full`.
//!
//! # It is not a pack of this project
//!
//! The pack id is not declared in `data/pack-policy.toml` and deliberately never will be while
//! this is an experiment. It is therefore invisible to `build-pack`, `verify-production-state`,
//! `rebuild-production` and the release bundle, it can never be resolved as a default or
//! opt-in pack by a host, and it is refused an output directory anywhere under
//! `data/build/packs/`. Its manifest records `release_kind = "evaluation"` and
//! `production_eligible = false` so that a copy separated from this directory still says what
//! it is.
//!
//! # Why it is built through the production path
//!
//! Selection, collision resolution, licensing, the language-model join and the binary
//! compilation are the production ones, reached by resolving the authoritative
//! `experimental-full` payload and then *removing* entries from it. Nothing is selected,
//! ranked or compiled by a rule that production does not use, because a population compiled by
//! different rules could not be compared with the packs it is measured against: a difference in
//! the result would not be attributable to the vocabulary.
//!
//! # Membership, by the population the resolver already assigned
//!
//! | population | in the evaluation pack |
//! |---|---|
//! | manual seed, seed metadata change | yes, a human wrote it |
//! | external approved, approved with metadata change | yes, a human approved it |
//! | external experimental only | no, a human decided it is not for the reviewed pack |
//! | external unreviewed | only when TRAIN-attested |
//!
//! No review status is read, written or inferred here beyond the population the authoritative
//! resolver already assigned, and attestation never becomes one: an entry admitted by this rule
//! is exactly as unreviewed afterwards as it was before.

use crate::compile::{compile_binary_pack_from_records, CompileStats};
use crate::pack::language_model::load_language_model;
use crate::pack::manifest::{
    generate_licensing_and_attribution, DataLicenseEntry, LanguageModelProvenance,
};
use crate::pack::policy::PackPolicyConfig;
use crate::pack::resolve_authoritative_pack_payload;
use crate::pack::selection::EntryPopulation;
use crate::review::compute_train_attestation;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub const EVALUATION_PACK_SCHEMA_VERSION: &str = "evaluation-pack-v1";
/// The authoritative pack whose resolved payload is filtered. Its policy also supplies the
/// model profile, so the embedded statistics follow the same rules as the pack this population
/// is compared against.
pub const BASE_PACK_ID: &str = "experimental-full";

/// Whether a resolved candidate belongs to the evaluation population.
///
/// Pure, so the membership rule is testable without a repository: `attested` answers whether
/// the candidate's normalized form occurs in the canonical TRAIN document set.
pub fn in_evaluation_population(population: EntryPopulation, attested: bool) -> bool {
    match population {
        EntryPopulation::ManualSeed
        | EntryPopulation::SeedMetadataChange
        | EntryPopulation::ExternalApproved
        | EntryPopulation::ExternalApprovedMetadataChange => true,
        // A human decided this form is not for the reviewed pack; corpus evidence does not
        // overturn a human decision.
        EntryPopulation::ExternalExperimentalOnly => false,
        EntryPopulation::ExternalUnreviewed => attested,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvaluationPackProvenance {
    pub schema_version: String,
    pub release_kind: String,
    pub production_eligible: bool,
    pub pack_id: String,
    pub base_pack_id: String,
    pub membership_rule: String,
    /// The attestation report this population was built from, verbatim.
    pub attestation: crate::review::TrainAttestationReport,
    pub counts: EvaluationPackCounts,
    pub binary_sha256: String,
    pub language_model_id: Option<String>,
    pub language_model_manifest_sha256: Option<String>,
    pub licenses: Vec<DataLicenseEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EvaluationPackCounts {
    pub base_resolved_entries: usize,
    pub kept_human_approved: usize,
    /// Of the human-approved entries kept, how many carry a form that is also TRAIN-attested.
    /// Present so the attested population closes arithmetically: an attested form that
    /// collides with an approved entry of the same normalized form is represented in the pack
    /// by that approved entry, so it is counted here and not under the undecided entries.
    pub kept_human_approved_also_train_attested: usize,
    pub kept_unreviewed_train_attested: usize,
    pub dropped_unreviewed_unattested: usize,
    pub dropped_experimental_only: usize,
    pub final_entry_count: usize,
    pub frequency_entry_count: usize,
    pub bigram_count: usize,
    pub trigram_count: usize,
    pub binary_bytes: usize,
}

#[derive(Debug)]
pub struct EvaluationPack {
    pub provenance: EvaluationPackProvenance,
    pub binary_bytes: Vec<u8>,
    pub attribution_text: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Drops `.` components so two paths can be compared componentwise. Both sides of any
/// containment check must go through this, never one of them.
fn normalize_path(p: &Path) -> std::path::PathBuf {
    p.components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect()
}

/// Resolves symlinks as far as the path exists, then re-appends the part that does not exist
/// yet. Needed because a containment check between a resolved and an unresolved path silently
/// passes wherever a parent directory is a symlink, which on macOS includes `/var` and so
/// every temporary directory.
fn resolve_existing_prefix(p: &Path) -> std::path::PathBuf {
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut current = normalize_path(p);
    loop {
        if let Ok(resolved) = fs::canonicalize(&current) {
            let mut out = resolved;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return out;
        }
        let name = match current.file_name() {
            Some(n) => n.to_os_string(),
            None => return normalize_path(p),
        };
        match current.parent() {
            Some(parent) if parent != current => {
                missing.push(name);
                current = parent.to_path_buf();
            }
            _ => return normalize_path(p),
        }
    }
}

/// Assembles the population in memory. Writes nothing.
pub fn assemble_evaluation_pack<P: AsRef<Path>>(
    root_dir: P,
    pack_id: &str,
    corpus_id: &str,
    source_id: &str,
    model_id: &str,
) -> Result<EvaluationPack, String> {
    let root = root_dir.as_ref();

    // An evaluation pack must not take the name of a pack the project ships, so that nothing
    // can later mistake one for the other in a path, a report or a conversation.
    let policy = PackPolicyConfig::load_from_file(root.join("data/pack-policy.toml"))?;
    if policy.packs.contains_key(pack_id) {
        return Err(format!(
            "'{}' is a pack declared in data/pack-policy.toml; an evaluation population must not reuse the name of an authoritative pack",
            pack_id
        ));
    }
    let base_def = policy.packs.get(BASE_PACK_ID).ok_or_else(|| {
        format!(
            "base pack '{}' is not declared in data/pack-policy.toml",
            BASE_PACK_ID
        )
    })?;

    // The attestation evidence and the embedded statistics must come from one model,
    // otherwise the population and the prediction behaviour describe different corpora.
    // Checked before the corpus walk, which is the expensive step.
    if base_def.uses_model() {
        let base_model_id = base_def.language_model.as_deref().ok_or_else(|| {
            format!(
                "base pack '{}' declares model_profile '{}' without a language_model",
                BASE_PACK_ID, base_def.model_profile
            )
        })?;
        if base_model_id != model_id {
            return Err(format!(
                "attestation was requested against language model '{}' but base pack '{}' embeds '{}'; both must be the same model",
                model_id, BASE_PACK_ID, base_model_id
            ));
        }
    }

    // 1. The membership evidence, computed by the one shared implementation.
    let attestation = compute_train_attestation(root, corpus_id, source_id, model_id)?;
    let attested = &attestation.attested_pending_normalized;

    // 2. The production payload this population is a subset of.
    let payload = resolve_authoritative_pack_payload(BASE_PACK_ID, root)?;

    // 3. Removal only: every kept candidate is a candidate production already resolved.
    let mut counts = EvaluationPackCounts {
        base_resolved_entries: payload.collision_result.resolved_entries.len(),
        ..Default::default()
    };
    let mut included_sources: BTreeSet<String> = BTreeSet::new();
    let mut compiler_entries = Vec::new();
    for cand in &payload.collision_result.resolved_entries {
        let is_attested = attested.contains(&cand.normalized);
        if !in_evaluation_population(cand.population, is_attested) {
            match cand.population {
                EntryPopulation::ExternalExperimentalOnly => counts.dropped_experimental_only += 1,
                EntryPopulation::ExternalUnreviewed => counts.dropped_unreviewed_unattested += 1,
                _ => unreachable!("every other population is kept"),
            }
            continue;
        }
        match cand.population {
            EntryPopulation::ExternalUnreviewed => counts.kept_unreviewed_train_attested += 1,
            _ => {
                counts.kept_human_approved += 1;
                if is_attested {
                    counts.kept_human_approved_also_train_attested += 1;
                }
            }
        }
        included_sources.insert(cand.source_id.clone());
        compiler_entries.push(cand.to_source_lexicon_entry());
    }
    counts.final_entry_count = compiler_entries.len();
    if compiler_entries.is_empty() {
        return Err(
            "the evaluation population is empty; refusing to compile a pack with no entries"
                .to_string(),
        );
    }

    // 4. Licensing, the language model and the binary: the production functions, with the base
    //    pack's model profile so the embedded statistics follow the same rules.
    let included: Vec<String> = included_sources.into_iter().collect();
    let (mut licenses, mut attribution_text) = generate_licensing_and_attribution(root, &included)?;

    let (binary_bytes, stats, language_model_id, language_model_manifest_sha256): (
        Vec<u8>,
        CompileStats,
        Option<String>,
        Option<String>,
    ) = if base_def.uses_model() {
        let base_model_id = base_def.language_model.as_deref().ok_or_else(|| {
            format!(
                "base pack '{}' declares model_profile '{}' without a language_model",
                BASE_PACK_ID, base_def.model_profile
            )
        })?;
        let model = load_language_model(root, base_model_id)?;
        if base_def.uses_frequencies() {
            for entry in compiler_entries.iter_mut() {
                entry.frequency_metadata = Some(
                    model
                        .unigrams
                        .get(&entry.normalized)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
        }
        let (bigrams, trigrams): (&[_], &[_]) = if base_def.uses_ngrams() {
            (&model.bigrams, &model.trigrams)
        } else {
            (&[], &[])
        };
        let (bytes, stats) = compile_binary_pack_from_records(
            &compiler_entries,
            base_def.uses_frequencies(),
            bigrams,
            trigrams,
        )?;
        let provenance = LanguageModelProvenance::from_model(&model);
        licenses.push(provenance.data_license_entry());
        attribution_text.push_str(&format!(
            "\n=== Evaluation population ===\nThis is an evaluation artifact, not a release pack: \
             it contains entries no human has reviewed individually, admitted because their form \
             occurs in the TRAIN partition of corpus {} ({}). It is not production eligible.\n",
            model.manifest.corpus_id, model.manifest.corpus_version
        ));
        let sha = sha256_hex(
            &fs::read(
                root.join("data/language-model")
                    .join(base_model_id)
                    .join("manifest.json"),
            )
            .map_err(|e| format!("language model manifest unreadable: {}", e))?,
        );
        (bytes, stats, Some(base_model_id.to_string()), Some(sha))
    } else {
        let (bytes, stats) = compile_binary_pack_from_records(&compiler_entries, false, &[], &[])?;
        (bytes, stats, None, None)
    };

    counts.frequency_entry_count = stats.frequency_entry_count;
    counts.bigram_count = stats.bigram_count;
    counts.trigram_count = stats.trigram_count;
    counts.binary_bytes = binary_bytes.len();

    Ok(EvaluationPack {
        provenance: EvaluationPackProvenance {
            schema_version: EVALUATION_PACK_SCHEMA_VERSION.to_string(),
            release_kind: "evaluation".to_string(),
            production_eligible: false,
            pack_id: pack_id.to_string(),
            base_pack_id: BASE_PACK_ID.to_string(),
            membership_rule:
                "human-approved entries, plus still-undecided entries of the source whose normalized form occurs in at least one canonical TRAIN document of the language model's corpus; experimental-only decisions excluded"
                    .to_string(),
            attestation: attestation.report,
            counts,
            binary_sha256: sha256_hex(&binary_bytes),
            language_model_id,
            language_model_manifest_sha256,
            licenses,
        },
        binary_bytes,
        attribution_text,
    })
}

/// Assembles and writes the pack to `out_dir`, which must be outside `data/build/packs/`.
pub fn build_evaluation_pack<P: AsRef<Path>>(
    root_dir: P,
    pack_id: &str,
    corpus_id: &str,
    source_id: &str,
    model_id: &str,
    out_dir: &Path,
) -> Result<EvaluationPackProvenance, String> {
    let root = root_dir.as_ref();

    // The production pack directory is out of bounds: an evaluation artifact must never be
    // able to stand where a release reads its packs from. Both paths are canonicalized and
    // normalized by the same rule before they are compared, because comparing a normalized
    // path against an unnormalized one silently passes: that defect let an earlier version of
    // this function overwrite data/build/packs/reviewed.
    if out_dir
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "refusing an output directory containing '..': {:?}",
            out_dir
        ));
    }
    let canonical_root = fs::canonicalize(root)
        .map_err(|e| format!("repository root {:?} could not be resolved: {}", root, e))?;
    let absolute_out = resolve_existing_prefix(&if out_dir.is_absolute() {
        out_dir.to_path_buf()
    } else {
        canonical_root.join(out_dir)
    });
    let packs_root = resolve_existing_prefix(&canonical_root.join("data/build/packs"));
    // Inside it, equal to it, or containing it: all three would put an evaluation artifact
    // where a production pack is read from.
    if absolute_out.starts_with(&packs_root) || packs_root.starts_with(&absolute_out) {
        return Err(format!(
            "refusing to write an evaluation pack to {:?}: the production pack directory is {:?}",
            absolute_out, packs_root
        ));
    }

    let pack = assemble_evaluation_pack(root, pack_id, corpus_id, source_id, model_id)?;

    fs::create_dir_all(&absolute_out)
        .map_err(|e| format!("Failed to create {:?}: {}", absolute_out, e))?;
    fs::write(absolute_out.join("lexicon.bin"), &pack.binary_bytes)
        .map_err(|e| format!("Failed to write lexicon.bin: {}", e))?;
    fs::write(absolute_out.join("attribution.txt"), &pack.attribution_text)
        .map_err(|e| format!("Failed to write attribution.txt: {}", e))?;
    let mut provenance_json = serde_json::to_string_pretty(&pack.provenance)
        .map_err(|e| format!("Failed to serialize provenance: {}", e))?;
    provenance_json.push('\n');
    fs::write(absolute_out.join("provenance.json"), &provenance_json)
        .map_err(|e| format!("Failed to write provenance.json: {}", e))?;
    Ok(pack.provenance)
}

/// Fixed-width text rendering. Counts and hashes only.
pub fn format_evaluation_pack(p: &EvaluationPackProvenance) -> String {
    let c = &p.counts;
    let mut out = format!(
        "Evaluation pack '{}' ({}, {}) built from '{}'\n  NOT PRODUCTION ELIGIBLE: contains entries no human reviewed individually\n",
        p.pack_id, p.schema_version, p.release_kind, p.base_pack_id
    );
    for (label, value) in [
        ("base resolved entries", c.base_resolved_entries),
        ("kept, human approved", c.kept_human_approved),
        (
            "  of those, also TRAIN-attested",
            c.kept_human_approved_also_train_attested,
        ),
        (
            "kept, unreviewed TRAIN-attested",
            c.kept_unreviewed_train_attested,
        ),
        (
            "dropped, unreviewed unattested",
            c.dropped_unreviewed_unattested,
        ),
        ("dropped, experimental only", c.dropped_experimental_only),
        ("final entries", c.final_entry_count),
        ("frequency entries", c.frequency_entry_count),
        ("bigrams", c.bigram_count),
        ("trigrams", c.trigram_count),
        ("binary bytes", c.binary_bytes),
    ] {
        out.push_str(&format!("  {:<34} {:>9}\n", label, value));
    }
    out.push_str(&format!("  binary sha256 {}\n", p.binary_sha256));
    out
}
