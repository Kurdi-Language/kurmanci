//! Integration tests for Milestone 3C1: Corpus Infrastructure & Deterministic Partitioning.

use data_builder_lib::corpus::importer::{CanonicalDocumentRecord, CanonicalImportManifest};
use data_builder_lib::corpus::partition::PartitionDocumentRecord;
use data_builder_lib::corpus::registry::CorpusRegistry;
use data_builder_lib::{
    audit_corpora, generate_corpus_inventory, import_all_corpora, partition_corpora,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("Workspace root exists")
        .to_path_buf()
}

#[test]
fn test_format_sensitive_registry_validation() {
    let _lock = TEST_LOCK.lock().unwrap();

    // Invalid jsonl missing text_field
    let invalid_jsonl = r#"
[[corpora]]
corpus_id = "test-invalid-jsonl"
corpus_name = "Test Invalid"
language = "ku-Latn"
license = "MIT"
license_url = "https://example.com"
url = "https://example.com"
version = "1.0"
description = "test"
attribution = "test"
notes = "test"
document_format = "jsonl"
document_id_field = "id"
"#;
    let res: Result<CorpusRegistry, _> = toml::from_str(invalid_jsonl);
    let registry = res.expect("Parse TOML succeeded");
    assert!(registry.corpora[0].validate_schema().is_err());

    // Invalid one-document-per-line with text_field
    let invalid_line = r#"
[[corpora]]
corpus_id = "test-invalid-line"
corpus_name = "Test Invalid"
language = "ku-Latn"
license = "MIT"
license_url = "https://example.com"
url = "https://example.com"
version = "1.0"
description = "test"
attribution = "test"
notes = "test"
document_format = "one-document-per-line"
text_field = "text"
"#;
    let res2: Result<CorpusRegistry, _> = toml::from_str(invalid_line);
    let registry2 = res2.expect("Parse TOML succeeded");
    assert!(registry2.corpora[0].validate_schema().is_err());
}

#[test]
fn test_canonical_import_and_atomic_staging() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();

    let reports = import_all_corpora(&root).expect("import_all_corpora failed");
    assert!(!reports.is_empty());

    let imported_dir = root.join("data/imported-canonical");
    assert!(imported_dir.exists());

    let manifest_path = imported_dir.join("manifest.json");
    assert!(manifest_path.exists());

    let manifest_str = fs::read_to_string(&manifest_path).unwrap();
    let manifest: CanonicalImportManifest = serde_json::from_str(&manifest_str).unwrap();
    assert_eq!(manifest.schema_version, "canonical-corpus-v1");
    assert!(!manifest.corpora.is_empty());

    // Inspect canonical JSONL records
    let first_corpus = &manifest.corpora[0];
    let doc_path = imported_dir.join(&first_corpus.documents_file);
    let file = File::open(&doc_path).unwrap();
    let reader = BufReader::new(file);

    let mut line_count = 0usize;
    for line in reader.lines() {
        let line_str = line.unwrap();
        let record: CanonicalDocumentRecord = serde_json::from_str(&line_str).unwrap();

        assert_eq!(record.corpus_id, first_corpus.corpus_id);
        assert!(!record.document_id.is_empty());
        assert!(!record.text.trim().is_empty());
        assert!(record.document_id.contains(':')); // <filename>:<line>
        line_count += 1;
    }
    assert_eq!(line_count, first_corpus.document_count);
}

#[test]
fn test_inventory_audit_and_partitioning() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();

    let _ = import_all_corpora(&root).expect("import_all_corpora failed");

    // Inventory
    let inv_summary = generate_corpus_inventory(&root).expect("generate_corpus_inventory failed");
    assert!(inv_summary.corpus_count > 0);
    assert!(inv_summary.document_count > 0);

    // Audit
    let audit_summary = audit_corpora(&root).expect("audit_corpora failed");
    assert!(audit_summary.total_documents > 0);

    // Partition
    let part_summary = partition_corpora(&root).expect("partition_corpora failed");
    assert_eq!(part_summary.total_documents, inv_summary.document_count);
    assert_eq!(
        part_summary.train_documents
            + part_summary.development_documents
            + part_summary.evaluation_documents,
        part_summary.total_documents
    );

    let part_build_dir = root.join("data/build/corpus-partitions");
    assert!(part_build_dir.join("train.jsonl").exists());
    assert!(part_build_dir.join("development.jsonl").exists());
    assert!(part_build_dir.join("evaluation.jsonl").exists());
    assert!(part_build_dir.join("manifest.json").exists());
}

