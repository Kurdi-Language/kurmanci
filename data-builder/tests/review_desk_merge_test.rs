//! The Review Desk import boundary (`scripts/review-desk/merge_review_desk_export.py` and
//! `prepare_export.py`), driven with small fixtures in a temporary root: every exported
//! decision must bind to a real queue target of its source, already-decided targets are an
//! idempotent no-op when identical and a fail-closed conflict when different, any failure
//! leaves the store byte-identical, metadata-change decisions keep their explicit human
//! replacement, a policy rewrite keeps the original Review Desk choice recoverable, and the
//! real current store re-imports as an idempotent no-op. No clock, network or service.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn ws_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

const SOURCE: &str = "kurdish-hunspell-kmr";
const T_ROJ: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const T_SEV: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const T_BAS: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const T_HEVAL: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const T_UNKNOWN: &str = "9999999999999999999999999999999999999999999999999999999999999999";
const G_CONFLICT: &str = "5555555555555555555555555555555555555555555555555555555555555555";

fn queue_record(target_id: &str, display: &str, normalized: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "review-queue-v1", "rule_id": "HUNSPELL_ONLY_V1", "rule_version": "1",
        "target_type": "entry", "target_id": target_id, "display": display, "normalized": normalized,
        "source_id": SOURCE, "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "source_lines": [1], "flags": "", "morphology": [], "part_of_speech": "noun",
        "reason_codes": ["HUNSPELL_ONLY"], "suggested_action": "retain",
        "generated_status": "unreviewed", "effective_review_status": "unreviewed",
        "decision_entry_id": null, "queue_categories": ["hunspell-only"]
    })
}

fn decision(target_id: &str, status: &str, notes: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "review-decision-v1", "target_type": "entry", "target_id": target_id,
        "source_id": SOURCE, "review_status": status, "reviewer_id": "tester",
        "review_date": "2026-09-17", "review_notes": notes, "evidence": []
    })
}

fn export(decisions: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "review-desk-export-v1", "queue_id": "fixture-queue", "source_id": SOURCE,
        "source_revision": "88131d6878ef7fa3ee114aa554adc385ff85b44c",
        "exported_at": "2026-09-17T00:00:00Z", "decisions": decisions
    })
}

/// A root with a three-entry review pool, one policy-excluded entry, one conflict group and a
/// store holding one decision.
fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let qdir = tmp.path().join("data/review-queues").join(SOURCE);
    fs::create_dir_all(&qdir).unwrap();
    let pool: Vec<String> = [
        queue_record(T_ROJ, "roj", "roj"),
        queue_record(T_SEV, "şev", "şev"),
        queue_record(T_BAS, "baş", "baş"),
    ]
    .iter()
    .map(|v| v.to_string())
    .collect();
    fs::write(qdir.join("hunspell-only.jsonl"), pool.join("\n") + "\n").unwrap();
    // The authoritative generator's classification of an out-of-alphabet entry: it lives in
    // alphabet-policy-excluded.jsonl with the offending code point in its reason codes.
    let mut excluded = queue_record(T_HEVAL, "hérault", "hérault");
    excluded["rule_id"] = serde_json::json!("ALPHABET_POLICY_V1");
    excluded["reason_codes"] = serde_json::json!(["OUT_OF_ALPHABET", "'é' (U+00E9)"]);
    excluded["suggested_action"] = serde_json::json!("excluded_by_alphabet_policy");
    excluded["queue_categories"] = serde_json::json!(["alphabet-policy-excluded"]);
    fs::write(
        qdir.join("alphabet-policy-excluded.jsonl"),
        excluded.to_string() + "\n",
    )
    .unwrap();
    let group = serde_json::json!({
        "schema_version": "review-queue-v1", "rule_id": "METADATA_CONFLICT_V1", "rule_version": "1",
        "target_type": "conflict_group", "target_id": G_CONFLICT, "normalized": "sê",
        "member_entry_ids": [], "members": [], "differing_fields": ["flags"],
        "reason_codes": ["METADATA_CONFLICT"], "suggested_action": "manual_review",
        "generated_status": "unreviewed", "effective_review_status": "unreviewed",
        "decision_entry_id": null, "queue_categories": ["metadata-conflict-groups"]
    });
    fs::write(
        qdir.join("metadata-conflict-groups.jsonl"),
        group.to_string() + "\n",
    )
    .unwrap();
    let ddir = tmp.path().join("data/review-decisions").join(SOURCE);
    fs::create_dir_all(&ddir).unwrap();
    fs::write(
        ddir.join("decisions.jsonl"),
        decision(T_ROJ, "approved", "Human-approved lexical entry").to_string() + "\n",
    )
    .unwrap();
    tmp
}

