//! `inspect-word` is read-only and reports the repository's own records for a word: pack
//! membership from the authoritative resolver, source evidence, and the human review
//! history per source. It must never expose corpus text, document ids or context
//! references, and must never merge per-source statuses into one verdict.

use data_builder_lib::review::inspect::{inspect_word, render_json, render_text};
use data_builder_lib::review::ReviewDecisionRecord;
use std::path::Path;

fn ws_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn assert_no_corpus_material(json: &str) {
    for forbidden in [
        "context_references",
        "document_id",
        "\"context\"",
        "snippet",
        "source_text",
    ] {
        assert!(
            !json.contains(forbidden),
            "inspection output must not contain {:?}",
            forbidden
        );
    }
}

#[test]
fn seed_word_is_in_every_pack_with_no_review_targets() {
    let r = inspect_word(ws_root(), "Kurdî").unwrap();
    assert_eq!(r.normalized, "kurdî");
    assert!(r.membership.seed && r.membership.reviewed && r.membership.experimental_full);
    // The seed records the display form "Kurdî"; every display form is the same identity.
    assert!(!r.membership.display_forms.is_empty());
    for form in &r.membership.display_forms {
        assert_eq!(data_builder_lib::normalize_text(form), "kurdî");
    }
    assert!(r
        .membership
        .pack_sources
        .contains(&"manual-seed".to_string()));
    let json = render_json(&r);
    assert_no_corpus_material(&json);
    let text = render_text(&r);
    assert!(text.contains("seed:              yes"));
}

#[test]
fn hunspell_decided_word_reports_its_source_specific_status() {
    // "wela" carries a needs_source_investigation decision in the Hunspell decisions file.
    let r = inspect_word(ws_root(), "wela").unwrap();
    let hunspell: Vec<_> = r
        .review_history
        .iter()
        .filter(|h| h.source_id == "kurdish-hunspell-kmr")
        .collect();
    assert!(
        !hunspell.is_empty(),
        "wela must appear in the Hunspell review history"
    );
    assert!(hunspell
        .iter()
        .any(|h| h.status == "needs_source_investigation"));
    assert!(r.previously_assigned);
    assert!(r.sources.hunspell.is_some());
    // A needs_* status keeps the word out of reviewed; membership is reported, never inferred.
    assert!(!r.membership.reviewed);
    assert_no_corpus_material(&render_json(&r));
}

#[test]
fn kuwiki_batch_word_reports_batch_evidence_and_decision() {
    // "kategorî" is rank 1 of kuwiki-batch-001 with an experimental_only decision.
    let r = inspect_word(ws_root(), "kategorî").unwrap();
    let batch = r
        .sources
        .kuwiki_batches
        .iter()
        .find(|b| b.batch_id == "kuwiki-batch-001")
        .expect("batch evidence");
    assert_eq!(batch.batch_rank, 1);
    assert!(batch.document_count > 0 && batch.token_count >= batch.document_count);
    let decision = r
        .review_history
        .iter()
        .find(|h| h.source_id == "kuwiki-batch-001")
        .expect("batch decision");
    assert_eq!(decision.status, "experimental_only");
    assert_eq!(decision.reviewer_id.as_deref(), Some("ferhatguneri"));
    assert!(r.previously_assigned);
    assert!(r.membership.experimental_full);
    assert!(!r.membership.reviewed);
    assert_no_corpus_material(&render_json(&r));
    let text = render_text(&r);
    assert!(text.contains("kuwiki-batch-001: rank 1"));
    assert!(text.contains("experimental_only"));
}

#[test]
fn statuses_are_reported_per_source_and_never_flattened() {
    // Every history entry names its source; the structure has no global status field.
    let r = inspect_word(ws_root(), "kategorî").unwrap();
    let json: serde_json::Value = serde_json::from_str(&render_json(&r)).unwrap();
    assert!(json.get("status").is_none());
    assert!(json.get("review_status").is_none());
    for h in json["review_history"].as_array().unwrap() {
        assert!(h["source_id"].as_str().is_some());
        assert!(h["status"].as_str().is_some());
    }
}

#[test]
fn unknown_word_has_no_membership_sources_or_history() {
    let r = inspect_word(ws_root(), "xqzvwplt").unwrap();
    assert!(!r.membership.seed && !r.membership.reviewed && !r.membership.experimental_full);
    assert!(r.sources.hunspell.is_none());
    assert!(r.sources.kuwiki_model.is_none());
    assert!(r.sources.kuwiki_batches.is_empty());
    assert!(r.review_history.is_empty());
    assert!(!r.previously_assigned);
    assert!(render_text(&r).contains("decisions:           none"));
}

#[test]
fn inspection_is_read_only_and_uses_the_canonical_identity() {
    let root = ws_root();
    let decisions = root.join("data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl");
    let before = std::fs::read(&decisions).unwrap();
    let a = inspect_word(root, "KURDÎ").unwrap();
    let b = inspect_word(root, "kurdî").unwrap();
    assert_eq!(a.normalized, b.normalized);
    assert_eq!(a.membership, b.membership);
    assert_eq!(a.review_history, b.review_history);
    assert_eq!(std::fs::read(&decisions).unwrap(), before);
    assert!(inspect_word(root, "   ").is_err());

    // Sanity: decisions the inspector reports exist verbatim in the decision file.
    let text = std::fs::read_to_string(&decisions).unwrap();
    let ids: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str::<ReviewDecisionRecord>(l)
                .unwrap()
                .target_id
        })
        .collect();
    let r = inspect_word(root, "wela").unwrap();
    for h in r.review_history.iter().filter(|h| h.status != "pending") {
        if h.source_id == "kurdish-hunspell-kmr" {
            assert!(ids.contains(&h.target_id));
        }
    }
}
