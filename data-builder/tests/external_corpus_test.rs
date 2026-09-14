//! Integration tests for `external` corpora: registry validation, skip-when-absent
//! behaviour of the canonical importer, and fail-closed acquisition verification.

use data_builder_lib::corpus::acquire::acquire_corpus;
use data_builder_lib::corpus::importer::{verify_canonical_manifest, CanonicalImportManifest};
use data_builder_lib::corpus::registry::CorpusRegistry;
use data_builder_lib::import_all_corpora;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kurmanci-external-corpus-{}-{}",
        name,
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(root.join("data/source-registry")).unwrap();
    fs::create_dir_all(root.join("data/original/tracked")).unwrap();
    root
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

const TRACKED_TEXT: &str = "rojbaş cîhan\nez kurd im\n";

fn registry_toml(external_files_sha: &str, extra_external_fields: &str) -> String {
    format!(
        r#"
[[corpora]]
corpus_id = "tracked-small"
corpus_name = "Tracked"
language = "ku-Latn"
license = "CC BY-SA 4.0"
license_spdx = "CC-BY-SA-4.0"
license_url = "https://example.org/license"
url = "https://example.org"
version = "1"
description = "tracked test corpus"
attribution = "test"
notes = "test"
document_format = "one-document-per-line"

[[corpora.files]]
path = "data/original/tracked/corpus.txt"
sha256 = "{tracked_sha}"

[[corpora]]
corpus_id = "external-wiki"
corpus_name = "External"
language = "ku-Latn"
license = "CC BY-SA 4.0"
license_spdx = "CC-BY-SA-4.0"
license_url = "https://example.org/license"
url = "https://example.org/dump/"
version = "20260801"
description = "external test corpus"
attribution = "test"
notes = "test"
document_format = "jsonl"
document_id_field = "page_id"
text_field = "text"
acquisition = "external"
{extra}
[corpora.source_artifact]
url = "https://example.org/dump/pages-articles.xml.bz2"
path = "data/original/external-wiki/pages-articles.xml.bz2"
sha256 = "{artifact_sha}"
extractor = "wikimedia-xml"

[[corpora.files]]
path = "data/imported/external-wiki/documents.jsonl"
sha256 = "{files_sha}"
"#,
        tracked_sha = sha256_hex(TRACKED_TEXT.as_bytes()),
        artifact_sha = "0".repeat(64),
        files_sha = external_files_sha,
        extra = extra_external_fields,
    )
}