fn store_path(root: &Path) -> PathBuf {
    root.join("data/review-decisions")
        .join(SOURCE)
        .join("decisions.jsonl")
}

fn run_merge(root: &Path, export: &serde_json::Value, apply: bool) -> (bool, String) {
    let export_path = root.join("export.json");
    fs::write(&export_path, export.to_string()).unwrap();
    let mut cmd = Command::new("python3");
    cmd.arg(ws_root().join("scripts/review-desk/merge_review_desk_export.py"))
        .arg(&export_path)
        .arg("--root")
        .arg(root);
    if apply {
        cmd.arg("--apply");
    }
    let out = cmd.output().expect("python3 must be available");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

fn store_lines(root: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(store_path(root))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn valid_known_targets_merge_in_export_order_and_keep_replacement_metadata() {
    let tmp = fixture();
    let root = tmp.path();
    let mut meta = decision(T_BAS, "approved_with_metadata_change", "corrected metadata");
    meta["replacement_metadata"] = serde_json::json!({
        "display": "Baş", "normalized": "baş", "flags": "AN", "morphology": ["po:adj"], "part_of_speech": "adjective"
    });
    let exp = export(vec![
        decision(
            T_SEV,
            "experimental_only",
            "Human review: retain only in experimental vocabulary.",
        ),
        meta,
    ]);
    let (ok, text) = run_merge(root, &exp, false);
    assert!(ok, "{}", text);
    assert!(text.contains("new: 2"), "{}", text);
    assert_eq!(store_lines(root).len(), 1, "dry run writes nothing");
    let (ok, text) = run_merge(root, &exp, true);
    assert!(ok, "{}", text);
    let lines = store_lines(root);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1]["target_id"], T_SEV);
    assert_eq!(lines[2]["target_id"], T_BAS);
    assert_eq!(lines[2]["review_status"], "approved_with_metadata_change");
    assert_eq!(lines[2]["replacement_metadata"]["display"], "Baş");
    assert_eq!(
        lines[2]["replacement_metadata"]["part_of_speech"],
        "adjective"
    );
    assert_eq!(
        lines[2]["replacement_metadata"]["morphology"],
        serde_json::json!(["po:adj"])
    );
    assert_eq!(lines[2]["reviewer_id"], "tester");
}

#[test]
fn unknown_target_source_mismatch_and_unsupported_type_fail_closed_without_writing() {
    let tmp = fixture();
    let root = tmp.path();
    let before = fs::read(store_path(root)).unwrap();

    // unknown target id (well-formed SHA-256 shape, but not in the queues)
    let (ok, text) = run_merge(
        root,
        &export(vec![decision(T_UNKNOWN, "approved", "x")]),
        true,
    );
    assert!(!ok);
    assert!(
        text.contains("not in the kurdish-hunspell-kmr review queues"),
        "{}",
        text
    );
    assert_eq!(fs::read(store_path(root)).unwrap(), before);

    // source mismatch
    let mut foreign = decision(T_SEV, "approved", "x");
    foreign["source_id"] = serde_json::json!("kuwiki-batch-001");
    let (ok, text) = run_merge(root, &export(vec![foreign]), true);
    assert!(!ok);
    assert!(text.contains("schema/source mismatch"), "{}", text);
    assert_eq!(fs::read(store_path(root)).unwrap(), before);

    // unsupported target type: a real conflict-group target is refused explicitly
    let mut group = decision(G_CONFLICT, "needs_linguist", "x");
    group["target_type"] = serde_json::json!("conflict_group");
    let (ok, text) = run_merge(root, &export(vec![group]), true);
    assert!(!ok);
    assert!(text.contains("unsupported target_type"), "{}", text);
    assert_eq!(fs::read(store_path(root)).unwrap(), before);

    // one bad record poisons the whole export: the valid one is not written either
    let (ok, _) = run_merge(
        root,
        &export(vec![
            decision(T_SEV, "approved", "x"),
            decision(T_UNKNOWN, "approved", "x"),
        ]),
        true,
    );
    assert!(!ok);
    assert_eq!(fs::read(store_path(root)).unwrap(), before);
}

