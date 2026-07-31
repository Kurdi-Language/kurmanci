//! Integration tests for the quality audit pipeline.
//!
//! Tests cover:
//! - Full audit pipeline on the real registered source
//! - Deterministic output verification (byte-identical across runs)
//! - Cross-check correctness
//! - Classification unit tests for edge cases

use data_builder_lib::audit::classify::{
    classify_script, classify_shape, is_possible_proper_noun, is_unicode_letter, is_unicode_number,
    is_unicode_punctuation, is_unicode_symbol, ScriptClass,
};

// ─── Classification edge cases ──────────────────────────────────────────────

#[test]
fn test_arabic_only_script() {
    // Arabic word: مرحبا
    assert_eq!(classify_script("مرحبا"), ScriptClass::ArabicOnly);
}

#[test]
fn test_latin_arabic_mixed_script() {
    // Mix of Latin "abc" and Arabic "مرح"
    assert_eq!(classify_script("abcمرح"), ScriptClass::LatinArabicMixed);
}

#[test]
fn test_latin_cyrillic_mixed_script() {
    assert_eq!(classify_script("abcПри"), ScriptClass::LatinCyrillicMixed);
}

#[test]
fn test_greek_plus_syriac_is_other_mixed() {
    // Greek α (U+03B1) + Syriac ܐ (U+0710) — two non-Latin scripts
    let result = classify_script("α\u{0710}");
    assert_eq!(result, ScriptClass::OtherMixedScript);
}

#[test]
fn test_digits_do_not_make_mixed_script() {
    // Latin letters with digits — digits are Common script
    assert_eq!(classify_script("abc123"), ScriptClass::LatinOnly);
}

#[test]
fn test_common_punctuation_not_mixed() {
    // Hyphen and apostrophe are Common script
    assert_eq!(classify_script("xwe-xwe"), ScriptClass::LatinOnly);
    assert_eq!(classify_script("l'amour"), ScriptClass::LatinOnly);
}

#[test]
fn test_no_scripted_letters_digits_and_punctuation() {
    assert_eq!(classify_script("123!@#"), ScriptClass::NoScriptedLetters);
}

#[test]
fn test_punctuation_only_vs_symbol_only() {
    let punc = classify_shape("!.,", "!.,");
    assert!(punc.is_punctuation_only);
    assert!(!punc.is_symbol_only);
    assert!(!punc.is_digit_only);

    let sym = classify_shape("$€", "$€");
    assert!(!sym.is_punctuation_only);
    assert!(sym.is_symbol_only);
    assert!(!sym.is_digit_only);
}

#[test]
fn test_digit_only() {
    let shape = classify_shape("42", "42");
    assert!(shape.is_digit_only);
    assert!(!shape.is_punctuation_only);
    assert!(!shape.is_symbol_only);
}

#[test]
fn test_no_letters_mixed_punct_digit() {
    let shape = classify_shape("123!", "123!");
    assert!(shape.has_no_letters);
    assert!(!shape.is_digit_only);
    assert!(!shape.is_punctuation_only);
}

#[test]
fn test_uppercase_requires_cased_letter() {
    let shape = classify_shape("!!!", "!!!");
    assert!(!shape.is_uppercase_only);

    let shape = classify_shape("ABC", "abc");
    assert!(shape.is_uppercase_only);
    assert!(!shape.is_title_case);
}

#[test]
fn test_title_case_strict() {
    let shape = classify_shape("Rojbaş", "rojbaş");
    assert!(shape.is_title_case);
    assert!(!shape.is_uppercase_only);
    assert!(!shape.is_mixed_case);

    // Uncased Lo letter should not satisfy the lowercase rule
    let shape = classify_shape("A中", "a中");
    assert!(
        !shape.is_title_case,
        "Uncased Lo letters must not satisfy the lowercase requirement"
    );
}

#[test]
fn test_mixed_case() {
    let shape = classify_shape("rOjBaŞ", "rojbaş");
    assert!(shape.is_mixed_case);
    assert!(!shape.is_title_case);
    assert!(!shape.is_uppercase_only);
}

#[test]
fn test_possible_proper_noun() {
    assert!(is_possible_proper_noun("Rojbaş"));
    assert!(is_possible_proper_noun("Diyarbekir"));
    assert!(!is_possible_proper_noun("rojbaş"));
    assert!(!is_possible_proper_noun("ROJBAŞ"));
    assert!(!is_possible_proper_noun("a"));
}

#[test]
fn test_grapheme_vs_scalar_length() {
    let shape = classify_shape("e\u{0301}", "e\u{0301}");
    assert_eq!(shape.unicode_scalar_length, 2);
    assert_eq!(shape.grapheme_length, 1);
}

#[test]
fn test_very_long_thresholds() {
    let long_word = "a".repeat(26);
    let shape = classify_shape(&long_word, &long_word);
    assert!(shape.is_very_long_25);
    assert!(!shape.is_very_long_40);

    let very_long = "a".repeat(41);
    let shape = classify_shape(&very_long, &very_long);
    assert!(shape.is_very_long_25);
    assert!(shape.is_very_long_40);
}

