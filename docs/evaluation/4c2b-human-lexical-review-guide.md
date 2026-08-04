# 4C.2B Human Lexical Review Guide

Operational procedure for human reviewers conducting genuine linguistic review of imported Kurmancî lexical entries that rank ahead of expected candidates across the four `experimental-full` regressions.

---

## 1. Purpose & Non-Authoritative Status

- **Milestone Stage**: `Milestone 4C.2B (Human Lexical Review Guide - Prepared; Human Review Pending)`
- **Comparison Baseline SHA**: `7db99ee676bf81c8db25df57ee0fbb7dca74b2a0`
- **Canonical Worksheet**: `evaluation/spelling/review-worksheets/4c2a-regression-interfering-entries.csv`
- **Authoritative Status**: **Non-authoritative procedural guide**. This document provides review instructions and schema conversion rules. It does not make any linguistic decisions, approve/reject entries, populate reviewer metadata, or edit `data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl`.

The CSV worksheet remains the single canonical source of truth for mechanical metadata, line numbers, entry IDs, provenance, and ranking positions.

---

## 2. Reviewer Qualifications & Responsibilities

Human reviewers evaluating entries must:
1. Be qualified native or fluent Kurmancî speakers or professional Kurdish linguists.
2. Provide explicit, verifiable citations or documented rationale for every decision.
3. Record their genuine reviewer handle (`reviewer_id`) and exact review date (`review_date` in `YYYY-MM-DD` Gregorian format).
4. Evaluate entry spelling, diacritics, morphology, and standard Hawar orthography.

---

## 3. Evidence Standards

Primary evidence required to establish lexical correctness:
- **Published Dictionaries**: e.g., Dêrsimkî, Zana Farqînî, Kamuran Bedirxan, Ferhenga Kurdî-Tirkî.
- **Grammar & Orthography References**: Published linguistic works and official language academy documentation.
- **Documented Linguistic Judgments**: Explicit, documented rationale by qualified Kurmancî speakers.

> [!CAUTION]
> **Supporting Leads vs Primary Evidence**:
> Wiktionary entries, raw corpus occurrences, Hunspell membership, internet search hit counts, and engine ranking behavior provide helpful leads, but they are **not sufficient by themselves** to establish lexical correctness or approve/reject an entry.

---

## 4. Decision Status Definitions

The project governance uses the following `ReviewDecisionStatus` values:

- **`unreviewed`**:
  Default state before human review. No completed human decision exists. Do not include `reviewer_id`, `review_date`, `review_notes`, `replacement_metadata`, or fabricated evidence. Normally no `decisions.jsonl` record should be created before review.
- **`approved`**:
  The existing imported entry and its metadata are accepted for inclusion in the `reviewed` pack. This does **not** automatically change the repository's `default_pack` policy (which remains `seed`).
- **`approved_with_metadata_change`**:
  Used for ordinary entries only when the entry is accepted with explicit replacement metadata. Requires a complete `replacement_metadata` object (`display`, `normalized`, `flags`, `morphology`, `part_of_speech`).
- **`rejected_from_default_pack`**:
  Excludes the record from the default/reviewed pack based on genuine linguistic evidence. Requires non-empty notes or evidence. Does not imply deletion from the `experimental-full` pack. Must **not** be used solely because an entry ranks ahead of a benchmark target.
- **`experimental_only`**:
  Retains the entry in `experimental-full` data but excludes it from the `reviewed` pack.
- **`needs_linguist`**:
  Formal decision indicating that qualified specialist linguistic review is required. Requires a genuine `reviewer_id` and `review_date` when serialized into `decisions.jsonl`.
- **`needs_source_investigation`**:
  Formal decision indicating that the source record, provenance, or Hunspell flags require further investigation. Requires a genuine `reviewer_id` and `review_date` when serialized into `decisions.jsonl`.

---

## 5. Decision Target Selection & Group Resolution

When converting completed worksheet rows into `decisions.jsonl`:

- **Ordinary Entry (`target_type = "entry"`)**:
  Use when evaluating a single, specific imported entry. Set `target_id` to `entry_id`.
- **Conflict Group (`target_type = "conflict_group"`)**:
  Use when resolving competing metadata records as a group. Set `target_id` to `conflict_group_id`. Group approval requires a mandatory `group_resolution` object:
  - `SelectMember`: `{"type": "select_member", "selected_entry_id": "<entry_id>"}`
  - `ReplaceGroup`: `{"type": "replace_group", "replacement_metadata": {...}}`

> [!IMPORTANT]
> - Conflict-group membership in a worksheet row does **not** force a group decision if only a single entry is being evaluated.
> - Conflict groups cannot use top-level `approved_with_metadata_change` or top-level `replacement_metadata`; replacement must be expressed via `review_status = "approved"` plus `GroupResolution::ReplaceGroup`.

---

## 6. Lexical Validity vs Engine Ranking Policy

> [!NOTE]
> `ranking_policy_followup_needed` is engineering triage metadata outside `decisions.jsonl`.
> It is **not** serialized into `decisions.jsonl` and must not affect the human `review_status`.

