//! Controlled language pack build transaction manager.

use serde_json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::compile::{compile_entries_to_directory, CompilerModelConfig};
use crate::corpus::importer::LockFileGuard;
use crate::pack::collisions::{
    resolve_collisions, write_collision_report, CollisionResolutionResult,
};
use crate::pack::manifest::{
    calculate_file_sha256, generate_licensing_and_attribution, PackManifest,
    LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION,
};
use crate::pack::policy::PackPolicyConfig;
use crate::pack::selection::{select_candidates_for_pack, SelectionCounts};
use crate::review::merger::load_validated_review_snapshot;
use crate::review::queues::{EntryQueueRecord, MetadataConflictGroupQueueRecord};
use crate::review::schema::ReviewDecisionRecord;
use crate::sources::SourceRegistry;
use crate::validate::SourceLexiconEntry;

fn remove_dir_or_file<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    let p = path.as_ref();
    if p.is_dir() {
        fs::remove_dir_all(p)
    } else if p.exists() || p.symlink_metadata().is_ok() {
        fs::remove_file(p)
    } else {
        Ok(())
    }
}

use crate::pack::manifest::SourceReviewProvenance;
use crate::review::kuwiki_decisions::{
    load_and_validate_kuwiki_decisions, select_kuwiki_candidates_for_pack,
};

/// Payload returned by pure authoritative pack selection and collision resolution.
pub struct AuthoritativePackResolution {
    pub resolved_entries: Vec<SourceLexiconEntry>,
    pub candidate_counts: SelectionCounts,
    pub collision_result: CollisionResolutionResult,
    pub policy_sha256: String,
    pub decisions_sha256: Option<String>,
    pub queue_manifest_sha256: Option<String>,
    pub review_report_manifest_sha256: Option<String>,
    pub source_provenance: Vec<SourceReviewProvenance>,
}

