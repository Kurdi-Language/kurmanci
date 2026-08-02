//! Integration test suite for Milestone 4A.2 Controlled Pack Policy and Pack Builds.

use data_builder_lib::pack::{
    builder::build_pack, policy::PackPolicyConfig, selection::select_candidates_for_pack,
    PACK_POLICY_SCHEMA_VERSION,
};
use data_builder_lib::review::queues::{
    ConflictGroupMemberEvidence, EntryQueueRecord, MetadataConflictGroupQueueRecord,
};
use data_builder_lib::review::schema::{
    validate_decision_record, GroupResolution, ReplacementMetadata, ReviewDecisionRecord,
    ReviewDecisionStatus, ReviewTargetType,
};
use data_builder_lib::validate::SourceLexiconEntry;
use kurmanci_engine::Engine;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn prepare_test_fixture(dir: &Path) {
    // 1. Manual seed
    let seed_dir = dir.join("data/reviewed");
    fs::create_dir_all(&seed_dir).unwrap();
    let seed_file = seed_dir.join("lexicon.jsonl");
    fs::write(
        &seed_file,
        r#"{"word":"a","lemma":"a","normalized":"a","part_of_speech":"noun","frequency":10,"status":"approved","variants":[],"sources":["manual-seed"],"regions":["general"]}
{"word":"bext","lemma":"bext","normalized":"bext","part_of_speech":"noun","frequency":50,"status":"approved","variants":[],"sources":["manual-seed"],"regions":["general"]}
"#,
    )
    .unwrap();

    // 2. Source registry
    let reg_dir = dir.join("data/source-registry");
    fs::create_dir_all(&reg_dir).unwrap();
    fs::write(
        reg_dir.join("sources.toml"),
        r#"schema_version = "source-registry-v1"

[[sources]]
source_id = "manual-seed"
source_name = "Kurmancî Manually Reviewed Seed Lexicon"
author = "Kurmancî Language Platform Contributors"
license = "Apache-2.0"
license_url = "https://www.apache.org/licenses/LICENSE-2.0"
url = "https://github.com/Kurdi-Language/kurmanci"
version = "0.1.0"
redistribution = "allowed"
notes = "Seed"

[[sources]]
source_id = "kurdish-hunspell-kmr"
source_name = "KurdishHunspell"
source_type = "hunspell"
language = "ku-Latn"
script = "Latn"
author = "KurdishHunspell Team"
license = "CC-BY-SA-4.0"
license_url = "https://spdx.org/licenses/CC-BY-SA-4.0"
url = "https://github.com/hunspell/kmr"
version = "88131d6878ef7fa3ee114aa554adc385ff85b44c"
redistribution = "allowed"
notes = "Test"
"#,
    )
    .unwrap();

    // 3. Pack policy
    let policy_path = dir.join("data/pack-policy.toml");
    fs::create_dir_all(dir.join("data")).unwrap();
    fs::write(
        &policy_path,
        r#"schema_version = "pack-policy-v1"
default_pack = "seed"

[packs.seed]
description = "Manually reviewed seed lexicon only"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.reviewed]
description = "Manual seed plus explicitly approved external entries"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.experimental-full]
description = "Manual seed plus mechanically valid imported entries"
opt_in = true
allow_as_default = false
model_profile = "none"
"#,
    )
    .unwrap();
}

#[test]
fn test_strict_policy_parsing_and_invariants() {
    let temp = tempdir().unwrap();
    prepare_test_fixture(temp.path());

    let policy_path = temp.path().join("data/pack-policy.toml");
    let config = PackPolicyConfig::load_from_file(&policy_path).unwrap();
    assert_eq!(config.schema_version, PACK_POLICY_SCHEMA_VERSION);
    assert_eq!(config.default_pack, "seed");
    assert_eq!(config.packs.len(), 3);

    // Test unknown field rejection
    let bad_policy = temp.path().join("data/bad-policy.toml");
    fs::write(
        &bad_policy,
        r#"schema_version = "pack-policy-v1"
default_pack = "seed"
unknown_field = true

[packs.seed]
description = "Seed"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.reviewed]
description = "Reviewed"
opt_in = false
allow_as_default = true
model_profile = "none"

[packs.experimental-full]
description = "Full"
opt_in = true
allow_as_default = false
model_profile = "none"
"#,
    )
    .unwrap();
    assert!(PackPolicyConfig::load_from_file(&bad_policy).is_err());
}