#[test]
fn test_external_registry_schema_rules() {
    // Valid external entry parses and validates.
    let ok: CorpusRegistry = toml::from_str(&registry_toml(&"a".repeat(64), "")).unwrap();
    for entry in &ok.corpora {
        entry.validate_schema().unwrap();
    }
    assert!(ok.find_corpus("external-wiki").unwrap().is_external());
    assert!(!ok.find_corpus("tracked-small").unwrap().is_external());

    // Unknown acquisition value is rejected.
    let bad_kind = registry_toml(&"a".repeat(64), "")
        .replace("acquisition = \"external\"", "acquisition = \"mystery\"");
    let bad: CorpusRegistry = toml::from_str(&bad_kind).unwrap();
    assert!(bad.corpora[1].validate_schema().is_err());

    // A tracked corpus must not carry a source_artifact.
    let tracked_with_artifact = registry_toml(&"a".repeat(64), "")
        .replace("acquisition = \"external\"", "acquisition = \"tracked\"");
    let bad2: CorpusRegistry = toml::from_str(&tracked_with_artifact).unwrap();
    assert!(bad2.corpora[1].validate_schema().is_err());

    // An external corpus must use the jsonl format its extractor emits.
    let bad_format = registry_toml(&"a".repeat(64), "")
        .replace("document_format = \"jsonl\"\ndocument_id_field = \"page_id\"\ntext_field = \"text\"\nacquisition", "document_format = \"one-document-per-line\"\nacquisition");
    let bad3: CorpusRegistry = toml::from_str(&bad_format).unwrap();
    assert!(bad3.corpora[1].validate_schema().is_err());

    // Loading through the registry loader enforces the same rules.
    let root = temp_root("schema");
    fs::write(root.join("data/source-registry/corpora.toml"), bad_kind).unwrap();
    assert!(
        CorpusRegistry::load_from_file(root.join("data/source-registry/corpora.toml")).is_err()
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn test_external_write_paths_are_confined_to_corpus_roots() {
    const GOOD_ARTIFACT: &str = "path = \"data/original/external-wiki/pages-articles.xml.bz2\"";
    const GOOD_DERIVED: &str = "path = \"data/imported/external-wiki/documents.jsonl\"";
    let base = registry_toml(&"a".repeat(64), "");
    let ok: CorpusRegistry = toml::from_str(&base).unwrap();
    ok.corpora[1].validate_schema().unwrap();

    // Artifact path outside data/original/<corpus_id>/ is rejected.
    for bad_artifact in [
        "path = \"README.md\"",
        "path = \"data/original/other-corpus/file.xml\"",
        "path = \"data/original/external-wiki\"",
        "path = \"data/imported/external-wiki/pages-articles.xml.bz2\"",
    ] {
        let toml_text = base.replace(GOOD_ARTIFACT, bad_artifact);
        let reg: CorpusRegistry = toml::from_str(&toml_text).unwrap();
        assert!(
            reg.corpora[1].validate_schema().is_err(),
            "artifact {} must be rejected",
            bad_artifact
        );
    }

    // Derived path outside data/imported/<corpus_id>/ is rejected.
    for bad_derived in [
        "path = \"Cargo.toml\"",
        "path = \"data/imported/other-corpus/documents.jsonl\"",
        "path = \"data/review-batches/foo\"",
        "path = \"data/original/external-wiki/documents.jsonl\"",
    ] {
        let toml_text = base.replace(GOOD_DERIVED, bad_derived);
        let reg: CorpusRegistry = toml::from_str(&toml_text).unwrap();
        assert!(
            reg.corpora[1].validate_schema().is_err(),
            "derived {} must be rejected",
            bad_derived
        );
    }

    // Artifact path equal to the derived path is rejected even when both are in-root
    // (here the derived path is moved into data/original, which is already rejected, so
    // check equality through the artifact side pointing at the derived location instead).
    let same = base.replace(
        GOOD_ARTIFACT,
        "path = \"data/imported/external-wiki/documents.jsonl\"",
    );
    let reg: CorpusRegistry = toml::from_str(&same).unwrap();
    assert!(reg.corpora[1].validate_schema().is_err());

    // Traversal and absolute paths stay rejected.
    for bad in [
        "path = \"data/original/external-wiki/../x.bz2\"",
        "path = \"/data/original/external-wiki/x.bz2\"",
    ] {
        let toml_text = base.replace(GOOD_ARTIFACT, bad);
        let reg: CorpusRegistry = toml::from_str(&toml_text).unwrap();
        assert!(reg.corpora[1].validate_schema().is_err(), "{}", bad);
    }
}

#[test]
fn test_duplicate_corpus_ids_rejected_at_registry_load() {
    let root = temp_root("dup");
    let dup = registry_toml(&"a".repeat(64), "").replace(
        "corpus_id = \"tracked-small\"",
        "corpus_id = \"external-wiki\"",
    );
    fs::write(root.join("data/source-registry/corpora.toml"), dup).unwrap();
    let err =
        CorpusRegistry::load_from_file(root.join("data/source-registry/corpora.toml")).unwrap_err();
    assert!(
        err.contains("Duplicate corpus_id 'external-wiki'"),
        "{}",
        err
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn test_import_skips_absent_external_corpus_and_manifest_verifies() {
    let root = temp_root("skip");
    fs::write(root.join("data/original/tracked/corpus.txt"), TRACKED_TEXT).unwrap();
    fs::write(
        root.join("data/source-registry/corpora.toml"),
        registry_toml(&"b".repeat(64), ""),
    )
    .unwrap();

    let reports =
        import_all_corpora(&root).expect("import must succeed without the external corpus");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].corpus_id, "tracked-small");

    let manifest_path = root.join("data/imported-canonical/manifest.json");
    let manifest: CanonicalImportManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.corpora.len(), 1);
    assert_eq!(
        manifest.skipped_external_corpora,
        vec!["external-wiki".to_string()]
    );

    // The manifest verifier accepts the skip because the corpus is registered as external.
    verify_canonical_manifest(&root).expect("manifest with skipped external corpus must verify");

    // Two passes are byte-identical (determinism).
    let first = fs::read(&manifest_path).unwrap();
    import_all_corpora(&root).unwrap();
    assert_eq!(first, fs::read(&manifest_path).unwrap());

    // If the registry later marks the corpus as tracked, the old manifest no longer verifies.
    let tracked = registry_toml(&"b".repeat(64), "")
        .replace("acquisition = \"external\"", "acquisition = \"tracked\"")
        .replace(
            "[corpora.source_artifact]\nurl = \"https://example.org/dump/pages-articles.xml.bz2\"\npath = \"data/original/external-wiki/pages-articles.xml.bz2\"\nsha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\nextractor = \"wikimedia-xml\"\n",
            "",
        );
    fs::write(root.join("data/source-registry/corpora.toml"), tracked).unwrap();
    assert!(verify_canonical_manifest(&root).is_err());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn test_import_includes_external_corpus_when_present() {
    let root = temp_root("present");
    fs::write(root.join("data/original/tracked/corpus.txt"), TRACKED_TEXT).unwrap();
    let docs = "{\"page_id\":\"1\",\"title\":\"A\",\"text\":\"rojbaş cîhan\"}\n{\"page_id\":\"2\",\"title\":\"B\",\"text\":\"ez kurd im\"}\n";
    fs::create_dir_all(root.join("data/imported/external-wiki")).unwrap();
    fs::write(
        root.join("data/imported/external-wiki/documents.jsonl"),
        docs,
    )
    .unwrap();
    fs::write(
        root.join("data/source-registry/corpora.toml"),
        registry_toml(&sha256_hex(docs.as_bytes()), ""),
    )
    .unwrap();

    let reports = import_all_corpora(&root).unwrap();
    assert_eq!(reports.len(), 2);
    let manifest: CanonicalImportManifest = serde_json::from_slice(
        &fs::read(root.join("data/imported-canonical/manifest.json")).unwrap(),
    )
    .unwrap();
    assert!(manifest.skipped_external_corpora.is_empty());
    assert_eq!(manifest.corpora.len(), 2);
    assert_eq!(
        manifest
            .corpora
            .iter()
            .find(|c| c.corpus_id == "external-wiki")
            .unwrap()
            .document_count,
        2
    );
    verify_canonical_manifest(&root).unwrap();

    // A present external file with the wrong hash is a hard error, never a silent skip.
    fs::write(
        root.join("data/source-registry/corpora.toml"),
        registry_toml(&"c".repeat(64), ""),
    )
    .unwrap();
    assert!(import_all_corpora(&root).is_err());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn test_acquire_corpus_refuses_tracked_and_fails_closed_on_artifact_hash() {
    let root = temp_root("acquire");
    fs::write(root.join("data/original/tracked/corpus.txt"), TRACKED_TEXT).unwrap();
    fs::write(
        root.join("data/source-registry/corpora.toml"),
        registry_toml(&"d".repeat(64), ""),
    )
    .unwrap();

    // Tracked corpora are never acquired.
    assert!(acquire_corpus("tracked-small", &root).is_err());
    assert!(acquire_corpus("unknown-corpus", &root).is_err());

    // A present artifact whose hash does not match the registry triggers a download attempt
    // to an unreachable example URL, which must fail (never accept the wrong bytes).
    fs::create_dir_all(root.join("data/original/external-wiki")).unwrap();
    fs::write(
        root.join("data/original/external-wiki/pages-articles.xml.bz2"),
        b"not a dump",
    )
    .unwrap();
    let err = acquire_corpus("external-wiki", &root).unwrap_err();
    assert!(
        err.contains("HTTP fetch failed") || err.contains("mismatch"),
        "unexpected error: {}",
        err
    );

    fs::remove_dir_all(&root).unwrap();
}
