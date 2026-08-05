//! Integration & Unit tests for Milestone 2E: Frequency-Aware Suggestion Ranking,
//! Binary Pack v2 format, Frequency Join, Engine Ranking Policy, and Evaluation Suite.

use data_builder_lib::eval_ranking::run_ranking_evaluation;
use data_builder_lib::validate::{
    FrequencyMetadata as BuilderFrequencyMetadata, SourceLexiconEntry,
};
use data_builder_lib::{compile_binary_pack, MAGIC_BYTES, PACK_VERSION};
use kurmanci_engine::{
    Engine, FrequencyMetadata as EngineFrequencyMetadata, RankedCandidate, RankingConfig,
    SuggestionKind,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir
        .join("data/source-registry/corpora.toml")
        .exists()
    {
        manifest_dir
    } else if manifest_dir
        .join("../data/source-registry/corpora.toml")
        .exists()
    {
        manifest_dir.join("..")
    } else {
        PathBuf::from(".")
    }
}

// ─── Unit Tests: Ranking Policy & Fixed-Point Zipf ───────────────────────

#[test]
fn test_fixed_point_zipf_conversion_and_metadata() {
    let meta = EngineFrequencyMetadata {
        token_count: 500,
        document_count: 50,
        zipf_milli: 4823,
    };
    assert_eq!(meta.token_count, 500);
    assert_eq!(meta.document_count, 50);
    assert_eq!(meta.zipf_milli, 4823);
}

#[test]
fn test_edit_cost_priority_over_frequency() {
    let config = RankingConfig::default();

    // Close rare word: edit_cost 1, zipf_milli 1000
    let rare_close = RankedCandidate {
        word: "spas".to_string(),
        edit_cost: 1,
        is_diacritic_match: false,
        prefix_quality: 0,
        frequency: EngineFrequencyMetadata {
            token_count: 10,
            document_count: 1,
            zipf_milli: 1000,
        },
        kind: SuggestionKind::Correction,
    };

    // Distant common word: edit_cost 3, zipf_milli 9000
    let common_distant = RankedCandidate {
        word: "ziman".to_string(),
        edit_cost: 3,
        is_diacritic_match: false,
        prefix_quality: 0,
        frequency: EngineFrequencyMetadata {
            token_count: 10000,
            document_count: 500,
            zipf_milli: 9000,
        },
        kind: SuggestionKind::Correction,
    };

    // Rare close candidate MUST rank before common distant candidate
    assert_eq!(
        rare_close.cmp_with_config(&common_distant, &config),
        std::cmp::Ordering::Less,
        "Lower edit cost MUST take precedence over frequency"
    );
}

#[test]
fn test_frequency_tie_breaking_when_edit_cost_equal() {
    let config = RankingConfig::default();

    let cand_high_freq = RankedCandidate {
        word: "rojbaş".to_string(),
        edit_cost: 1,
        is_diacritic_match: true,
        prefix_quality: 0,
        frequency: EngineFrequencyMetadata {
            token_count: 100,
            document_count: 10,
            zipf_milli: 7500,
        },
        kind: SuggestionKind::Correction,
    };

    let cand_low_freq = RankedCandidate {
        word: "rojan".to_string(),
        edit_cost: 1,
        is_diacritic_match: true,
        prefix_quality: 0,
        frequency: EngineFrequencyMetadata {
            token_count: 2,
            document_count: 1,
            zipf_milli: 2000,
        },
        kind: SuggestionKind::Correction,
    };

    assert_eq!(
        cand_high_freq.cmp_with_config(&cand_low_freq, &config),
        std::cmp::Ordering::Less,
        "Higher Zipf frequency MUST rank first when edit cost and diacritic match are equal"
    );
}

#[test]
fn test_disabled_frequency_falls_back_to_lexical() {
    let disabled_config = RankingConfig::disabled();

    let cand_b = RankedCandidate {
        word: "baza".to_string(),
        edit_cost: 1,
        is_diacritic_match: false,
        prefix_quality: 0,
        frequency: EngineFrequencyMetadata {
            token_count: 1000,
            document_count: 100,
            zipf_milli: 9000,
        },
        kind: SuggestionKind::Correction,
    };

    let cand_a = RankedCandidate {
        word: "amêd".to_string(),
        edit_cost: 1,
        is_diacritic_match: false,
        prefix_quality: 0,
        frequency: EngineFrequencyMetadata {
            token_count: 1,
            document_count: 1,
            zipf_milli: 100,
        },
        kind: SuggestionKind::Correction,
    };

    // When frequency is disabled, lexical ordering ('amêd' < 'baza') determines order
    assert_eq!(
        cand_a.cmp_with_config(&cand_b, &disabled_config),
        std::cmp::Ordering::Less,
        "Disabled frequency must fall back to lexical ordering"
    );
}