#[test]
fn test_seed_pack_build_and_engine_loading() {
    let temp = tempdir().unwrap();
    prepare_test_fixture(temp.path());

    let manifest = build_pack("seed", temp.path()).unwrap();
    assert_eq!(manifest.pack_id, "seed");
    assert!(manifest.is_default);
    assert!(!manifest.is_experimental);
    assert_eq!(manifest.model_profile, "none");
    assert_eq!(manifest.final_unique_entry_count, 2);
    assert_eq!(manifest.data_licenses.len(), 1);
    assert_eq!(manifest.data_licenses[0].source_id, "manual-seed");

    // Verify 5 files present in pack directory
    let pack_dir = temp.path().join("data/build/packs/seed");
    let entries: Vec<_> = fs::read_dir(&pack_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries.len(), 5);
    assert!(entries.contains(&"lexicon.bin".to_string()));
    assert!(entries.contains(&"manifest.json".to_string()));
    assert!(entries.contains(&"collision-report.jsonl".to_string()));
    assert!(entries.contains(&"attribution.txt".to_string()));
    assert!(entries.contains(&"artifacts.sha256".to_string()));

    // Verify engine loads binary
    let bin_bytes = fs::read(pack_dir.join("lexicon.bin")).unwrap();
    let mut engine = Engine::new();
    engine.load_binary_pack(&bin_bytes).unwrap();
    assert!(engine.contains("bext"));
}

#[test]
fn test_two_pass_byte_identical_reproducibility() {
    let temp = tempdir().unwrap();
    prepare_test_fixture(temp.path());

    let m1 = build_pack("seed", temp.path()).unwrap();
    let bin1 = fs::read(temp.path().join("data/build/packs/seed/lexicon.bin")).unwrap();

    let m2 = build_pack("seed", temp.path()).unwrap();
    let bin2 = fs::read(temp.path().join("data/build/packs/seed/lexicon.bin")).unwrap();

    assert_eq!(m1.binary_sha256, m2.binary_sha256);
    assert_eq!(bin1, bin2);
}

#[test]
fn test_model_profile_none_excludes_on_disk_ngram_files() {
    let temp = tempdir().unwrap();
    prepare_test_fixture(temp.path());

    // Create synthetic bigrams and trigrams on disk under data/build/
    let build_dir = temp.path().join("data/build");
    fs::create_dir_all(&build_dir).unwrap();
    fs::write(
        build_dir.join("bigrams.jsonl"),
        r#"{"previous":"ez","next":"baş","context_count":10,"count":10,"probability_millionths":1000000}
"#,
    )
    .unwrap();
    fs::write(
        build_dir.join("trigrams.jsonl"),
        r#"{"previous_2":"ez","previous_1":"gelek","next":"baş","context_count":10,"count":10,"probability_millionths":1000000}
"#,
    )
    .unwrap();

    let manifest = build_pack("seed", temp.path()).unwrap();
    assert_eq!(manifest.bigram_count, 0);
    assert_eq!(manifest.trigram_count, 0);
    assert_eq!(manifest.frequency_entry_count, 0);

    let bin_bytes = fs::read(temp.path().join("data/build/packs/seed/lexicon.bin")).unwrap();
    let mut engine = Engine::new();
    engine.load_binary_pack(&bin_bytes).unwrap();
    assert!(engine.predict_next("ez", 5).is_empty());
}

