//! The corpus registry's `[corpora.redistribution]` table is a human's determination that the
//! code copies verbatim (into the language model manifest and the release bundle) and never
//! decides: absent means `pending-review`, an unknown value or an incomplete record is
//! refused at load time, and the committed registry records the project owner's
//! determination of 2026-09-19 for the corpora the packs derive from.

use data_builder_lib::corpus::registry::{
    is_valid_gregorian_date, CorpusRegistry, REDISTRIBUTION_ALLOWED, REDISTRIBUTION_PENDING_REVIEW,
};
use std::fs;
use std::path::Path;

fn ws_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn registry_toml(redistribution: &str) -> String {
    format!(
        r#"
[[corpora]]
corpus_id = "test-corpus"
corpus_name = "Test corpus"
language = "ku-Latn"
license = "CC BY-SA 4.0"
license_spdx = "CC-BY-SA-4.0"
license_url = "https://creativecommons.org/licenses/by-sa/4.0/"
url = "https://example.org/corpus"
version = "1"
description = "test"
attribution = "Test contributors"
notes = "test"
{redistribution}
[[corpora.files]]
path = "data/original/test-corpus/corpus.txt"
sha256 = "{zeros}"
"#,
        redistribution = redistribution,
        zeros = "0".repeat(64)
    )
}

fn load(toml: &str) -> Result<CorpusRegistry, String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corpora.toml");
    fs::write(&path, toml).unwrap();
    CorpusRegistry::load_from_file(&path)
}

const ALLOWED: &str = r#"
[corpora.redistribution]
determination = "allowed"
determined_by = "project owner (test)"
determined_on = "2026-09-19"
basis = "Unrestricted broad reuse, including commercial use; upstream licences, attribution and ShareAlike obligations remain."
"#;

#[test]
fn absent_table_means_pending_review_and_a_recorded_determination_is_copied_verbatim() {
    let none = load(&registry_toml("")).unwrap();
    let entry = none.find_corpus("test-corpus").unwrap();
    assert!(entry.redistribution.is_none());
    assert_eq!(
        entry.redistribution_determination(),
        REDISTRIBUTION_PENDING_REVIEW
    );

    let allowed = load(&registry_toml(ALLOWED)).unwrap();
    let entry = allowed.find_corpus("test-corpus").unwrap();
    assert_eq!(entry.redistribution_determination(), REDISTRIBUTION_ALLOWED);
    let r = entry.redistribution.as_ref().unwrap();
    assert_eq!(r.determined_by, "project owner (test)");
    assert_eq!(r.determined_on, "2026-09-19");
    assert!(r.basis.contains("commercial use"));
}

#[test]
fn unknown_or_incomplete_determinations_are_refused_at_load() {
    let err = load(&registry_toml(
        &ALLOWED.replace("\"allowed\"", "\"approved\""),
    ))
    .unwrap_err();
    assert!(err.contains("redistribution.determination"), "{err}");
    assert!(err.contains("approved"), "{err}");

    let err = load(&registry_toml(
        &ALLOWED.replace("\"2026-09-19\"", "\"19 Sep 2026\""),
    ))
    .unwrap_err();
    assert!(err.contains("determined_on"), "{err}");

    let err = load(&registry_toml(&ALLOWED.replace(
        "basis = \"Unrestricted broad reuse, including commercial use; upstream licences, attribution and ShareAlike obligations remain.\"",
        "basis = \"  \"",
    )))
    .unwrap_err();
    assert!(err.contains("redistribution.basis"), "{err}");

    // A table without its required fields is a parse error, not a silent default.
    let err = load(&registry_toml(
        "[corpora.redistribution]\ndetermination = \"allowed\"\n",
    ))
    .unwrap_err();
    assert!(
        err.contains("determined_by") || err.contains("missing field"),
        "{err}"
    );
}

#[test]
fn committed_registry_records_the_owner_determination_of_2026_09_19() {
    let registry =
        CorpusRegistry::load_from_file(ws_root().join("data/source-registry/corpora.toml"))
            .unwrap();
    // The only registered corpus: the external Wikipedia corpus the language model derives
    // from (no test fixture is ever registered here).
    assert_eq!(registry.corpora.len(), 1);
    let entry = registry.find_corpus("kuwiki").unwrap();
    assert_eq!(entry.redistribution_determination(), REDISTRIBUTION_ALLOWED);
    let r = entry.redistribution.as_ref().unwrap();
    assert_eq!(r.determined_on, "2026-09-19");
    assert!(r.determined_by.contains("project owner"));
    assert!(r
        .basis
        .contains("unrestricted broad reuse, including commercial use"));
}

#[test]
fn determined_on_must_be_a_real_gregorian_calendar_date() {
    for ok in [
        "2026-09-19",
        "2024-02-29",
        "2000-02-29",
        "2026-12-31",
        "0001-01-01",
    ] {
        assert!(is_valid_gregorian_date(ok), "{ok}");
        assert!(
            load(&registry_toml(
                &ALLOWED.replace("\"2026-09-19\"", &format!("\"{ok}\""))
            ))
            .is_ok(),
            "{ok}"
        );
    }
    for bad in [
        "2026-13-01",
        "2026-00-10",
        "2026-02-30",
        "2026-02-29",
        "1900-02-29",
        "2100-02-29",
        "2026-04-31",
        "2026-09-00",
        "2026-09-32",
        "0000-01-01",
        "19 Sep 2026",
        "2026-9-19",
        "2026/09/19",
        "2026-09-19T00:00",
        "20260919",
        "",
    ] {
        assert!(!is_valid_gregorian_date(bad), "{bad}");
        let err = load(&registry_toml(
            &ALLOWED.replace("\"2026-09-19\"", &format!("\"{bad}\"")),
        ))
        .unwrap_err();
        assert!(err.contains("determined_on"), "{bad}: {err}");
    }
}
