use data_builder_lib::{calculate_sha256, compile_binary_pack, SourceLexiconEntry, SourceRegistry};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;

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

fn start_mock_server(status: u16, content: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);

            let header = if status == 200 {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    content.len()
                )
            } else {
                format!(
                    "HTTP/1.1 {} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    status
                )
            };

            let _ = stream.write_all(header.as_bytes());
            if status == 200 {
                let _ = stream.write_all(content);
            }
            let _ = stream.flush();
        }
    });

    url
}

#[test]
fn test_source_registry_integrity() {
    let root = get_workspace_root();
    let registry_path = root.join("data/source-registry/sources.toml");
    assert!(
        registry_path.exists(),
        "data/source-registry/sources.toml must exist at {:?}",
        registry_path
    );

    let registry = SourceRegistry::load_from_file(&registry_path)
        .expect("sources.toml must parse successfully");

    // 1. Verify preserved files and hashes match registry
    registry
        .verify_preserved_files(&root)
        .expect("Preserved source files must pass hash and schema verification");

    // 2. Inspect kurdish-hunspell-kmr specifically
    let hunspell_source = registry
        .sources
        .iter()
        .find(|s| s.source_id == "kurdish-hunspell-kmr")
        .expect("kurdish-hunspell-kmr must be registered in sources.toml");

    // Full 40-character commit SHA
    assert_eq!(
        hunspell_source.version.len(),
        40,
        "Commit SHA version must be exactly 40 characters"
    );
    assert!(
        hunspell_source
            .version
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "Commit SHA must contain hexadecimal characters"
    );

    // Raw URL pinned to commit SHA (no /main/ or /master/)
    assert!(
        !hunspell_source.license_url.contains("/main/"),
        "URL must not refer to mutable branch 'main'"
    );
    assert!(
        !hunspell_source.license_url.contains("/master/"),
        "URL must not refer to mutable branch 'master'"
    );
    assert!(
        hunspell_source
            .license_url
            .contains(&hunspell_source.version),
        "License URL must contain the exact commit SHA"
    );

    // SPDX license identifier CC-BY-SA-4.0
    assert_eq!(
        hunspell_source.license, "CC-BY-SA-4.0",
        "SPDX license identifier must be CC-BY-SA-4.0"
    );

    // Verified registered files contain exact commit SHA in URLs
    for file_entry in &hunspell_source.files {
        if let Some(ref url) = file_entry.url {
            assert!(
                url.contains(&hunspell_source.version),
                "File URL '{}' must contain commit SHA '{}'",
                url,
                hunspell_source.version
            );
            assert!(
                !url.contains("/main/"),
                "File URL '{}' must not contain '/main/'",
                url
            );
            assert!(
                !url.contains("/master/"),
                "File URL '{}' must not contain '/master/'",
                url
            );
        }
    }
}

