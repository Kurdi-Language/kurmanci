//! Evaluator Self-Parity Test: Proves that when base_pack == candidate_pack,
//! evaluate_candidate_experiment_pack yields identical base and candidate metrics
//! with zero improvements and zero regressions.

use data_builder_lib::compile::{compile_binary_pack_with_config, CompilerModelConfig};
use data_builder_lib::evaluate_candidate_experiment_pack;
use data_builder_lib::validate::SourceLexiconEntry;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_candidate_evaluator_self_parity() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.parent().unwrap().to_path_buf();

    // Load tracked reviewed lexicon entries without depending on data/build/ or controlled review reports
    let reviewed_lexicon_path = root.join("data/reviewed/lexicon.jsonl");
    let file = File::open(&reviewed_lexicon_path)
        .expect("Failed to open tracked data/reviewed/lexicon.jsonl");
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        if !line.trim().is_empty() {
            entries.push(
                serde_json::from_str::<SourceLexiconEntry>(&line)
                    .expect("Failed to parse lexicon record"),
            );
        }
    }
    assert!(
        !entries.is_empty(),
        "Tracked reviewed lexicon entries must not be empty"
    );

    // Compile deterministic binary pack directly in an isolated tempdir
    let pack_bytes = compile_binary_pack_with_config(&root, &entries, CompilerModelConfig::none())
        .expect("Failed to compile test binary pack");

    let tmp = tempdir().expect("Failed to create tempdir");
    let test_pack_path = tmp.path().join("test_lexicon.bin");
    fs::write(&test_pack_path, &pack_bytes).expect("Failed to write test lexicon binary");

    // Evaluate base binary against identical candidate binary
    let parity_comp = evaluate_candidate_experiment_pack(&root, &test_pack_path, &test_pack_path)
        .expect("Candidate experiment evaluation failed");

    // Self-parity invariants (base == candidate metrics):
    assert_eq!(parity_comp.total_cases, 357);
    assert_eq!(
        parity_comp.base_raw_pass_count,
        parity_comp.candidate_raw_pass_count
    );
    assert_eq!(parity_comp.top_1_base, parity_comp.top_1_cand);
    assert_eq!(parity_comp.top_3_base, parity_comp.top_3_cand);
    assert_eq!(parity_comp.top_5_base, parity_comp.top_5_cand);
    assert_eq!(parity_comp.comp_recall_base, parity_comp.comp_recall_cand);
    assert_eq!(parity_comp.kw_coverage_base, parity_comp.kw_coverage_cand);
    assert_eq!(parity_comp.fa_rate_base, parity_comp.fa_rate_cand);
    assert_eq!(parity_comp.mrr_base, parity_comp.mrr_cand);
    assert_eq!(
        parity_comp.improvements.len(),
        0,
        "Improvements must be 0 when base == candidate!"
    );
    assert_eq!(
        parity_comp.regressions.len(),
        0,
        "Regressions must be 0 when base == candidate!"
    );
}