#[test]
fn identical_repeat_is_idempotent_and_a_differing_repeat_fails_closed() {
    let tmp = fixture();
    let root = tmp.path();
    let before = fs::read(store_path(root)).unwrap();

    // identical to the stored decision (representation differences only): no-op
    let mut same = decision(T_ROJ, "approved", "Human-approved lexical entry");
    same.as_object_mut().unwrap().remove("evidence");
    let (ok, text) = run_merge(root, &export(vec![same]), true);
    assert!(ok, "{}", text);
    assert!(
        text.contains("identical-already-stored (no-op): 1"),
        "{}",
        text
    );
    assert!(text.contains("nothing to append"), "{}", text);
    assert_eq!(fs::read(store_path(root)).unwrap(), before);

    // a different decision for an already-decided target: refused, store untouched
    let (ok, text) = run_merge(
        root,
        &export(vec![decision(
            T_ROJ,
            "rejected_from_default_pack",
            "changed my mind",
        )]),
        true,
    );
    assert!(!ok);
    assert!(text.contains("conflicting-already-stored: 1"), "{}", text);
    assert!(
        text.contains(&format!("conflict {}", &T_ROJ[..12])),
        "{}",
        text
    );
    assert!(
        text.contains("review_status: stored='approved' export='rejected_from_default_pack'"),
        "{}",
        text
    );
    assert_eq!(fs::read(store_path(root)).unwrap(), before);
}

#[test]
fn prepare_export_keeps_the_original_review_desk_choice_recoverable() {
    let tmp = fixture();
    let root = tmp.path();
    let exp = export(vec![
        decision(T_HEVAL, "approved", "Human-approved lexical entry"),
        decision(T_SEV, "approved", "Human-approved lexical entry"),
    ]);
    let export_path = root.join("export.json");
    fs::write(&export_path, exp.to_string()).unwrap();
    let prepared = root.join("prepared.json");
    let audit = root.join("audit.json");
    let out = Command::new("python3")
        .arg(ws_root().join("scripts/review-desk/prepare_export.py"))
        .arg(&export_path)
        .arg(&prepared)
        .arg("--audit-json")
        .arg(&audit)
        .arg("--root")
        .arg(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let p: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&prepared).unwrap()).unwrap();
    let d = p["decisions"].as_array().unwrap();
    assert_eq!(d.len(), 2);
    // the in-alphabet approval passes through untouched
    assert_eq!(d[1]["review_status"], "approved");
    assert_eq!(d[1]["review_notes"], "Human-approved lexical entry");
    // the approval of the entry the authoritative generator classified as excluded is
    // rewritten with the original choice preserved; the characters come from the artifact
    assert_eq!(d[0]["review_status"], "rejected_from_default_pack");
    let notes = d[0]["review_notes"].as_str().unwrap();
    assert!(
        notes.contains("alphabet policy") && notes.contains("approved (tester, 2026-09-17)"),
        "{}",
        notes
    );
    assert!(notes.contains("U+00E9"), "{}", notes);
    let evidence = d[0]["evidence"].as_array().unwrap();
    assert!(
        evidence.iter().any(|e| e
            .as_str()
            .unwrap()
            .contains("review-desk-original:status=approved;reviewer=tester;date=2026-09-17")),
        "{:?}",
        evidence
    );
    let a: serde_json::Value = serde_json::from_str(&fs::read_to_string(&audit).unwrap()).unwrap();
    assert_eq!(a["schema_version"], "review-desk-prepare-audit-v1");
    assert_eq!(a["counts"]["policy_rejected"], 1);
    assert_eq!(a["rewritten"][0]["review_desk_status"], "approved");
    assert_eq!(
        a["rewritten"][0]["prepared_status"],
        "rejected_from_default_pack"
    );
    assert!(a["rewritten"][0]["outside_characters"]
        .as_str()
        .unwrap()
        .contains("U+00E9"));
    // the prepared file merges (the rewritten record carries notes and evidence)
    let (ok, text) = run_merge(root, &p, true);
    assert!(ok, "{}", text);
    assert_eq!(store_lines(root).len(), 3);
}

