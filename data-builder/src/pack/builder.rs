//! Controlled language pack build transaction manager (Milestone 4A.2).

use serde_json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::compile::{compile_entries_to_directory, CompilerModelConfig};
use crate::corpus::importer::LockFileGuard;
use crate::pack::collisions::{resolve_collisions, write_collision_report};
use crate::pack::manifest::{
    calculate_file_sha256, generate_licensing_and_attribution, PackManifest,
    LANGUAGE_PACK_MANIFEST_SCHEMA_VERSION,
};
use crate::pack::policy::PackPolicyConfig;
use crate::pack::selection::select_candidates_for_pack;
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

/// Builds a controlled language pack for `pack_id` under `data/build/packs/<pack_id>/`.
pub fn build_pack<P: AsRef<Path>>(pack_id: &str, root_dir: P) -> Result<PackManifest, String> {
    let root = root_dir.as_ref();

    // 1. Acquire global build lock data/pack-builds.lock held for entire transaction
    let lock_path = root.join("data/pack-builds.lock");
    let lock = LockFileGuard::acquire(&lock_path)?;

    // 2. Load and validate pack policy
    let policy_path = root.join("data/pack-policy.toml");
    let policy = PackPolicyConfig::load_from_file(&policy_path)?;
    let pack_def = policy.packs.get(pack_id).ok_or_else(|| {
        format!(
            "Pack ID '{}' not declared in data/pack-policy.toml",
            pack_id
        )
    })?;

    let policy_sha256 = calculate_file_sha256(&policy_path)?;

    println!("=== Kurmancî Controlled Pack Builder ===");
    println!("  Pack ID:        {}", pack_id);
    println!("  Description:    {}", pack_def.description);
    println!("  Model Profile:  {}", pack_def.model_profile);
    println!("  Is Default:     {}", policy.default_pack == pack_id);
    println!("  Opt In:         {}", pack_def.opt_in);

    // 3. Load manual seed entries (Single authoritative path)
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

    // 4. Handle Stale-Input & Review Pipeline Validation
    let mut decisions = Vec::new();
    let mut entry_queues = BTreeMap::new();
    let mut conflict_group_queues = BTreeMap::new();
    let mut decisions_sha256 = None;
    let mut queue_manifest_sha256 = None;
    let mut review_report_manifest_sha256 = None;
    let mut valid_queue_targets = BTreeSet::new();
    let source_id = "kurdish-hunspell-kmr";

    // Load source registry dynamically
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

    if pack_id != "seed" {
        // Load immutable validated snapshot without rewriting reports
        let review_summary = load_validated_review_snapshot(source_id, root)?;
        decisions_sha256 = Some(review_summary.decision_file_sha256.clone());
        queue_manifest_sha256 = Some(review_summary.provenance.queue_manifest_sha256.clone());

        let reports_dir = root.join("data/reports/controlled-lexicon-review");
        let r_manifest = reports_dir.join("artifacts.sha256");
        review_report_manifest_sha256 = Some(calculate_file_sha256(&r_manifest)?);

        // Load typed review queues
        let queues_dir = root.join(format!("data/review-queues/{}", source_id));
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

        // Load decisions file
        let decisions_file = root.join(format!(
            "data/review-decisions/{}/decisions.jsonl",
            source_id
        ));
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
    }

    // 5. Select candidates for pack
    let (raw_candidates, counts) = select_candidates_for_pack(
        pack_id,
        &manual_seed_entries,
        &entry_queues,
        &conflict_group_queues,
        &decisions,
        &valid_queue_targets,
        source_id,
    )?;

    // 6. Resolve collisions
    let collision_result = resolve_collisions(pack_id, raw_candidates)?;

    let mut included_sources_set = BTreeSet::new();
    let mut compiler_entries = Vec::new();
    for cand in &collision_result.resolved_entries {
        included_sources_set.insert(cand.source_id.clone());
        compiler_entries.push(cand.to_source_lexicon_entry());
    }

    let included_sources_vec: Vec<String> = included_sources_set.into_iter().collect();
    let (licenses, attribution_text) =
        generate_licensing_and_attribution(root, &included_sources_vec)?;

    // 7. Atomic Staging Layout
    let stage_dir = root.join(format!("data/build/packs/.{}.tmp-stage", pack_id));
    let backup_dir = root.join(format!("data/build/packs/.{}.tmp-backup", pack_id));
    let pack_dir = root.join(format!("data/build/packs/{}", pack_id));

    if stage_dir.exists() {
        remove_dir_or_file(&stage_dir).map_err(|e| format!("Failed to clean stage dir: {}", e))?;
    }
    fs::create_dir_all(&stage_dir).map_err(|e| format!("Failed to create stage dir: {}", e))?;

    // 8. Invoke compiler with CompilerModelConfig::none()
    let binary_path = compile_entries_to_directory(
        root,
        &compiler_entries,
        &stage_dir,
        CompilerModelConfig::none(),
    )?;
    let binary_bytes = fs::read(&binary_path).map_err(|e| e.to_string())?;
    let binary_sha256 = format!("{:x}", Sha256::digest(&binary_bytes));
    let binary_size_bytes = binary_bytes.len() as u64;

    // Verify engine load
    let mut engine = kurmanci_engine::Engine::new();
    engine.load_binary_pack(&binary_bytes).map_err(|e| {
        format!(
            "Engine load verification failed for compiled pack '{}': {}",
            pack_id, e
        )
    })?;

    // Write collision report
    let collision_report_path = stage_dir.join("collision-report.jsonl");
    write_collision_report(
        &collision_report_path,
        &collision_result.collision_report_records,
    )?;

    // Write attribution.txt
    let attribution_path = stage_dir.join("attribution.txt");
    fs::write(&attribution_path, attribution_text)
        .map_err(|e| format!("Failed to write attribution.txt: {}", e))?;

    // Build pack manifest
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
        manual_seed_selected_count: counts.manual_seed_selected,
        external_approved_selected_count: counts.external_approved_selected,
        external_metadata_replacement_selected_count: counts.external_metadata_replacement_selected,
        external_experimental_selected_count: counts.external_experimental_selected,
        external_unreviewed_selected_count: counts.external_unreviewed_selected,
        external_excluded_by_status_count: counts.external_excluded_by_status_count,
        external_discarded_by_collision_count: collision_result
            .external_discarded_by_collision_count,
        final_unique_entry_count: collision_result.resolved_entries.len(),
        pack_policy_sha256: policy_sha256,
        review_decisions_sha256: decisions_sha256,
        review_queue_manifest_sha256: queue_manifest_sha256,
        controlled_review_report_manifest_sha256: review_report_manifest_sha256,
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

    // 9. Generate self-excluding artifacts.sha256 (Hashes exactly 4 files)
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

    // 10. Atomic Swap
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