/// Single common pure resolver for authoritative pack selection and collision resolution.
pub fn resolve_authoritative_pack_payload<P: AsRef<Path>>(
    pack_id: &str,
    root_dir: P,
) -> Result<AuthoritativePackResolution, String> {
    let root = root_dir.as_ref();

    let policy_path = root.join("data/pack-policy.toml");
    let policy = PackPolicyConfig::load_from_file(&policy_path)?;
    if !policy.packs.contains_key(pack_id) {
        return Err(format!(
            "Pack ID '{}' not declared in data/pack-policy.toml",
            pack_id
        ));
    }
    let policy_sha256 = calculate_file_sha256(&policy_path)?;

    // Load manual seed entries (single authoritative file)
    let seed_file = root.join("data/reviewed/lexicon.jsonl");
    if !seed_file.exists() {
        return Err(format!(
            "Authoritative manual seed file missing at {:?}",
            seed_file
        ));
    }

    let s_file = File::open(&seed_file).map_err(|e| format!("Failed to open seed file: {}", e))?;
    let mut manual_seed_entries = Vec::new();
    for (l_idx, line_res) in BufReader::new(s_file).lines().enumerate() {
        let line = line_res.map_err(|e| format!("Read error in seed line {}: {}", l_idx + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: SourceLexiconEntry = serde_json::from_str(&line)
            .map_err(|e| format!("JSON error in seed file line {}: {}", l_idx + 1, e))?;
        manual_seed_entries.push(entry);
    }

    let mut decisions = Vec::new();
    let mut entry_queues = BTreeMap::new();
    let mut conflict_group_queues = BTreeMap::new();
    let mut decisions_sha256 = None;
    let mut queue_manifest_sha256 = None;
    let mut review_report_manifest_sha256 = None;
    let mut valid_queue_targets = BTreeSet::new();
    let mut source_provenance = Vec::new();
    let source_id = "kurdish-hunspell-kmr";

    let registry_path = root.join("data/source-registry/sources.toml");
    let registry = SourceRegistry::load_from_file(&registry_path)?;
    let src_entry = registry
        .sources
        .iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| {
            format!(
                "Source '{}' not found in data/source-registry/sources.toml",
                source_id
            )
        })?;
    let _source_revision = &src_entry.version;

    let (raw_candidates, counts) = if pack_id == "seed" {
        select_candidates_for_pack(
            pack_id,
            &manual_seed_entries,
            &entry_queues,
            &conflict_group_queues,
            &decisions,
            &valid_queue_targets,
            source_id,
        )?
    } else {
        // 1. Process Hunspell source
        let review_summary = load_validated_review_snapshot(source_id, root)?;
        decisions_sha256 = Some(review_summary.decision_file_sha256.clone());
        queue_manifest_sha256 = Some(review_summary.provenance.queue_manifest_sha256.clone());

        let reports_dir = root.join("data/reports/controlled-lexicon-review");
        let r_manifest = reports_dir.join("artifacts.sha256");
        let r_manifest_sha = calculate_file_sha256(&r_manifest)?;
        review_report_manifest_sha256 = Some(r_manifest_sha.clone());

        let queues_dir = root.join(format!("data/review-queues/{}", source_id));
        if !queues_dir.exists() {
            return Err(format!(
                "Review queues directory missing at {:?}. Run review pipeline first.",
                queues_dir
            ));
        }

        for entry_res in fs::read_dir(&queues_dir).map_err(|e| e.to_string())? {
            let entry = entry_res.map_err(|e| e.to_string())?;
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".jsonl") {
                let path = queues_dir.join(&fname);
                let f = File::open(&path).map_err(|e| e.to_string())?;

                if fname == "metadata-conflict-groups.jsonl" {
                    for (l_idx, line_res) in BufReader::new(f).lines().enumerate() {
                        let line = line_res.map_err(|e| e.to_string())?;
                        if line.trim().is_empty() {
                            continue;
                        }
                        let rec: MetadataConflictGroupQueueRecord = serde_json::from_str(&line)
                            .map_err(|e| {
                                format!(
                                    "Invalid conflict group queue record in {:?} at line {}: {}",
                                    path,
                                    l_idx + 1,
                                    e
                                )
                            })?;
                        valid_queue_targets
                            .insert(("conflict_group".to_string(), rec.target_id.clone()));
                        conflict_group_queues.insert(rec.target_id.clone(), rec);
                    }
                } else {
                    for (l_idx, line_res) in BufReader::new(f).lines().enumerate() {
                        let line = line_res.map_err(|e| e.to_string())?;
                        if line.trim().is_empty() {
                            continue;
                        }
                        let rec: EntryQueueRecord = serde_json::from_str(&line).map_err(|e| {
                            format!(
                                "Invalid entry queue record in {:?} at line {}: {}",
                                path,
                                l_idx + 1,
                                e
                            )
                        })?;
                        valid_queue_targets.insert(("entry".to_string(), rec.target_id.clone()));
                        entry_queues.insert(rec.target_id.clone(), rec);
                    }
                }
            }
        }

        let decisions_file = root.join(format!(
            "data/review-decisions/{}/decisions.jsonl",
            source_id
        ));
        if !decisions_file.exists() {
            return Err(format!(
                "Review decisions file missing at {:?}. Run review pipeline first.",
                decisions_file
            ));
        }

        let d_file = File::open(&decisions_file).map_err(|e| e.to_string())?;
        for (l_idx, line_res) in BufReader::new(d_file).lines().enumerate() {
            let line = line_res
                .map_err(|e| format!("Read error in decisions line {}: {}", l_idx + 1, e))?;
            if line.trim().is_empty() {
                continue;
            }
            let dec: ReviewDecisionRecord = serde_json::from_str(&line)
                .map_err(|e| format!("JSON error in decisions line {}: {}", l_idx + 1, e))?;
            decisions.push(dec);
        }

        let (mut raw_candidates, mut counts) = select_candidates_for_pack(
            pack_id,
            &manual_seed_entries,
            &entry_queues,
            &conflict_group_queues,
            &decisions,
            &valid_queue_targets,
            source_id,
        )?;

        source_provenance.push(SourceReviewProvenance {
            source_id: source_id.to_string(),
            decisions_sha256: Some(review_summary.decision_file_sha256.clone()),
            candidates_artifact_sha256: None,
            batch_manifest_sha256: None,
            decision_provenance_manifest_sha256: None,
            review_queue_manifest_sha256: Some(
                review_summary.provenance.queue_manifest_sha256.clone(),
            ),
            controlled_review_report_manifest_sha256: Some(r_manifest_sha),
        });

        // 2. Process Kuwiki source (kuwiki-batch-001)
        if let Some(kuwiki_snapshot) = load_and_validate_kuwiki_decisions(root)? {
            let kuwiki_cands =
                select_kuwiki_candidates_for_pack(pack_id, &kuwiki_snapshot, &mut counts)?;
            raw_candidates.extend(kuwiki_cands);

            source_provenance.push(SourceReviewProvenance {
                source_id: "kuwiki-batch-001".to_string(),
                decisions_sha256: Some(kuwiki_snapshot.decision_file_sha256.clone()),
                candidates_artifact_sha256: Some(kuwiki_snapshot.candidate_artifact_sha256.clone()),
                batch_manifest_sha256: Some(kuwiki_snapshot.batch_manifest_sha256.clone()),
                decision_provenance_manifest_sha256: Some(
                    kuwiki_snapshot.decision_provenance_manifest_sha256.clone(),
                ),
                review_queue_manifest_sha256: None,
                controlled_review_report_manifest_sha256: None,
            });
        }

        source_provenance.sort_by(|a, b| a.source_id.cmp(&b.source_id));

        (raw_candidates, counts)
    };

    let collision_result = resolve_collisions(pack_id, raw_candidates)?;

    let resolved_entries = collision_result
        .resolved_entries
        .iter()
        .map(|cand| cand.to_source_lexicon_entry())
        .collect();

    Ok(AuthoritativePackResolution {
        resolved_entries,
        candidate_counts: counts,
        collision_result,
        policy_sha256,
        decisions_sha256,
        queue_manifest_sha256,
        review_report_manifest_sha256,
        source_provenance,
    })
}