#[test]
fn test_acquisition_mock_success_and_idempotency() {
    let mock_content = b"sample mock content for hunspell test";
    let mock_url = start_mock_server(200, mock_content);

    let mut hasher = Sha256::new();
    hasher.update(mock_content);
    let expected_hash = format!("{:x}", hasher.finalize());

    let test_dir = std::env::temp_dir().join(format!("test_acq_success_{}", std::process::id()));
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).unwrap();

    let registry_toml = format!(
        r#"
[[sources]]
source_id = "test-mock-source"
source_name = "Test Mock Source"
author = "Test Author"
license = "CC-BY-SA-4.0"
license_url = "{}/LICENSE"
url = "https://github.com/sinaahmadi/KurdishHunspell"
version = "88131d6878ef7fa3ee114aa554adc385ff85b44c"
redistribution = "allowed"
notes = "Test"

[[sources.files]]
path = "target_file.txt"
upstream_path = "target_file.txt"
url = "{}/target_file.txt"
sha256 = "{}"
"#,
        mock_url, mock_url, expected_hash
    );

    let reg_path = test_dir.join("sources.toml");
    fs::write(&reg_path, registry_toml).unwrap();

    let registry = SourceRegistry::load_from_file(&reg_path).unwrap();

    // 1. First Pass Acquisition
    let res1 = registry.acquire_source("test-mock-source", &test_dir, Some(&mock_url));
    assert!(res1.is_ok(), "Acquisition should succeed: {:?}", res1);

    let target = test_dir.join("target_file.txt");
    assert!(target.exists(), "Target file must exist after acquisition");
    let content = fs::read(&target).unwrap();
    assert_eq!(content, mock_content);

    // 2. Second Pass Acquisition (Idempotent - existing file matches hash)
    let res2 = registry.acquire_source("test-mock-source", &test_dir, Some(&mock_url));
    assert!(
        res2.is_ok(),
        "Repeated acquisition must succeed idempotently"
    );

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_acquisition_mock_http_error() {
    let mock_url = start_mock_server(500, b"");

    let test_dir = std::env::temp_dir().join(format!("test_acq_500_{}", std::process::id()));
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).unwrap();

    let registry_toml = format!(
        r#"
[[sources]]
source_id = "test-500-source"
source_name = "Test 500"
author = "Test"
license = "CC-BY-SA-4.0"
license_url = "{}/LICENSE"
url = "https://github.com/sinaahmadi/KurdishHunspell"
version = "88131d6878ef7fa3ee114aa554adc385ff85b44c"
redistribution = "allowed"
notes = "Test"

[[sources.files]]
path = "failed_file.txt"
upstream_path = "failed_file.txt"
url = "{}/failed_file.txt"
sha256 = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
"#,
        mock_url, mock_url
    );

    let reg_path = test_dir.join("sources.toml");
    fs::write(&reg_path, registry_toml).unwrap();

    let registry = SourceRegistry::load_from_file(&reg_path).unwrap();
    let res = registry.acquire_source("test-500-source", &test_dir, Some(&mock_url));
    assert!(res.is_err(), "Acquisition must fail on HTTP error");

    let failed_file = test_dir.join("failed_file.txt");
    assert!(
        !failed_file.exists(),
        "No partial target file should remain after HTTP error"
    );

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_acquisition_mock_checksum_mismatch() {
    let mock_content = b"actual downloaded content";
    let mock_url = start_mock_server(200, mock_content);

    let test_dir = std::env::temp_dir().join(format!("test_acq_mismatch_{}", std::process::id()));
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).unwrap();

    let registry_toml = format!(
        r#"
[[sources]]
source_id = "test-badhash-source"
source_name = "Test Bad Hash"
author = "Test"
license = "CC-BY-SA-4.0"
license_url = "{}/LICENSE"
url = "https://github.com/sinaahmadi/KurdishHunspell"
version = "88131d6878ef7fa3ee114aa554adc385ff85b44c"
redistribution = "allowed"
notes = "Test"

[[sources.files]]
path = "bad_hash.txt"
upstream_path = "bad_hash.txt"
url = "{}/bad_hash.txt"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
"#,
        mock_url, mock_url
    );

    let reg_path = test_dir.join("sources.toml");
    fs::write(&reg_path, registry_toml).unwrap();

    let registry = SourceRegistry::load_from_file(&reg_path).unwrap();
    let res = registry.acquire_source("test-badhash-source", &test_dir, Some(&mock_url));
    assert!(res.is_err(), "Acquisition must fail on checksum mismatch");

    let bad_file = test_dir.join("bad_hash.txt");
    assert!(
        !bad_file.exists(),
        "No target file should be installed after checksum mismatch"
    );

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_acquisition_existing_mismatched_file_rejection() {
    let mock_content = b"new acquired content";
    let mock_url = start_mock_server(200, mock_content);

    let test_dir = std::env::temp_dir().join(format!("test_acq_exist_diff_{}", std::process::id()));
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).unwrap();

    // Pre-create existing file with DIFFERENT content
    let target = test_dir.join("existing.txt");
    fs::write(&target, b"pre-existing local conflicting bytes").unwrap();

    let mut hasher = Sha256::new();
    hasher.update(mock_content);
    let expected_hash = format!("{:x}", hasher.finalize());

    let registry_toml = format!(
        r#"
[[sources]]
source_id = "test-exist-diff"
source_name = "Test Existing Diff"
author = "Test"
license = "CC-BY-SA-4.0"
license_url = "{}/LICENSE"
url = "https://github.com/sinaahmadi/KurdishHunspell"
version = "88131d6878ef7fa3ee114aa554adc385ff85b44c"
redistribution = "allowed"
notes = "Test"

[[sources.files]]
path = "existing.txt"
upstream_path = "existing.txt"
url = "{}/existing.txt"
sha256 = "{}"
"#,
        mock_url, mock_url, expected_hash
    );

    let reg_path = test_dir.join("sources.toml");
    fs::write(&reg_path, registry_toml).unwrap();

    let registry = SourceRegistry::load_from_file(&reg_path).unwrap();
    let res = registry.acquire_source("test-exist-diff", &test_dir, Some(&mock_url));
    assert!(
        res.is_err(),
        "Acquisition must refuse to overwrite existing file with different bytes"
    );
    assert!(res.unwrap_err().contains("refusing to overwrite"));

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_compiler_determinism_remains_intact() {
    let sample_entry = SourceLexiconEntry {
        word: "rojbaş".to_string(),
        lemma: "rojbaş".to_string(),
        normalized: "rojbaş".to_string(),
        part_of_speech: "interjection".to_string(),
        frequency: 100,
        status: "verified".to_string(),
        variants: Vec::new(),
        regions: vec!["general".to_string()],
        sources: vec!["manual-seed".to_string()],
        frequency_metadata: None,
    };

    let entries = vec![sample_entry];

    let bytes_pass1 = compile_binary_pack(&entries).expect("Pass 1 binary compilation failed");
    let hash_pass1 = calculate_sha256(&bytes_pass1);

    let bytes_pass2 = compile_binary_pack(&entries).expect("Pass 2 binary compilation failed");
    let hash_pass2 = calculate_sha256(&bytes_pass2);

    assert_eq!(
        bytes_pass1, bytes_pass2,
        "Compiler output bytes must be identical"
    );
    assert_eq!(
        hash_pass1, hash_pass2,
        "Compiler SHA-256 hashes must be identical"
    );
}
