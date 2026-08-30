//! Unit & Integration tests for MediaWiki XML extractor.

use data_builder_lib::corpus::extractors::{extract_wikimedia_file, strip_wikitext};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_wikimedia_extractor_sample_fixture() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join("tests/fixtures/kuwiki_sample.xml");

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let out_1 = temp_dir.path().join("out_1.jsonl");
    let out_2 = temp_dir.path().join("out_2.jsonl");

    let report_1 =
        extract_wikimedia_file(&fixture_path, &out_1, None).expect("Extraction run 1 failed");
    let report_2 =
        extract_wikimedia_file(&fixture_path, &out_2, None).expect("Extraction run 2 failed");

    // Verify statistics
    assert_eq!(report_1.total_pages_seen, 4);
    assert_eq!(report_1.main_namespace_pages, 1);
    assert_eq!(report_1.redirect_pages_excluded, 1);
    assert_eq!(report_1.non_main_pages_excluded, 2);
    assert_eq!(report_1.extracted_documents, 1);

    // Verify byte-identical output across 2 runs
    let bytes_1 = fs::read(&out_1).expect("Failed to read out_1");
    let bytes_2 = fs::read(&out_2).expect("Failed to read out_2");

    assert_eq!(
        bytes_1, bytes_2,
        "Extraction must be 100% byte-identical across runs!"
    );
    assert_eq!(
        report_1.output_checksum_sha256,
        report_2.output_checksum_sha256
    );
}

#[test]
fn test_strip_wikitext_ref_variations() {
    // 1. Self-closing <ref name="x" />
    let input1 = "Prose before<ref name=\"x\" /> prose after.";
    let stripped1 = strip_wikitext(input1);
    assert_eq!(stripped1, "Prose before prose after.");

    // 2. <references />
    let input2 = "Prose before<references /> prose after.";
    let stripped2 = strip_wikitext(input2);
    assert_eq!(stripped2, "Prose before prose after.");

    // 3. Normal <ref>...</ref>
    let input3 = "Prose before<ref>citation content</ref> prose after.";
    let stripped3 = strip_wikitext(input3);
    assert_eq!(stripped3, "Prose before prose after.");

    // 4. Case-insensitive <REF ...>...</REF>
    let input4 = "Prose before<REF NAME=\"ALT\">citation content</REF> prose after.";
    let stripped4 = strip_wikitext(input4);
    assert_eq!(stripped4, "Prose before prose after.");

    // 5. Non-ref tag starting with <ref (e.g. <reflection>) must NOT be treated as <ref>
    let input5 = "Prose before <reflection>important</reflection> prose after.";
    let stripped5 = strip_wikitext(input5);
    assert_eq!(stripped5, "Prose before important prose after.");
}

#[test]
fn test_wikimedia_extractor_malformed_xml_transactional() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let input_path = temp_dir.path().join("malformed.xml");
    let output_path = temp_dir.path().join("out.jsonl");

    // Invalid UTF-8 sequence causes XML stream parse error
    let malformed_xml = b"<mediawiki><page><title>Bad</title>\xFF\xFE\xFD</page></mediawiki>";
    fs::write(&input_path, malformed_xml).unwrap();

    let res = extract_wikimedia_file(&input_path, &output_path, None);
    assert!(res.is_err());
    assert!(
        !output_path.exists(),
        "Partial output file must NOT exist when extraction fails!"
    );
}

#[test]
fn test_wikimedia_extractor_checksum_rejection_transactional() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let input_path = temp_dir.path().join("sample.xml");
    let output_path = temp_dir.path().join("out.jsonl");

    let sample_xml = "<mediawiki><page><title>Good</title><ns>0</ns><id>1</id><text>Text</text></page></mediawiki>";
    fs::write(&input_path, sample_xml).unwrap();

    // Pass invalid expected SHA-1
    let bad_sha1 = "0000000000000000000000000000000000000000";
    let res = extract_wikimedia_file(&input_path, &output_path, Some(bad_sha1));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("checksum mismatch"));
    assert!(
        !output_path.exists(),
        "Output file must NOT exist when checksum verification fails!"
    );
}