#[test]
fn test_select_conflict_member_absent_from_accepted_lexicon() {
    let manual_seed: Vec<SourceLexiconEntry> = vec![];
    let entry_queues = BTreeMap::new();
    let mut conflict_group_queues = BTreeMap::new();

    let group_id = "group_001".to_string();
    let member_a = ConflictGroupMemberEvidence {
        entry_id: "id_a".to_string(),
        display: "gotar".to_string(),
        source_lines: vec![10],
        flags: "noun".to_string(),
        morphology: vec![],
        part_of_speech: Some("noun".to_string()),
    };
    let member_b = ConflictGroupMemberEvidence {
        entry_id: "id_b".to_string(),
        display: "Gotar".to_string(),
        source_lines: vec![12],
        flags: "noun".to_string(),
        morphology: vec![],
        part_of_speech: Some("noun".to_string()),
    };

    conflict_group_queues.insert(
        group_id.clone(),
        MetadataConflictGroupQueueRecord {
            schema_version: "METADATA_CONFLICT_V1".to_string(),
            rule_id: "metadata_conflict".to_string(),
            rule_version: "1.0.0".to_string(),
            target_type: "conflict_group".to_string(),
            target_id: group_id.clone(),
            normalized: "gotar".to_string(),
            member_entry_ids: vec!["id_a".to_string(), "id_b".to_string()],
            members: vec![member_a, member_b],
            differing_fields: vec!["display".to_string()],
            reason_codes: vec!["case_conflict".to_string()],
            suggested_action: "select".to_string(),
            generated_status: "needs_review".to_string(),
            effective_review_status: "approved".to_string(),
            decision_entry_id: None,
            queue_categories: vec![],
        },
    );

    let mut valid_targets = BTreeSet::new();
    valid_targets.insert(("conflict_group".to_string(), group_id.clone()));

    let decisions = vec![ReviewDecisionRecord {
        schema_version: "review-decision-v1".to_string(),
        source_id: "kurdish-hunspell-kmr".to_string(),
        target_type: ReviewTargetType::ConflictGroup,
        target_id: group_id.clone(),
        review_status: ReviewDecisionStatus::Approved,
        reviewer_id: Some("linguist_1".to_string()),
        review_date: Some("2026-08-01".to_string()),
        review_notes: Some("Select member B".to_string()),
        group_resolution: Some(GroupResolution::SelectMember {
            selected_entry_id: "id_b".to_string(),
        }),
        replacement_metadata: None,
        evidence: vec![],
    }];

    let (candidates, _counts) = select_candidates_for_pack(
        "reviewed",
        &manual_seed,
        &entry_queues,
        &conflict_group_queues,
        &decisions,
        &valid_targets,
        "kurdish-hunspell-kmr",
    )
    .unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].entry_id, "id_b");
    assert_eq!(candidates[0].display, "Gotar");
}