/// The real store: re-exporting every stored Hunspell decision (which includes the 1,000
/// Review Desk batch records, transcribed unchanged) is an idempotent no-op in dry run and
/// in apply mode, so the current batch is accepted without changing any decision.
#[test]
fn current_real_store_reimports_as_an_idempotent_no_op() {
    let real = ws_root()
        .join("data/review-decisions")
        .join(SOURCE)
        .join("decisions.jsonl");
    let real_queues = ws_root().join("data/review-queues").join(SOURCE);
    if !real.is_file() || !real_queues.is_dir() {
        eprintln!("skipping: real store or queues not present");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("data/review-decisions").join(SOURCE)).unwrap();
    fs::copy(&real, store_path(root)).unwrap();
    let qdir = root.join("data/review-queues").join(SOURCE);
    fs::create_dir_all(&qdir).unwrap();
    for e in fs::read_dir(&real_queues).unwrap().flatten() {
        if e.path().extension().map(|x| x == "jsonl").unwrap_or(false) {
            fs::copy(e.path(), qdir.join(e.file_name())).unwrap();
        }
    }
    let before = fs::read(store_path(root)).unwrap();
    let stored = store_lines(root);
    let batch: Vec<serde_json::Value> = stored
        .iter()
        .filter(|d| {
            d["reviewer_id"] == "ferhatguneri" && d["review_date"].as_str().unwrap() >= "2026-09-10"
        })
        .cloned()
        .collect();
    assert!(
        batch.len() >= 1000,
        "the Review Desk batch is in the store ({} records)",
        batch.len()
    );
    let exp = export(stored.clone());
    let (ok, text) = run_merge(root, &exp, true);
    assert!(ok, "{}", text);
    assert!(
        text.contains(&format!(
            "identical-already-stored (no-op): {}",
            stored.len()
        )),
        "{}",
        text
    );
    assert!(
        text.contains("conflicting-already-stored: 0") && text.contains("invalid: 0"),
        "{}",
        text
    );
    assert_eq!(
        fs::read(store_path(root)).unwrap(),
        before,
        "store byte-identical"
    );
    let metadata_changes = stored
        .iter()
        .filter(|d| d["review_status"] == "approved_with_metadata_change")
        .count();
    assert_eq!(metadata_changes, 8);
    eprintln!(
        "{} stored decisions re-imported as no-ops ({} in the batch)",
        stored.len(),
        batch.len()
    );
}

/// A metadata change on an authoritatively excluded source target is the human's correction
/// of the source form: `prepare_export.py` must pass it through exactly as reviewed (status,
/// reviewer, date, notes, evidence, replacement metadata) and leave the replacement form's
/// eligibility to the authoritative Rust validation/resolution path.
#[test]
fn prepare_export_passes_metadata_change_on_excluded_target_through_unchanged() {
    let tmp = fixture();
    let root = tmp.path();
    let mut corrected = decision(
        T_HEVAL,
        "approved_with_metadata_change",
        "corrected spelling",
    );
    corrected["review_date"] = serde_json::json!("2026-09-12");
    corrected["evidence"] =
        serde_json::json!(["reviewer note: source form is a loanword spelling"]);
    corrected["replacement_metadata"] = serde_json::json!({
        "display": "hêralt", "normalized": "hêralt", "flags": "", "morphology": [], "part_of_speech": "noun"
    });
    let exp = export(vec![corrected.clone()]);
    let export_path = root.join("export.json");
    fs::write(&export_path, exp.to_string()).unwrap();
    let prepared = root.join("prepared.json");
    let audit = root.join("audit.json");
    let out = Command::new("python3")
        .arg(ws_root().join("scripts/review-desk/prepare_export.py"))
        .arg(&export_path)
        .arg(&prepared)
        .arg("--audit-json")
        .arg(&audit)
        .arg("--root")
        .arg(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let p: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&prepared).unwrap()).unwrap();
    let d = &p["decisions"][0];
    for field in [
        "review_status",
        "reviewer_id",
        "review_date",
        "review_notes",
        "evidence",
        "replacement_metadata",
        "target_id",
    ] {
        assert_eq!(d[field], corrected[field], "{} must be untouched", field);
    }
    assert_eq!(d["review_status"], "approved_with_metadata_change");
    let a: serde_json::Value = serde_json::from_str(&fs::read_to_string(&audit).unwrap()).unwrap();
    assert_eq!(a["counts"]["policy_rejected"], 0);
    assert_eq!(a["counts"]["passed_through"], 1);
    // The prepared decision merges unchanged; replacement eligibility is Rust's to decide.
    let (ok, text) = run_merge(root, &p, true);
    assert!(ok, "{}", text);
    let stored = store_lines(root);
    assert_eq!(
        stored.last().unwrap()["replacement_metadata"]["normalized"],
        "hêralt"
    );
    assert_eq!(
        stored.last().unwrap()["review_status"],
        "approved_with_metadata_change"
    );
}

