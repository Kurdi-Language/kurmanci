use data_builder_lib::importers::{import_hunspell_dic, parse_hunspell_line};
use sha2::Digest;
use std::fs;
use std::path::PathBuf;

fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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
        PathBuf::from(".")
    }
}

#[test]
fn test_hunspell_line_parser_plain_words_and_flags() {
    let p1 = parse_hunspell_line("rojbaş/AN").unwrap();
    assert_eq!(p1.raw_word, "rojbaş");
    assert_eq!(p1.flags, "AN");
    assert!(p1.morphology.is_empty());

    let p2 = parse_hunspell_line("axivtin").unwrap();
    assert_eq!(p2.raw_word, "axivtin");
    assert_eq!(p2.flags, "");
    assert!(p2.morphology.is_empty());
}

#[test]
fn test_hunspell_line_parser_morphology_without_flags() {
    // "word po:noun"
    let p1 = parse_hunspell_line("axivtin po:noun").unwrap();
    assert_eq!(p1.raw_word, "axivtin");
    assert_eq!(p1.flags, "");
    assert_eq!(p1.morphology, vec!["po:noun"]);

    // "word/FLAGS po:noun"
    let p2 = parse_hunspell_line("axivtin/V po:verb").unwrap();
    assert_eq!(p2.raw_word, "axivtin");
    assert_eq!(p2.flags, "V");
    assert_eq!(p2.morphology, vec!["po:verb"]);

    // "word/ po:noun"
    let p3 = parse_hunspell_line("axivtin/ po:verb").unwrap();
    assert_eq!(p3.raw_word, "axivtin");
    assert_eq!(p3.flags, "");
    assert_eq!(p3.morphology, vec!["po:verb"]);
}

#[test]
fn test_hunspell_line_parser_escaped_slash() {
    let p = parse_hunspell_line(r"AC\/DC/AN po:noun").unwrap();
    assert_eq!(p.raw_word, "AC/DC");
    assert_eq!(p.flags, "AN");
    assert_eq!(p.morphology, vec!["po:noun"]);
}

#[test]
fn test_hunspell_line_parser_canonical_unicode_equivalence() {
    // NFD: "e" + combining circumflex (\u{0302})
    let nfd_word = "e\u{0302}v\u{00EE}n";
    let p = parse_hunspell_line(nfd_word).unwrap();
    // Normalized form should equal NFC "êvîn"
    let norm = data_builder_lib::normalize_text(&p.raw_word);
    assert_eq!(norm, "êvîn");
}