#[test]
fn test_undecided_conflict_group_in_experimental_full() {
    let manual_seed: Vec<SourceLexiconEntry> = vec![];
    let entry_queues = BTreeMap::new();
    let mut conflict_group_queues = BTreeMap::new();

    let group_id = "group_undecided".to_string();
    let m1 = ConflictGroupMemberEvidence {
        entry_id: "id_1".to_string(),
        display: "pirtûk".to_string(),
        source_lines: vec![100],
        flags: "noun".to_string(),
        morphology: vec![],
        part_of_speech: Some("noun".to_string()),
    };
    let m2 = ConflictGroupMemberEvidence {
        entry_id: "id_2".to_string(),
        display: "Pirtûk".to_string(),
        source_lines: vec![105],
        flags: "noun".to_string(),
        morphology: vec![],
        part_of_speech: Some("noun".to_string()),
    };
    let m3 = ConflictGroupMemberEvidence {
        entry_id: "id_3".to_string(),
        display: "PIRTÛK".to_string(),
        source_lines: vec![110],
        flags: "noun".to_string(),
        morphology: vec![],
        part_of_speech: Some("noun".to_string()),
    };

    conflict_group_queues.insert(
        group_id.clone(),
        MetadataConflictGroupQueueRecord {
            schema_version: "METADATA_CONFLICT_V1".to_string(),
            rule_id: "metadata_conflict".to_string(),
            rule_version: "1.0.0".to_string(),
            target_type: "conflict_group".to_string(),
            target_id: group_id.clone(),
            normalized: "pirtuk".to_string(),
            member_entry_ids: vec!["id_1".to_string(), "id_2".to_string(), "id_3".to_string()],
            members: vec![m1, m2, m3],
            differing_fields: vec!["display".to_string()],
            reason_codes: vec!["case_conflict".to_string()],
            suggested_action: "review".to_string(),
            generated_status: "unreviewed".to_string(),
            effective_review_status: "unreviewed".to_string(),
            decision_entry_id: None,
            queue_categories: vec![],
        },
    );

    let valid_targets = BTreeSet::new();
    let decisions = vec![];

    let (candidates, _counts) = select_candidates_for_pack(
        "experimental-full",
        &manual_seed,
        &entry_queues,
        &conflict_group_queues,
        &decisions,
        &valid_targets,
        "kurdish-hunspell-kmr",
    )
    .unwrap();

    // All 3 members must be selected as candidates for experimental-full
    assert_eq!(candidates.len(), 3);

    // Run collision resolution
    let res =
        data_builder_lib::pack::collisions::resolve_collisions("experimental-full", candidates)
            .unwrap();
    assert_eq!(res.resolved_entries.len(), 1);
    assert_eq!(res.collision_report_records.len(), 1);
    assert_eq!(res.collision_report_records[0].competing_entries_count, 3);
    assert_eq!(res.collision_report_records[0].discarded_entry_ids.len(), 2);
}

#[test]
fn test_missing_pos_remains_unknown_not_noun() {
    let manual_seed: Vec<SourceLexiconEntry> = vec![];
    let mut entry_queues = BTreeMap::new();
    let conflict_group_queues = BTreeMap::new();

    entry_queues.insert(
        "entry_nopos".to_string(),
        EntryQueueRecord {
            schema_version: "ENTRY_QUEUE_V1".to_string(),
            rule_id: "rule_1".to_string(),
            rule_version: "1.0.0".to_string(),
            target_type: "entry".to_string(),
            target_id: "entry_nopos".to_string(),
            display: "zanyar".to_string(),
            normalized: "zanyar".to_string(),
            source_id: "kurdish-hunspell-kmr".to_string(),
            source_revision: "88131d6878ef7fa3ee114aa554adc385ff85b44c".to_string(),
            source_lines: vec![1],
            flags: String::new(),
            morphology: vec![],
            part_of_speech: None,
            reason_codes: vec![],
            suggested_action: String::new(),
            generated_status: String::new(),
            effective_review_status: String::new(),
            decision_entry_id: None,
            queue_categories: vec![],
        },
    );

    let valid_targets = BTreeSet::new();
    let decisions = vec![];

    let (candidates, _counts) = select_candidates_for_pack(
        "experimental-full",
        &manual_seed,
        &entry_queues,
        &conflict_group_queues,
        &decisions,
        &valid_targets,
        "kurdish-hunspell-kmr",
    )
    .unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].part_of_speech, "unknown");
    assert_ne!(candidates[0].part_of_speech, "noun");
}

#[test]
fn test_group_approved_with_metadata_change_rejected_as_noncanonical() {
    let record = ReviewDecisionRecord {
        schema_version: "review-decision-v1".to_string(),
        source_id: "kurdish-hunspell-kmr".to_string(),
        target_type: ReviewTargetType::ConflictGroup,
        target_id: "group_001".to_string(),
        review_status: ReviewDecisionStatus::ApprovedWithMetadataChange,
        reviewer_id: Some("linguist_1".to_string()),
        review_date: Some("2026-08-01".to_string()),
        review_notes: None,
        group_resolution: None,
        replacement_metadata: Some(ReplacementMetadata {
            display: "gotar".to_string(),
            normalized: "gotar".to_string(),
            part_of_speech: Some("noun".to_string()),
            flags: None,
            morphology: None,
        }),
        evidence: vec![],
    };

    assert!(validate_decision_record(&record).is_err());
}