/// Review Desk tooling consumes the policy-clean pool produced by the authoritative Rust
/// review infrastructure and defines no alphabet of its own: the scripts carry no alphabet
/// or word-internal-punctuation table and no character-eligibility function.
#[test]
fn review_desk_scripts_define_no_independent_alphabet_policy() {
    for name in [
        "build_hunspell_queue.py",
        "prepare_export.py",
        "merge_review_desk_export.py",
    ] {
        let src = fs::read_to_string(ws_root().join("scripts/review-desk").join(name)).unwrap();
        for forbidden in [
            "ALPHABET",
            "WORD_INTERNAL",
            "def outside(",
            "abcçdeêfghiîjklmnopqrsştuûvwxyz",
        ] {
            assert!(
                !src.contains(forbidden),
                "{} defines its own policy: {}",
                name,
                forbidden
            );
        }
    }
    let prep = fs::read_to_string(ws_root().join("scripts/review-desk/prepare_export.py")).unwrap();
    assert!(prep.contains("alphabet-policy-excluded.jsonl"));
}

/// The queue builder trusts hunspell-only.jsonl as the policy-clean pool and refuses to run
/// when the authoritative artifacts contradict each other (an excluded target also in the
/// pool). Driven on a tiny corpus so the Wikipedia join is instant.
#[test]
fn queue_builder_consumes_the_clean_pool_and_refuses_artifact_overlap() {
    let tmp = fixture();
    let root = tmp.path();
    let qdir = root.join("data/review-queues").join(SOURCE);
    for extra in [
        "capitalization-anomalies",
        "digit-only",
        "punctuation-only",
        "rare-code-points",
        "short-and-long-forms",
        "suspicious-entries",
        "symbol-only",
        "parser-rejections",
    ] {
        fs::write(qdir.join(format!("{}.jsonl", extra)), "").unwrap();
    }
    let docs = root.join("data/imported/kuwiki");
    fs::create_dir_all(&docs).unwrap();
    fs::write(
        docs.join("documents.jsonl"),
        concat!(
            "{\"title\":\"a\",\"text\":\"şev roj baş şev\"}\n",
            "{\"title\":\"b\",\"text\":\"şev hérault\"}\n"
        ),
    )
    .unwrap();
    let run = |root: &Path| {
        let out = root.join("queue.json");
        let o = Command::new("python3")
            .arg(ws_root().join("scripts/review-desk/build_hunspell_queue.py"))
            .arg(&out)
            .arg("10")
            .arg("fixture-queue")
            .arg("--root")
            .arg(root)
            .output()
            .unwrap();
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).to_string(),
            out,
        )
    };
    let (ok, err, out) = run(root);
    assert!(ok, "{}", err);
    let q: serde_json::Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    let displays: Vec<&str> = q["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["display"].as_str().unwrap())
        .collect();
    // roj is already decided; şev and baş are the attested pending pool entries, ranked by
    // document count; the excluded entry is never a candidate because it is not in the pool
    assert_eq!(displays, vec!["şev", "baş"]);
    assert_eq!(
        q["summary"]["alphabet_policy_excluded_by_review_infrastructure"],
        1
    );
    assert!(q["summary"]["skipped"]
        .get("outside_alphabet_policy")
        .is_none());
    assert!(
        out.with_extension("js").is_file(),
        "the Review Desk file is written"
    );

    // Contradiction: the excluded target also appears in the pool. Refuse, do not re-judge.
    let mut pool = fs::read_to_string(qdir.join("hunspell-only.jsonl")).unwrap();
    pool.push_str(&(queue_record(T_HEVAL, "hérault", "hérault").to_string() + "\n"));
    fs::write(qdir.join("hunspell-only.jsonl"), pool).unwrap();
    let (ok, err, _) = run(root);
    assert!(!ok);
    assert!(
        err.contains("both hunspell-only.jsonl and alphabet-policy-excluded.jsonl"),
        "{}",
        err
    );
}