#[test]
fn test_fixture_import_pipeline_and_strengthened_determinism() {
    let test_dir =
        std::env::temp_dir().join(format!("test_fixture_hunspell_{}", std::process::id()));
    let _ = fs::remove_dir_all(&test_dir);

    let original_dir = test_dir.join("data/original/kurdish-hunspell-kmr");
    fs::create_dir_all(&original_dir).unwrap();

    let fixture_dic = r#"4
rojbaş/AN po:ij
axivtin po:verb
pirtûk/N po:noun
rojbaş/AN po:ij
"#;

    let dic_path = original_dir.join("kmr-Latn.dic");
    let aff_path = original_dir.join("kmr-Latn.aff");
    let lic_path = original_dir.join("LICENSE");
    let readme_path = original_dir.join("README.md");

    fs::write(&dic_path, fixture_dic).unwrap();
    fs::write(&aff_path, "# affix rules").unwrap();
    fs::write(&lic_path, "CC-BY-SA-4.0").unwrap();
    fs::write(&readme_path, "# Hunspell").unwrap();

    let sha_dic = format!("{:x}", sha2::Sha256::digest(fixture_dic.as_bytes()));
    let sha_aff = format!("{:x}", sha2::Sha256::digest(b"# affix rules"));
    let sha_lic = format!("{:x}", sha2::Sha256::digest(b"CC-BY-SA-4.0"));
    let sha_readme = format!("{:x}", sha2::Sha256::digest(b"# Hunspell"));

    let reg_dir = test_dir.join("data/source-registry");
    fs::create_dir_all(&reg_dir).unwrap();

    let sources_toml = format!(
        r#"
[[sources]]
source_id = "kurdish-hunspell-kmr"
source_name = "KurdishHunspell"
author = "Sina Ahmadi"
license = "CC-BY-SA-4.0"
license_url = "https://raw.githubusercontent.com/sinaahmadi/KurdishHunspell/88131d6878ef7fa3ee114aa554adc385ff85b44c/LICENSE"
url = "https://github.com/sinaahmadi/KurdishHunspell"
version = "88131d6878ef7fa3ee114aa554adc385ff85b44c"
redistribution = "allowed"
notes = "Fixture test"

[[sources.files]]
path = "data/original/kurdish-hunspell-kmr/kmr-Latn.dic"
sha256 = "{}"

[[sources.files]]
path = "data/original/kurdish-hunspell-kmr/kmr-Latn.aff"
sha256 = "{}"

[[sources.files]]
path = "data/original/kurdish-hunspell-kmr/LICENSE"
sha256 = "{}"

[[sources.files]]
path = "data/original/kurdish-hunspell-kmr/README.md"
sha256 = "{}"
"#,
        sha_dic, sha_aff, sha_lic, sha_readme
    );

    fs::write(reg_dir.join("sources.toml"), sources_toml).unwrap();

    // Import Pass 1
    let summary1 = import_hunspell_dic("kurdish-hunspell-kmr", &test_dir).unwrap();
    assert_eq!(summary1.declared_entry_count, Some(4));
    assert_eq!(summary1.parsed_entries, 4);
    assert_eq!(summary1.accepted_entries, 3);
    assert_eq!(summary1.duplicate_surface_forms, 1);

    let lex_bytes1 =
        fs::read(test_dir.join("data/imported/kurdish-hunspell-kmr/lexicon.jsonl")).unwrap();
    let rej_bytes1 =
        fs::read(test_dir.join("data/reports/kurdish-hunspell-kmr/rejected.jsonl")).unwrap();
    let conf_bytes1 =
        fs::read(test_dir.join("data/reports/kurdish-hunspell-kmr/conflicts.jsonl")).unwrap();
    let sum_bytes1 =
        fs::read(test_dir.join("data/reports/kurdish-hunspell-kmr/import-summary.json")).unwrap();

    // Import Pass 2 (Strengthened Determinism Verification)
    let _summary2 = import_hunspell_dic("kurdish-hunspell-kmr", &test_dir).unwrap();
    let lex_bytes2 =
        fs::read(test_dir.join("data/imported/kurdish-hunspell-kmr/lexicon.jsonl")).unwrap();
    let rej_bytes2 =
        fs::read(test_dir.join("data/reports/kurdish-hunspell-kmr/rejected.jsonl")).unwrap();
    let conf_bytes2 =
        fs::read(test_dir.join("data/reports/kurdish-hunspell-kmr/conflicts.jsonl")).unwrap();
    let sum_bytes2 =
        fs::read(test_dir.join("data/reports/kurdish-hunspell-kmr/import-summary.json")).unwrap();

    // Byte-identical comparisons across all 4 generated output files
    assert_eq!(
        lex_bytes1, lex_bytes2,
        "lexicon.jsonl must be byte-identical"
    );
    assert_eq!(
        rej_bytes1, rej_bytes2,
        "rejected.jsonl must be byte-identical"
    );
    assert_eq!(
        conf_bytes1, conf_bytes2,
        "conflicts.jsonl must be byte-identical"
    );
    assert_eq!(
        sum_bytes1, sum_bytes2,
        "import-summary.json must be byte-identical"
    );

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_registered_hunspell_source_import_and_strengthened_determinism() {
    let root = get_workspace_root();
    let reg_path = root.join("data/source-registry/sources.toml");
    if !reg_path.exists() {
        return;
    }

    let summary1 = import_hunspell_dic("kurdish-hunspell-kmr", &root).unwrap();
    assert!(
        summary1.accepted_entries > 40000,
        "Accepted entries should exceed 40,000"
    );

    let lex_bytes1 =
        fs::read(root.join("data/imported/kurdish-hunspell-kmr/lexicon.jsonl")).unwrap();
    let rej_bytes1 =
        fs::read(root.join("data/reports/kurdish-hunspell-kmr/rejected.jsonl")).unwrap();
    let conf_bytes1 =
        fs::read(root.join("data/reports/kurdish-hunspell-kmr/conflicts.jsonl")).unwrap();
    let sum_bytes1 =
        fs::read(root.join("data/reports/kurdish-hunspell-kmr/import-summary.json")).unwrap();

    let _summary2 = import_hunspell_dic("kurdish-hunspell-kmr", &root).unwrap();

    let lex_bytes2 =
        fs::read(root.join("data/imported/kurdish-hunspell-kmr/lexicon.jsonl")).unwrap();
    let rej_bytes2 =
        fs::read(root.join("data/reports/kurdish-hunspell-kmr/rejected.jsonl")).unwrap();
    let conf_bytes2 =
        fs::read(root.join("data/reports/kurdish-hunspell-kmr/conflicts.jsonl")).unwrap();
    let sum_bytes2 =
        fs::read(root.join("data/reports/kurdish-hunspell-kmr/import-summary.json")).unwrap();

    assert_eq!(
        lex_bytes1, lex_bytes2,
        "lexicon.jsonl must be byte-identical"
    );
    assert_eq!(
        rej_bytes1, rej_bytes2,
        "rejected.jsonl must be byte-identical"
    );
    assert_eq!(
        conf_bytes1, conf_bytes2,
        "conflicts.jsonl must be byte-identical"
    );
    assert_eq!(
        sum_bytes1, sum_bytes2,
        "import-summary.json must be byte-identical"
    );
}