#[test]
fn test_two_level_non_leakage_assertions() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();

    let _ = import_all_corpora(&root).expect("Import failed");
    let _ = partition_corpora(&root).expect("Partitioning failed");

    let part_build_dir = root.join("data/build/corpus-partitions");

    let partitions = ["train.jsonl", "development.jsonl", "evaluation.jsonl"];
    let mut doc_locations: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut group_locations: BTreeMap<String, String> = BTreeMap::new();

    for p_file in &partitions {
        let p_path = part_build_dir.join(p_file);
        let f = File::open(&p_path).unwrap();
        let reader = BufReader::new(f);

        for line_res in reader.lines() {
            let line = line_res.unwrap();
            if line.trim().is_empty() {
                continue;
            }
            let rec: PartitionDocumentRecord = serde_json::from_str(&line).unwrap();

            // 1. Assert zero (corpus_id, document_id) cross-partition leakage
            let doc_key = (rec.corpus_id.clone(), rec.document_id.clone());
            if let Some(existing_p) = doc_locations.get(&doc_key) {
                panic!(
                    "Leakage detected! Document {:?} found in both '{}' and '{}'",
                    doc_key, existing_p, rec.partition
                );
            }
            doc_locations.insert(doc_key, rec.partition.clone());

            // 2. Assert zero duplicate_group_id cross-partition leakage
            if let Some(existing_p) = group_locations.get(&rec.duplicate_group_id) {
                if existing_p != &rec.partition {
                    panic!(
                        "Leakage detected! Duplicate group '{}' found in both '{}' and '{}'",
                        rec.duplicate_group_id, existing_p, rec.partition
                    );
                }
            }
            group_locations.insert(rec.duplicate_group_id.clone(), rec.partition.clone());
        }
    }
}

#[test]
fn test_exact_report_manifest_integrity() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();

    let _ = import_all_corpora(&root).expect("Import failed");
    let _ = generate_corpus_inventory(&root).expect("Inventory failed");
    let _ = audit_corpora(&root).expect("Audit failed");
    let _ = partition_corpora(&root).expect("Partition failed");

    let report_suites = [
        (
            "data/reports/corpus-inventory",
            vec!["summary.json", "README.md", "artifacts.sha256"],
            vec![
                "data/reports/corpus-inventory/summary.json",
                "data/reports/corpus-inventory/README.md",
            ],
        ),
        (
            "data/reports/corpus-quality",
            vec![
                "summary.json",
                "duplicate-files.jsonl",
                "duplicate-documents.jsonl",
                "duplicate-sentences.jsonl",
                "script-validation.json",
                "per-corpus-statistics.json",
                "README.md",
                "artifacts.sha256",
            ],
            vec![
                "data/reports/corpus-quality/summary.json",
                "data/reports/corpus-quality/duplicate-files.jsonl",
                "data/reports/corpus-quality/duplicate-documents.jsonl",
                "data/reports/corpus-quality/duplicate-sentences.jsonl",
                "data/reports/corpus-quality/script-validation.json",
                "data/reports/corpus-quality/per-corpus-statistics.json",
                "data/reports/corpus-quality/README.md",
            ],
        ),
        (
            "data/reports/corpus-partitions",
            vec!["summary.json", "README.md", "artifacts.sha256"],
            vec![
                "data/reports/corpus-partitions/summary.json",
                "data/reports/corpus-partitions/README.md",
            ],
        ),
    ];

    for (rel_dir, expected_files_slice, expected_manifest_paths_slice) in &report_suites {
        let dir_path = root.join(rel_dir);
        assert!(dir_path.exists());

        let expected_files: BTreeSet<String> =
            expected_files_slice.iter().map(|s| s.to_string()).collect();
        let actual_files: BTreeSet<String> = fs::read_dir(&dir_path)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();

        assert_eq!(
            actual_files, expected_files,
            "Directory set mismatch for {}",
            rel_dir
        );

        let manifest_str = fs::read_to_string(dir_path.join("artifacts.sha256")).unwrap();
        assert!(
            !manifest_str.contains("artifacts.sha256"),
            "Manifest must exclude itself"
        );

        let expected_manifest_paths: BTreeSet<String> = expected_manifest_paths_slice
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut actual_manifest_paths = BTreeSet::new();

        for line in manifest_str.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(parts.len(), 2);
            let expected_hash = parts[0];
            let rel_path = parts[1];

            actual_manifest_paths.insert(rel_path.to_string());

            let target = root.join(rel_path);
            assert!(target.exists());

            let content = fs::read(&target).unwrap();
            let computed_hash = format!("{:x}", Sha256::digest(&content));
            assert_eq!(
                computed_hash, expected_hash,
                "Hash mismatch for {}",
                rel_path
            );
        }

        assert_eq!(
            actual_manifest_paths, expected_manifest_paths,
            "Manifest relative path set mismatch for {}",
            rel_dir
        );
    }
}