/// Pure helper that resolves authoritative lexical entries for `pack_id` ("seed", "reviewed", "experimental-full")
/// using the exact single common resolver `resolve_authoritative_pack_payload`.
pub fn resolve_authoritative_pack_lexicon<P: AsRef<Path>>(
    pack_id: &str,
    root_dir: P,
) -> Result<Vec<SourceLexiconEntry>, String> {
    resolve_authoritative_pack_payload(pack_id, root_dir).map(|res| res.resolved_entries)
}

/// Builds a controlled language pack for `pack_id` under `data/build/packs/<pack_id>/`.
pub fn build_pack<P: AsRef<Path>>(pack_id: &str, root_dir: P) -> Result<PackManifest, String> {
    let root = root_dir.as_ref();

    let lock_path = root.join("data/pack-builds.lock");
    let lock = LockFileGuard::acquire(&lock_path)?;

    let policy_path = root.join("data/pack-policy.toml");
    let policy = PackPolicyConfig::load_from_file(&policy_path)?;
    let pack_def = policy.packs.get(pack_id).ok_or_else(|| {
        format!(
            "Pack ID '{}' not declared in data/pack-policy.toml",
            pack_id
        )
    })?;

    println!("=== Kurmancî Controlled Pack Builder ===");
    println!("  Pack ID:        {}", pack_id);
    println!("  Description:    {}", pack_def.description);
    println!("  Model Profile:  {}", pack_def.model_profile);
    println!("  Is Default:     {}", policy.default_pack == pack_id);
    println!("  Opt In:         {}", pack_def.opt_in);

    // Resolve authoritative pack payload via common pure resolver
    let payload = resolve_authoritative_pack_payload(pack_id, root)?;

    let mut included_sources_set = BTreeSet::new();
    let mut compiler_entries = Vec::new();
    for cand in &payload.collision_result.resolved_entries {
        included_sources_set.insert(cand.source_id.clone());
        compiler_entries.push(cand.to_source_lexicon_entry());
    }

    let included_sources_vec: Vec<String> = included_sources_set.into_iter().collect();
    let (licenses, attribution_text) =
        generate_licensing_and_attribution(root, &included_sources_vec)?;

    // Atomic Staging Layout
    let stage_dir = root.join(format!("data/build/packs/.{}.tmp-stage", pack_id));
    let backup_dir = root.join(format!("data/build/packs/.{}.tmp-backup", pack_id));
    let pack_dir = root.join(format!("data/build/packs/{}", pack_id));

    if stage_dir.exists() {
        remove_dir_or_file(&stage_dir).map_err(|e| format!("Failed to clean stage dir: {}", e))?;
    }
    fs::create_dir_all(&stage_dir).map_err(|e| format!("Failed to create stage dir: {}", e))?;

    let binary_path = compile_entries_to_directory(
        root,
        &compiler_entries,
        &stage_dir,
        CompilerModelConfig::none(),
    )?;
    let binary_bytes = fs::read(&binary_path).map_err(|e| e.to_string())?;
    let binary_sha256 = format!("{:x}", Sha256::digest(&binary_bytes));
    let binary_size_bytes = binary_bytes.len() as u64;

    let mut engine = kurmanci_engine::Engine::new();
    engine.load_binary_pack(&binary_bytes).map_err(|e| {
        format!(
            "Engine load verification failed for compiled pack '{}': {}",
            pack_id, e
        )
    })?;

    let collision_report_path = stage_dir.join("collision-report.jsonl");
    write_collision_report(
        &collision_report_path,
        &payload.collision_result.collision_report_records,
    )?;

    let attribution_path = stage_dir.join("attribution.txt");
    fs::write(&attribution_path, attribution_text)
        .map_err(|e| format!("Failed to write attribution.txt: {}", e))?;

    let manifest = PackManifest {
        schema_version: LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION.to_string(),
        pack_id: pack_id.to_string(),
        pack_format_version: 4,
        language: "ku-Latn".to_string(),
        is_default: policy.default_pack == pack_id,
        is_experimental: pack_def.opt_in,
        model_profile: pack_def.model_profile.clone(),
        frequency_entry_count: 0,
        bigram_count: 0,
        trigram_count: 0,
        manual_seed_selected_count: payload.candidate_counts.manual_seed_selected,
        external_approved_selected_count: payload.candidate_counts.external_approved_selected,
        external_metadata_replacement_selected_count: payload
            .candidate_counts
            .external_metadata_replacement_selected,
        external_experimental_selected_count: payload
            .candidate_counts
            .external_experimental_selected,
        external_unreviewed_selected_count: payload.candidate_counts.external_unreviewed_selected,
        external_excluded_by_status_count: payload
            .candidate_counts
            .external_excluded_by_status_count,
        external_discarded_by_collision_count: payload
            .collision_result
            .external_discarded_by_collision_count,
        final_unique_entry_count: payload.collision_result.resolved_entries.len(),
        pack_policy_sha256: payload.policy_sha256,
        review_decisions_sha256: payload.decisions_sha256,
        review_queue_manifest_sha256: payload.queue_manifest_sha256,
        controlled_review_report_manifest_sha256: payload.review_report_manifest_sha256,
        source_provenance: payload.source_provenance,
        binary_sha256: binary_sha256.clone(),
        binary_size_bytes,
        data_licenses: licenses,
        attribution_files: vec!["attribution.txt".to_string()],
    };

    let manifest_path = stage_dir.join("manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Failed to write manifest.json: {}", e))?;

    let artifact_files = [
        "lexicon.bin",
        "manifest.json",
        "collision-report.jsonl",
        "attribution.txt",
    ];

    let mut manifest_entries = Vec::new();
    for name in &artifact_files {
        let fpath = stage_dir.join(name);
        let content =
            fs::read(&fpath).map_err(|e| format!("Read artifact {:?} failed: {}", fpath, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        let rel_path = format!("data/build/packs/{}/{}", pack_id, name);
        manifest_entries.push(format!("{} {}", hash, rel_path));
    }
    manifest_entries.sort();
    let manifest_bytes = manifest_entries.join("\n") + "\n";
    fs::write(stage_dir.join("artifacts.sha256"), manifest_bytes)
        .map_err(|e| format!("Write artifacts.sha256 failed: {}", e))?;

    if backup_dir.exists() {
        remove_dir_or_file(&backup_dir)
            .map_err(|e| format!("Failed to clean backup dir: {}", e))?;
    }
    if pack_dir.exists() {
        fs::rename(&pack_dir, &backup_dir)
            .map_err(|e| format!("Failed to rename pack dir to backup: {}", e))?;
    }

    match fs::rename(&stage_dir, &pack_dir) {
        Ok(()) => {
            if backup_dir.exists() {
                if let Err(e) = remove_dir_or_file(&backup_dir) {
                    eprintln!(
                        "Warning: failed to clean up backup dir {:?}: {}",
                        backup_dir, e
                    );
                }
            }
        }
        Err(err) => {
            if backup_dir.exists() {
                if let Err(rollback_err) = fs::rename(&backup_dir, &pack_dir) {
                    return Err(format!(
                        "Failed to install pack dir {:?}: {}; rollback also failed: {}",
                        pack_dir, err, rollback_err
                    ));
                }
            }
            return Err(format!(
                "Failed to install pack dir {:?}: {}",
                pack_dir, err
            ));
        }
    }

    println!(
        "⚡ PACK BUILT SUCCESSFULLY under data/build/packs/{}/",
        pack_id
    );
    println!("  Total Entries:  {}", manifest.final_unique_entry_count);
    println!("  Binary SHA-256: {}", manifest.binary_sha256);

    lock.release()?;
    Ok(manifest)
}

/// Builds a temporary frequency-aware binary pack for experiment candidate evaluation without mutating production pack policy or production pack binaries.
pub fn build_temp_frequency_pack<P: AsRef<Path>>(
    pack_id: &str,
    root_dir: P,
    output_binary_path: P,
) -> Result<crate::corpus::join::FrequencyJoinSummaryReport, String> {
    use crate::compile::{compile_binary_pack_with_config, CompilerModelConfig};
    use crate::corpus::join::join_frequencies_to_lexicon;

    let root = root_dir.as_ref();
    let out_p = output_binary_path.as_ref();

    let lock_path = root.join("data/pack-builds.lock");
    let lock = LockFileGuard::acquire(&lock_path)?;

    let payload = resolve_authoritative_pack_payload(pack_id, root)?;
    let mut compiler_entries = payload.resolved_entries;

    let join_report = join_frequencies_to_lexicon(root, &mut compiler_entries)?;

    let config = CompilerModelConfig {
        include_frequencies: true,
        include_bigrams: false,
        include_trigrams: false,
    };

    let binary_bytes = compile_binary_pack_with_config(root, &compiler_entries, config)?;

    if let Some(parent) = out_p.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create output parent dir {:?}: {}", parent, e))?;
    }
    fs::write(out_p, &binary_bytes)
        .map_err(|e| format!("Failed to write candidate binary {:?}: {}", out_p, e))?;

    let mut engine = kurmanci_engine::Engine::new();
    engine
        .load_binary_pack(&binary_bytes)
        .map_err(|e| format!("Engine load failed for temp pack: {}", e))?;

    lock.release()?;
    Ok(join_report)
}