#[test]
fn test_explicit_unreviewed_group_decision_handling() {
    let manual_seed: Vec<SourceLexiconEntry> = vec![];
    let entry_queues = BTreeMap::new();
    let mut conflict_group_queues = BTreeMap::new();

    let group_id = "group_explicit_unreviewed".to_string();
    let m1 = ConflictGroupMemberEvidence {
        entry_id: "id_u1".to_string(),
        display: "pênus".to_string(),
        source_lines: vec![200],
        flags: "noun".to_string(),
        morphology: vec![],
        part_of_speech: Some("noun".to_string()),
    };
    let m2 = ConflictGroupMemberEvidence {
        entry_id: "id_u2".to_string(),
        display: "Pênus".to_string(),
        source_lines: vec![205],
        flags: "noun".to_string(),
        morphology: vec![],
        part_of_speech: Some("noun".to_string()),
    };

    conflict_group_queues.insert(
        group_id.clone(),
        MetadataConflictGroupQueueRecord {
            schema_version: "METADATA_CONFLICT_V1".to_string(),
            rule_id: "metadata_conflict".to_string(),
            rule_version: "1.0.0".to_string(),
            target_type: "conflict_group".to_string(),
            target_id: group_id.clone(),
            normalized: "penus".to_string(),
            member_entry_ids: vec!["id_u1".to_string(), "id_u2".to_string()],
            members: vec![m1, m2],
            differing_fields: vec!["display".to_string()],
            reason_codes: vec!["case_conflict".to_string()],
            suggested_action: "review".to_string(),
            generated_status: "unreviewed".to_string(),
            effective_review_status: "unreviewed".to_string(),
            decision_entry_id: None,
            queue_categories: vec![],
        },
    );

    let mut valid_targets = BTreeSet::new();
    valid_targets.insert(("conflict_group".to_string(), group_id.clone()));

    let decisions = vec![ReviewDecisionRecord {
        schema_version: "review-decision-v1".to_string(),
        source_id: "kurdish-hunspell-kmr".to_string(),
        target_type: ReviewTargetType::ConflictGroup,
        target_id: group_id.clone(),
        review_status: ReviewDecisionStatus::Unreviewed,
        reviewer_id: None,
        review_date: None,
        review_notes: None,
        group_resolution: None,
        replacement_metadata: None,
        evidence: vec![],
    }];

    // 1. Reviewed pack build excludes all unreviewed group members
    let (reviewed_cands, _counts) = select_candidates_for_pack(
        "reviewed",
        &manual_seed,
        &entry_queues,
        &conflict_group_queues,
        &decisions,
        &valid_targets,
        "kurdish-hunspell-kmr",
    )
    .unwrap();
    assert_eq!(reviewed_cands.len(), 0);

    // 2. Experimental-full pack includes all members as ExternalUnreviewed candidates
    let (exp_cands, _counts) = select_candidates_for_pack(
        "experimental-full",
        &manual_seed,
        &entry_queues,
        &conflict_group_queues,
        &decisions,
        &valid_targets,
        "kurdish-hunspell-kmr",
    )
    .unwrap();
    assert_eq!(exp_cands.len(), 2);

    // 3. Collision resolution chooses 1 winner and records discarded member with full info
    let res =
        data_builder_lib::pack::collisions::resolve_collisions("experimental-full", exp_cands)
            .unwrap();
    assert_eq!(res.resolved_entries.len(), 1);
    assert_eq!(res.collision_report_records.len(), 1);
    assert_eq!(res.collision_report_records[0].competing_entries.len(), 2);
    assert_eq!(res.collision_report_records[0].discarded_entry_ids.len(), 1);
    assert_eq!(
        res.collision_report_records[0].competing_entries[0].flags,
        "noun"
    );
}
