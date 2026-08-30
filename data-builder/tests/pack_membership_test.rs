//! Parity and Invariant Tests for Controlled Pack Selection & Authoritative Membership Resolution.

use data_builder_lib::pack::{
    build_pack, resolve_authoritative_pack_lexicon, resolve_authoritative_pack_payload,
};
use data_builder_lib::review::validate_review_decisions;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir)
}

#[test]
fn test_authoritative_pack_membership_invariants_and_builder_parity() {
    let root = get_workspace_root();

    // Ensure review snapshot is validated
    validate_review_decisions("kurdish-hunspell-kmr", &root)
        .expect("validate_review_decisions failed");

    let seed_entries = resolve_authoritative_pack_lexicon("seed", &root)
        .expect("resolve_authoritative_pack_lexicon seed failed");
    let reviewed_entries = resolve_authoritative_pack_lexicon("reviewed", &root)
        .expect("resolve_authoritative_pack_lexicon reviewed failed");
    let exp_entries = resolve_authoritative_pack_lexicon("experimental-full", &root)
        .expect("resolve_authoritative_pack_lexicon experimental-full failed");

    let seed_payload = resolve_authoritative_pack_payload("seed", &root)
        .expect("resolve_authoritative_pack_payload seed failed");
    let reviewed_payload = resolve_authoritative_pack_payload("reviewed", &root)
        .expect("resolve_authoritative_pack_payload reviewed failed");
    let exp_payload = resolve_authoritative_pack_payload("experimental-full", &root)
        .expect("resolve_authoritative_pack_payload experimental-full failed");

    let seed_count = seed_entries.len();
    let reviewed_count = reviewed_entries.len();
    let exp_count = exp_entries.len();

    println!("Authoritative Membership Scale:");
    println!("  Seed:              {}", seed_count);
    println!("  Reviewed:          {}", reviewed_count);
    println!("  Experimental-Full: {}", exp_count);

    // Hard invariant assertions
    assert!(
        seed_count <= reviewed_count,
        "seed count ({}) must be <= reviewed count ({})",
        seed_count,
        reviewed_count
    );
    assert!(
        reviewed_count <= exp_count,
        "reviewed count ({}) must be <= experimental-full count ({})",
        reviewed_count,
        exp_count
    );

    let seed_normalized: BTreeSet<String> =
        seed_entries.iter().map(|e| e.normalized.clone()).collect();
    let reviewed_normalized: BTreeSet<String> = reviewed_entries
        .iter()
        .map(|e| e.normalized.clone())
        .collect();
    let exp_normalized: BTreeSet<String> =
        exp_entries.iter().map(|e| e.normalized.clone()).collect();

    let seed_payload_norm: BTreeSet<String> = seed_payload
        .resolved_entries
        .iter()
        .map(|e| e.normalized.clone())
        .collect();
    let reviewed_payload_norm: BTreeSet<String> = reviewed_payload
        .resolved_entries
        .iter()
        .map(|e| e.normalized.clone())
        .collect();
    let exp_payload_norm: BTreeSet<String> = exp_payload
        .resolved_entries
        .iter()
        .map(|e| e.normalized.clone())
        .collect();

    // Exact BTreeSet set equality assertions between helper and single pure payload resolver
    assert_eq!(
        seed_normalized, seed_payload_norm,
        "Exact seed normalized set parity check failed"
    );
    assert_eq!(
        reviewed_normalized, reviewed_payload_norm,
        "Exact reviewed normalized set parity check failed"
    );
    assert_eq!(
        exp_normalized, exp_payload_norm,
        "Exact experimental-full normalized set parity check failed"
    );

    // Sub-set containment invariants
    for w in &seed_normalized {
        assert!(
            reviewed_normalized.contains(w),
            "Seed word '{}' must be contained in reviewed pack lexicon",
            w
        );
    }

    for w in &reviewed_normalized {
        assert!(
            exp_normalized.contains(w),
            "Reviewed word '{}' must be contained in experimental-full pack lexicon",
            w
        );
    }

    // Exact parity check for ALL THREE packs against controlled pack builder output
    let seed_manifest = build_pack("seed", &root).expect("build_pack seed failed");
    assert_eq!(
        seed_manifest.final_unique_entry_count,
        seed_normalized.len(),
        "Controlled pack builder and authoritative helper must report identical seed entry count"
    );

    let reviewed_manifest = build_pack("reviewed", &root).expect("build_pack reviewed failed");
    assert_eq!(
        reviewed_manifest.final_unique_entry_count,
        reviewed_normalized.len(),
        "Controlled pack builder and authoritative helper must report identical reviewed entry count"
    );

    let exp_manifest =
        build_pack("experimental-full", &root).expect("build_pack experimental-full failed");
    assert_eq!(
        exp_manifest.final_unique_entry_count,
        exp_normalized.len(),
        "Controlled pack builder and authoritative helper must report identical experimental-full entry count"
    );
}