#[test]
fn test_contains_digits_requires_letters() {
    let shape = classify_shape("123", "123");
    assert!(!shape.contains_digits);

    let shape = classify_shape("abc123", "abc123");
    assert!(shape.contains_digits);
}

#[test]
fn test_multiword() {
    let shape = classify_shape("ji bo", "ji bo");
    assert!(shape.is_multiword);

    let shape = classify_shape("rojbaş", "rojbaş");
    assert!(!shape.is_multiword);
}

#[test]
fn test_unicode_category_functions() {
    assert!(is_unicode_punctuation('!'));
    assert!(is_unicode_punctuation('.'));
    assert!(is_unicode_punctuation(','));
    assert!(!is_unicode_punctuation('$'));
    assert!(!is_unicode_punctuation('a'));
    assert!(!is_unicode_punctuation('5'));

    assert!(is_unicode_symbol('$'));
    assert!(is_unicode_symbol('€'));
    assert!(is_unicode_symbol('+'));
    assert!(!is_unicode_symbol('a'));
    assert!(!is_unicode_symbol('!'));

    assert!(is_unicode_number('0'));
    assert!(is_unicode_number('9'));
    assert!(!is_unicode_number('a'));

    assert!(is_unicode_letter('a'));
    assert!(is_unicode_letter('ş'));
    assert!(is_unicode_letter('ê'));
    assert!(!is_unicode_letter('5'));
    assert!(!is_unicode_letter('!'));
}

fn get_workspace_root() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir
        .join("data/source-registry/sources.toml")
        .exists()
    {
        manifest_dir
    } else if manifest_dir
        .join("../data/source-registry/sources.toml")
        .exists()
    {
        manifest_dir.join("..")
    } else {
        std::path::PathBuf::from(".")
    }
}

// ─── Full pipeline integration test ─────────────────────────────────────────

#[test]
fn test_registered_source_audit_pipeline_and_determinism() {
    use sha2::{Digest, Sha256};
    use std::fs;

    let root = get_workspace_root();

    // Run the importer to ensure lexicon.jsonl exists.
    // A missing prerequisite must FAIL, not silently skip.
    let lexicon_path = root.join("data/imported/kurdish-hunspell-kmr/lexicon.jsonl");
    if !lexicon_path.exists() {
        data_builder_lib::import_hunspell_dic("kurdish-hunspell-kmr", &root)
            .expect("Importer must succeed to create lexicon.jsonl for audit test");
    }

    assert!(
        lexicon_path.exists(),
        "FATAL: lexicon.jsonl must exist after import — cannot silently skip"
    );

    let audit_dir = root.join("data/reports/kurdish-hunspell-kmr/quality-audit");

    // Run 1
    data_builder_lib::run_quality_audit("kurdish-hunspell-kmr", &root).expect("Audit run 1 failed");

    let expected_files = [
        "summary.json",
        "character-inventory.json",
        "script-analysis.json",
        "length-distribution.json",
        "shape-analysis.json",
        "flag-analysis.json",
        "morphology-analysis.json",
        "conflict-groups.jsonl",
        "duplicate-groups.jsonl",
        "rejection-review.jsonl",
        "suspicious-entries.jsonl",
        "manual-seed-comparison.json",
        "benchmark-audit.json",
        "review-sample.jsonl",
        "README.md",
        "artifacts.sha256",
    ];

    // Verify all 16 files exist and that NO extra/stale files exist in audit_dir
    let actual_files: std::collections::BTreeSet<String> = fs::read_dir(&audit_dir)
        .expect("Failed to read audit_dir")
        .filter_map(|entry| entry.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    let expected_set: std::collections::BTreeSet<String> =
        expected_files.iter().map(|s| s.to_string()).collect();

    assert_eq!(
        actual_files, expected_set,
        "Audit directory contains unexpected or missing files!"
    );

    // Compute checksums for run 1
    let run1_checksums: Vec<(String, String)> = expected_files
        .iter()
        .map(|f| {
            let content = fs::read(audit_dir.join(f))
                .unwrap_or_else(|e| panic!("Failed to read {}: {}", f, e));
            let hash = format!("{:x}", Sha256::digest(&content));
            (f.to_string(), hash)
        })
        .collect();

    // Run 2
    data_builder_lib::run_quality_audit("kurdish-hunspell-kmr", &root).expect("Audit run 2 failed");

    // Compare all 16 files byte-for-byte
    for (filename, run1_hash) in &run1_checksums {
        let content = fs::read(audit_dir.join(filename))
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", filename, e));
        let run2_hash = format!("{:x}", Sha256::digest(&content));
        assert_eq!(
            run1_hash, &run2_hash,
            "DETERMINISM FAILURE: {} differs between runs. Run1={}, Run2={}",
            filename, run1_hash, run2_hash
        );
    }

    // Verify summary.json has cross_check_passed = true
    let summary_content =
        fs::read_to_string(audit_dir.join("summary.json")).expect("Failed to read summary.json");
    assert!(
        summary_content.contains("\"cross_check_passed\": true"),
        "Cross-check should pass"
    );
    assert!(
        summary_content.contains("\"verdict\": \"A\""),
        "Verdict should be A"
    );
}
