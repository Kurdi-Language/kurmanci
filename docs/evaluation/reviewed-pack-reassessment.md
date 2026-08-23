# Reviewed-Pack Rebuild and Re-evaluation Report

> **Assessment status**: reproduced on this branch.

This report documents the rebuild and re-evaluation of the `reviewed` language pack incorporating human lexical review decisions.

---

## 1. Provenance & Artifact Hashes

- **Base SHA**: `665066e9c396015b799725239feabf311002f619`
- **Source Revision (`kurdish-hunspell-kmr`)**: `88131d6878ef7fa3ee114aa554adc385ff85b44c`
- **Review Decisions SHA-256 (`data/review-decisions/kurdish-hunspell-kmr/decisions.jsonl`)**: `8d379b788811d868d39da29cdfdc97c39ba6e018fe8f2be7627bac985b33810c`
- **Reviewed Benchmark Cases SHA-256 (`evaluation/spelling/reviewed-cases.jsonl`)**: `943be89cae1db87bf5dcefcbbdd5cb7b5bfbeffcf4f647900b1ee2c39fa42fc1`
- **Pack Policy SHA-256 (`pack-policy-v1`)**: `66f88ad36ccf38959a8a2c5ef1ac0496b9c4094d213914c00fe490d8d6ebc582`

---

## 2. Decision & Pack Selection Summary

- **13 Authoritative Decisions Evaluated**:
  - **3 Approved Decisions**:
    - **2 Selected into `reviewed`**: `şeq` (valid noun 'slap/crack/split'), `şer` (valid noun 'war/fight')
    - **1 Approved but Unselected**: `sê` (approved as entry, but unselected because it belongs to an unresolved metadata-conflict group)
  - **10 Non-Approved Decisions, All Excluded**:
    - **1 `experimental_only`**: `Rojbîn` (retained in `experimental-full` pending proper-name pack policy)
    - **3 `needs_linguist`**: `pirt`, `pirtî`, `şen`
    - **6 `needs_source_investigation`**: `wela`, `pirtik`, `se`, `şeh`, `şep`, `şe`
  - **11 Total Exclusions** among the 13 reviewed external records.

### External Reviewed Additions
```
external_reviewed_additions = reviewed_entry_set - seed_entry_set = {şeq, şer}
```

---

## 3. Pack Metrics & Binary Integrity

| Pack ID | Description | Default Pack | Unique Entries | Binary Size | Binary SHA-256 |
| :--- | :--- | :---: | :---: | :---: | :--- |
| `seed` | Manually reviewed seed lexicon | `true` | 33 | 3,109 B | `4e186130f1d00893f12d3cb7684945fe55c4414a1b31f910571c84ce5a12a8f1` |
| `reviewed` | Manual seed plus explicitly approved entries | `false` | 35 | 3,312 B | `350aeae92bd40dc5a232d33f5b1b11b97834613ccf43fa3b0c16bb53afd1581e` |
| `experimental-full` | Manual seed plus mechanically valid imported entries | `false` | 41,496 | 4,865,539 B | `fb38f3864f93b981fb78a3b66016a1cf3f46ab508711babf8ff8ae05d4ff4ba7` |

---

## 4. Three-Pack Evaluation & Pairwise Comparisons

The 20 human-reviewed benchmark cases were evaluated across all three packs:

- **`reviewed_vs_seed`**: 0 improvements, **1 regression**, 19 unchanged
- **`experimental_vs_seed`**: 0 improvements, **2 regressions**, 18 unchanged

### Case-Level Regression Analysis

#### `reviewed_vs_seed` Regression (`eccacd0ada609...`)
- **Task**: `complete-prefix`
- **Input**: `"şe"`
- **Expected Candidate**: `"şev"`
- **Seed Pack Suggestions**: `["şev", "şevbaş", ...]` (Rank 1 for `şev`)
- **Reviewed Pack Suggestions**: `["şeq", "şer", "şev", "şevbaş", ...]` (Rank 3 for `şev`)

**Findings**:
Adding valid Kurmancî words `şeq` and `şer` to `reviewed` caused expected candidate `şev` to drop from rank 1 to rank 3 under prefix completion for `"şe"`. This demonstrates that **lexical approval and candidate ranking quality are independent**. Adding valid words can introduce tie-breaking interference until ranking-policy and frequency-weighting work is performed.

#### `experimental_vs_seed` Regressions
1. **Input `"rojb"`** -> `experimental-full` suggests proper name `Rojbîn` ahead of `rojbaş` (Rank 2 vs Rank 1 in seed).
2. **Input `"şe"`** -> `experimental-full` suggests `sê`, `şeh`, `şeq`, `şer` ahead of `şev` (Rank 5 vs Rank 1 in seed).

---

## 5. Open Governance Question: `sê` Conflict-Group Non-Selection

The approved entry-level decision for `sê` does not currently cause the entry to enter `reviewed`, because pack construction processes its unresolved metadata-conflict group (`ce6da264...`) separately.

This assessment records the observed behavior as current resolver logic but does not decide whether:
1. entry-level approval should select that exact member;
2. conflict-group `select_member` resolution is always required; or
3. decision and pack-builder semantics need clarification in future iterations.

---

## 6. Licensing Consequences & Attribution

The inclusion of `şeq` and `şer` in `reviewed` introduces `CC-BY-SA-4.0` attribution requirements for `kurdish-hunspell-kmr` alongside `Apache-2.0` for `manual-seed`. Both licenses and attribution sources are accurately reflected in `data/build/packs/reviewed/attribution.txt` and `manifest.json`.

---

## 7. Recommendation & Conclusion

- **Default Pack**: Maintain `default_pack = seed`.
- **Rationale**: Only 2 external entries entered `reviewed`, resulting in 1 benchmark ranking regression and 0 benchmark coverage improvements on the current seed-focused benchmark set. Broader non-seed benchmark expansion and ranking-policy refinement are necessary before considering `reviewed` as a default pack candidate.