#[test]
fn test_multi_file_same_basename_doc_id_uniqueness() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let imported_dir = root.join("data/imported-canonical");

    let _ = import_all_corpora(&root).expect("Import failed");

    let manifest_str = fs::read_to_string(imported_dir.join("manifest.json")).unwrap();
    let manifest: CanonicalImportManifest = serde_json::from_str(&manifest_str).unwrap();

    let mut doc_ids = BTreeSet::new();
    for corpus_entry in &manifest.corpora {
        let doc_path = imported_dir.join(&corpus_entry.documents_file);
        let f = File::open(&doc_path).unwrap();
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            let rec: CanonicalDocumentRecord = serde_json::from_str(&line).unwrap();
            assert!(
                doc_ids.insert(rec.document_id.clone()),
                "Document ID collision detected for ID: {}",
                rec.document_id
            );
            assert!(
                rec.document_id.contains('/'),
                "Document ID must include relative directory path, got: {}",
                rec.document_id
            );
        }
    }
}

#[test]
fn test_tampered_canonical_document_checksum_rejection() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let imported_dir = root.join("data/imported-canonical");

    let _ = import_all_corpora(&root).expect("Import failed");

    // Tamper with one byte in documents.jsonl
    let doc_path = imported_dir.join("opensubtitles-kmr/documents.jsonl");
    assert!(doc_path.exists());
    let mut content = fs::read_to_string(&doc_path).unwrap();
    content.push_str("\n{\"tampered\": true}");
    fs::write(&doc_path, &content).unwrap();

    // Verify all 3 subcommands reject tampered file
    let inv_res = generate_corpus_inventory(&root);
    assert!(
        inv_res.is_err(),
        "Inventory must reject tampered documents.jsonl"
    );
    assert!(inv_res.unwrap_err().contains("SHA-256 mismatch"));

    let audit_res = audit_corpora(&root);
    assert!(
        audit_res.is_err(),
        "Audit must reject tampered documents.jsonl"
    );
    assert!(audit_res.unwrap_err().contains("SHA-256 mismatch"));

    let part_res = partition_corpora(&root);
    assert!(
        part_res.is_err(),
        "Partitioning must reject tampered documents.jsonl"
    );
    assert!(part_res.unwrap_err().contains("SHA-256 mismatch"));

    // Restore canonical import state for remaining tests
    let _ = import_all_corpora(&root).expect("Restore import failed");
}

#[test]
fn test_importer_lock_race_prevention() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let lock_path = root.join("data/test_race.lock");

    if lock_path.exists() {
        let _ = fs::remove_file(&lock_path);
    }

    let guard1 = data_builder_lib::corpus::importer::LockFileGuard::acquire(&lock_path)
        .expect("First lock acquisition should succeed");

    let guard2 = data_builder_lib::corpus::importer::LockFileGuard::acquire(&lock_path);
    assert!(
        guard2.is_err(),
        "Second lock acquisition must fail atomically"
    );

    drop(guard1);
    assert!(!lock_path.exists(), "Lock file must be deleted on drop");
}