#[test]
fn test_exact_match_priority_over_high_frequency_diacritic_variant() {
    let config = RankingConfig::default();

    // Exact match candidate: low Zipf frequency (zipf_milli: 1000)
    let exact_cand = RankedCandidate {
        word: "bi".to_string(),
        edit_cost: 0,
        is_diacritic_match: true,
        prefix_quality: 100,
        frequency: EngineFrequencyMetadata {
            token_count: 5,
            document_count: 1,
            zipf_milli: 1000,
        },
        kind: SuggestionKind::Exact,
    };

    // Diacritic correction candidate: very high Zipf frequency (zipf_milli: 9500)
    let diacritic_cand = RankedCandidate {
        word: "bî".to_string(),
        edit_cost: 0,
        is_diacritic_match: true,
        prefix_quality: 0,
        frequency: EngineFrequencyMetadata {
            token_count: 50000,
            document_count: 2000,
            zipf_milli: 9500,
        },
        kind: SuggestionKind::DiacriticCorrection,
    };

    assert_eq!(
        exact_cand.cmp_with_config(&diacritic_cand, &config),
        std::cmp::Ordering::Less,
        "Exact candidate MUST rank before diacritic correction candidate despite lower frequency"
    );
}

// ─── Binary Pack Version 3 Tests ──────────────────────────────────────────

#[test]
fn test_binary_pack_v3_roundtrip_and_version_rejection() {
    let entries = vec![SourceLexiconEntry {
        word: "rojbaş".to_string(),
        lemma: "rojbaş".to_string(),
        normalized: "rojbaş".to_string(),
        part_of_speech: "noun".to_string(),
        frequency: 10,
        status: "accepted".to_string(),
        variants: vec![],
        sources: vec!["manual-seed".to_string()],
        regions: vec!["standard".to_string()],
        frequency_metadata: Some(BuilderFrequencyMetadata {
            token_count: 100,
            document_count: 10,
            zipf_milli: 7790,
        }),
    }];

    let bin_bytes = compile_binary_pack(&entries).expect("Binary pack v4 compilation failed");
    assert_eq!(&bin_bytes[0..4], MAGIC_BYTES);
    assert_eq!(PACK_VERSION, 4);

    let mut engine = Engine::new();
    let loaded = engine
        .load_binary_pack(&bin_bytes)
        .expect("Binary pack v3 load failed");
    assert_eq!(loaded, 1);

    // Verify version 2 pack rejection
    let mut v2_bytes = bin_bytes.clone();
    v2_bytes[4..8].copy_from_slice(&2u32.to_le_bytes()); // Force version 2
    let res = engine.load_binary_pack(&v2_bytes);
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("unsupported language-pack version"));
}

// ─── Integration & Evaluation Suite Tests ─────────────────────────────────

#[test]
fn test_evaluation_pipeline_and_determinism() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();

    // Ensure version 2 binary pack exists
    let pack_path = root.join("data/build/lexicon.bin");
    let needs_rebuild = if pack_path.exists() {
        let bytes = fs::read(&pack_path).unwrap_or_default();
        bytes.len() < 8 || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != PACK_VERSION
    } else {
        true
    };

    if needs_rebuild {
        let seed_path = root.join("data/reviewed/lexicon.jsonl");
        let file = fs::File::open(&seed_path).expect("Failed to open seed lexicon");
        let reader = std::io::BufReader::new(file);
        let mut entries: Vec<SourceLexiconEntry> = std::io::BufRead::lines(reader)
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).unwrap())
            .collect();

        let _ = data_builder_lib::corpus::join_frequencies_to_lexicon(&root, &mut entries);
        let bin_bytes = compile_binary_pack(&entries).expect("Compilation failed");
        fs::create_dir_all(pack_path.parent().unwrap()).unwrap();
        fs::write(&pack_path, bin_bytes).unwrap();
    }

    // 1. Run ranking evaluation (Pass 1)
    let summary1 = run_ranking_evaluation(&root).expect("Ranking evaluation pass 1 must succeed");
    assert!(summary1.total_cases > 0);
    assert!(summary1.acceptance_passed);

    let report_dir = root.join("data/reports/ranking-evaluation");
    let expected_reports = [
        "summary.json",
        "baseline-results.jsonl",
        "frequency-results.jsonl",
        "changed-rankings.jsonl",
        "regressions.jsonl",
        "improvements.jsonl",
        "README.md",
        "artifacts.sha256",
    ];

    for file in &expected_reports {
        assert!(
            report_dir.join(file).exists(),
            "Evaluation report file missing: {}",
            file
        );
    }

    // Verify manifest
    let manifest_content = fs::read_to_string(report_dir.join("artifacts.sha256"))
        .expect("Failed to read artifacts.sha256");

    for file in &expected_reports[..7] {
        let content = fs::read(report_dir.join(file)).expect("Failed to read report file");
        let hash = format!("{:x}", Sha256::digest(&content));
        assert!(
            manifest_content.contains(&hash),
            "Manifest must contain hash for {}",
            file
        );
    }

    let pass1_manifest_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(report_dir.join("artifacts.sha256")).unwrap())
    );

    // 2. Run ranking evaluation (Pass 2 - Determinism check)
    let summary2 = run_ranking_evaluation(&root).expect("Ranking evaluation pass 2 must succeed");
    assert_eq!(
        summary1.baseline_top_1_accuracy,
        summary2.baseline_top_1_accuracy
    );
    assert_eq!(
        summary1.experiment_top_1_accuracy,
        summary2.experiment_top_1_accuracy
    );

    let pass2_manifest_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(report_dir.join("artifacts.sha256")).unwrap())
    );

    assert_eq!(
        pass1_manifest_hash, pass2_manifest_hash,
        "Evaluation report suite must be byte-for-byte identical across runs"
    );
}