A word that a qualified human reviewer determines to be linguistically valid may be `approved` for the `reviewed` pack while separately receiving `ranking_policy_followup_needed = yes` for engine scoring rules (such as prefix-completion cutoffs or edit-distance tie-breaking). A valid word must **never** be rejected solely because it ranks ahead of a benchmark target.

---

## 7. Evidence Array Mapping

In the worksheet CSV, `evidence_or_reference` uses multi-line text (one reference per line). When serializing to `decisions.jsonl`:
- Trim surrounding whitespace from each line.
- Omit empty lines.
- Convert each non-empty line into a separate string element in the `evidence: ["..."]` JSON array.
- Preserve entered order without automatic splitting by commas or semicolons.

---

## 8. Compact 13-Entry Reference Table

Canonical metadata is maintained in `evaluation/spelling/review-worksheets/4c2a-regression-interfering-entries.csv`.

| Benchmark Input | Display | Normalized | Entry ID (`entry_id`) | Line | Conflict Group ID (`conflict_group_id`) |
| :--- | :--- | :--- | :--- | ---: | :--- |
| `welad` | `wela` | `wela` | `e853f7ecb78fd65787d149584b434e5d1bb65a1f4a7f6fa60804ad526da7bb15` | 38435 | *None* |
| `pirt` | `pirt` | `pirt` | `eb511a3f4d243b6077971b9ffaf67f64c95288dd842bcb4e10e2c89d09e2760d` | 29784 | *None* |
| `pirt` | `pirtî` | `pirtî` | `d02a178eaef628bfb131bd961de76b44c7328da2ca75aa2be139e39d3260a06b` | 29814 | *None* |
| `pirt` | `pirtik` | `pirtik` | `896eb1b22a62fb28260b7659f365d261f1fad74ffaf1587aabec2bb45487122a` | 29790 | *None* |
| `şe` | `şe` | `şe` | `36035e8e97f1487bed72acb79aea0789824124327d639d95f20cdd557e2ad5a1` | 43000 | *None* |
| `şe` | `se` | `se` | `98510520313d0c45ffc0efab8e45dc6c72150bda6def9dfe41d11061af2ac875` | 33856 | *None* |
| `şe` | `sê` | `sê` | `09b9615e18bca7b64270de5df9ca439415f4fec996f01f07bf7676ee8655be8e` | 35567 | `ce6da264f7c9c318282532121fbdd7cf300ee1273f7549e555518525d35e838f` |
| `şe` | `şeh` | `şeh` | `1ebd48dd8d38f914ea0ac160f153f2c0295f2d6a3d458039999ceca5e69c95a9` | 43019 | `2ab4bae28edf075f384b1743ade7677d7dccc49afc47ada3a9296a5575466fb9` |
| `şe` | `şen` | `şen` | `d230c02ab33ffcd54709d6a60ce607fa84d72818172867e7e4b699cffbab7f82` | 43155 | *None* |
| `şe` | `şep` | `şep` | `c063f32be8b8421308b6f08c97a5669c9137a520e4ef6ceebfa966a1a04b63cb` | 43169 | *None* |
| `şe` | `şeq` | `şeq` | `9889887800afec8b76732916a99547e8ab99de3b115cf0552e406a88382e77ff` | 43192 | *None* |
| `şe` | `şer` | `şer` | `c4338b296700f1538058f14f7f75e6043a840210c8e1b112ff521799c163d935` | 43210 | *None* |
| `rojb` | `Rojbîn` | `rojbîn` | `2ed2c3105fced70c4337f09b5b85c8d5bc9d971536f18c8e3be5ff9b218b3bdf` | 33119 | *None* |


---

## 9. Structural Conversion Examples

Below are placeholder examples illustrating correct `review-decision-v1` JSON structure.

### Ordinary Entry Decision Example
```json
{
  "schema_version": "review-decision-v1",
  "target_type": "entry",
  "target_id": "<entry-id>",
  "source_id": "kurdish-hunspell-kmr",
  "review_status": "<human-selected-supported-status>",
  "reviewer_id": "<genuine-reviewer-id>",
  "review_date": "<actual-review-date>",
  "review_notes": "<explanatory-linguistic-notes>",
  "evidence": [
    "<human-supplied-reference>"
  ]
}
```

### Conflict Group Resolution Example (Select Member)
```json
{
  "schema_version": "review-decision-v1",
  "target_type": "conflict_group",
  "target_id": "<conflict-group-id>",
  "source_id": "kurdish-hunspell-kmr",
  "review_status": "approved",
  "reviewer_id": "<genuine-reviewer-id>",
  "review_date": "<actual-review-date>",
  "review_notes": "<explanatory-linguistic-notes>",
  "evidence": [
    "<human-supplied-reference>"
  ],
  "group_resolution": {
    "type": "select_member",
    "selected_entry_id": "<selected-entry-id>"
  }
}
```

---

## 10. Validation Commands

After genuine human decisions are populated, run:

```bash
# Validate review decision store formatting and integrity
cargo run -p kurmanci-data-builder -- validate-review-decisions kurdish-hunspell-kmr

# Run worksheet completeness and schema verifiers
python3 scripts/verify_4c2a_regression_worksheet.py
python3 scripts/verify_4c2b_review_guide.py
```