#[test]
fn test_unsafe_registry_relative_paths() {
    use data_builder_lib::corpus::registry::validate_registry_relative_path;

    let unsafe_paths = [
        "/etc/passwd",
        "/tmp/corpus.txt",
        r"C:\data\corpus.txt",
        "C:/data/corpus.txt",
        "../corpus.txt",
        "./corpus.txt",
        "data//corpus.txt",
        "",
        "foo/../bar.txt",
        "foo/./bar.txt",
    ];

    for path in &unsafe_paths {
        assert!(
            validate_registry_relative_path(path).is_err(),
            "Path '{}' should be rejected as unsafe",
            path
        );
    }

    assert_eq!(
        validate_registry_relative_path("data/original/opensubtitles-kmr/corpus.txt").unwrap(),
        "data/original/opensubtitles-kmr/corpus.txt"
    );
}

#[test]
fn test_unexpected_snapshot_contents_fails() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let imported_dir = root.join("data/imported-canonical");

    let _ = import_all_corpora(&root).expect("Import failed");

    // Add unmanifested file
    let unmanifested_path = imported_dir.join("stale_file.txt");
    fs::write(&unmanifested_path, "stale content").unwrap();

    let inv_res = generate_corpus_inventory(&root);
    assert!(
        inv_res.is_err(),
        "Inventory must reject unexpected contents in imported-canonical"
    );
    assert!(inv_res
        .unwrap_err()
        .contains("Unexpected or unmanifested contents"));

    let _ = fs::remove_file(&unmanifested_path);
}

#[test]
fn test_failed_installation_and_failed_rollback_error_reporting() {
    use std::os::unix::fs::PermissionsExt;

    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();

    let backup_dir = root.join("data/reports/corpus-inventory.tmp_backup");

    // Clean up
    if backup_dir.exists() {
        let _ = fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o755));
        let _ = fs::remove_dir_all(&backup_dir);
    }

    // Ensure inventory exists
    let _ = generate_corpus_inventory(&root).expect("Initial inventory failed");

    // Create backup_dir with a file and make it read-only to force cleanup failure
    fs::create_dir_all(&backup_dir).unwrap();
    fs::write(backup_dir.join("read_only.txt"), "data").unwrap();
    fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let res = generate_corpus_inventory(&root);
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(
        err_msg.contains("Failed to clean backup reports dir"),
        "Expected cleanup failure error msg, got: {}",
        err_msg
    );

    // Reset permissions & clean up
    let _ = fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o755));
    let _ = fs::remove_dir_all(&backup_dir);
}

#[test]
fn test_reviewed_lexicon_audit_validation() {
    let _lock = TEST_LOCK.lock().unwrap();
    let root = get_workspace_root();
    let lexicon_path = root.join("data/reviewed/lexicon.jsonl");
    let backup_lexicon_path = root.join("data/reviewed/lexicon.jsonl.tmp_test_backup");

    // Ensure import exists
    let _ = import_all_corpora(&root).expect("Import failed");

    // Backup existing lexicon if present
    if lexicon_path.exists() {
        fs::rename(&lexicon_path, &backup_lexicon_path).unwrap();
    }

    // 1. Missing lexicon fails audit_corpora
    let missing_res = audit_corpora(&root);
    assert!(missing_res.is_err());
    assert!(missing_res
        .unwrap_err()
        .contains("Reviewed lexicon missing"));

    // 2. Malformed JSON fails audit_corpora
    fs::create_dir_all(lexicon_path.parent().unwrap()).unwrap();
    fs::write(&lexicon_path, "{invalid json line}").unwrap();
    let malformed_res = audit_corpora(&root);
    assert!(malformed_res.is_err());
    assert!(malformed_res
        .unwrap_err()
        .contains("Malformed reviewed lexicon JSON"));

    // 3. Missing normalized field fails audit_corpora
    fs::write(&lexicon_path, "{\"word\": \"roj baş\"}").unwrap();
    let missing_norm_res = audit_corpora(&root);
    assert!(missing_norm_res.is_err());
    assert!(missing_norm_res
        .unwrap_err()
        .contains("Missing normalized field"));

    // 4. Restore original lexicon (or create valid fixture) and ensure audit succeeds
    let _ = fs::remove_file(&lexicon_path);
    if backup_lexicon_path.exists() {
        let _ = fs::rename(&backup_lexicon_path, &lexicon_path);
    } else {
        fs::write(
            &lexicon_path,
            "{\"normalized\": \"roj\"}\n{\"normalized\": \"baş\"}\n",
        )
        .unwrap();
    }

    let valid_res = audit_corpora(&root);
    assert!(valid_res.is_ok(), "Audit must succeed with valid lexicon");
}
