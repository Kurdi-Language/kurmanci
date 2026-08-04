# Initial Controlled-Pack Quality Assessment

Authoritative record of the first three-pack evaluation run on the Kurmancî (`ku-Latn`) language platform, closing Milestone 4B.3B and establishing the baseline for Milestone 4C (*Pack Quality Assessment and Reviewed Lexicon Enrichment*).

---

## Executive Summary

The controlled evaluation pipeline is operational, and the first authoritative benchmark contains 20 human-reviewed cases. The `seed` and `reviewed` packs are currently byte-identical and behaviorally identical because zero external lexical entries have been approved in `reviewed` yet. On this small, seed-oriented benchmark, the `experimental-full` pack produced no measured improvements and four candidate-ranking regressions, while introducing no false acceptance in the single eligible negative case. No authoritative no-candidate behavior was evaluated. The evidence is insufficient to establish production linguistic readiness or to promote a new default pack. `seed` remains the default pack while Milestone 4C expands benchmark coverage and performs targeted human review of imported lexical entries.

---

## 1. Reproducibility & Provenance Metadata

- **Main Commit SHA**: `7db99ee676bf81c8db25df57ee0fbb7dca74b2a0`
- **Benchmark Schema Version**: `benchmark-case-v1`
- **Comparison Policy Version**: `three-pack-comparison-v1`
- **Reviewed Cases SHA-256**: `d4ae5d8d5360014c043cde5bedda2a3fc09d954e58492eb32468956b3993e289`
- **Reviewed Case Count**: 20 cases (100% human-reviewed, 0 draft cases)
- **Candidate Limit Default**: 10
- **Pack Policy SHA-256**: `66f88ad36ccf38959a8a2c5ef1ac0496b9c4094d213914c00fe490d8d6ebc582`
- **Review Decisions SHA-256**: `01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b` (0 approved entries)
- **Review Queue Manifest SHA-256**: `58342bec0f330ffc36cfcc2301632a8242dc9c5308e42aef941cfb62bb4704e5`
- **Controlled Review Report Manifest SHA-256**: `66ee197d4b87f7353ae04a23c4d36b14d2267ffb0739a26086643df79ee2a6e1`

### Benchmark Breakdown

- **Task Breakdown**:
  - `accept-word`: 8
  - `complete-prefix`: 3
  - `correct-word`: 9
- **Category Breakdown**:
  - `correct-spelling`: 4
  - `deletion`: 2
  - `exact-preservation`: 3
  - `false-acceptance`: 1
  - `insertion`: 2
  - `missing-diacritics`: 4
  - `prefix-completion`: 3
  - `substitution`: 1

### Pack Specifications & Hashes

| Property | `seed` | `reviewed` | `experimental-full` |
| :--- | :--- | :--- | :--- |
| **Pack Manifest SHA-256** | `d461649430ec78b7a5fc3aa0260c7ff9f273d5210e889a17e38ca2b684d55aa8` | `8aa4e5e7a071d044dcc9ae479d47265ec7d61f04f68e3426bd15e25a8d38770d` | `dcafe4cacd86551a143c0240407a57825d3914ca3a524301f429eeac13dc68fb` |
| **Binary `lexicon.bin` SHA-256** | `4e186130f1d00893f12d3cb7684945fe55c4414a1b31f910571c84ce5a12a8f1` | `4e186130f1d00893f12d3cb7684945fe55c4414a1b31f910571c84ce5a12a8f1` | `bb1bad60c14cade5d671dabccb3901d3481d8df535dc75995df7d65dd2a06940` |
| **Pack Format Version** | 4 | 4 | 4 |
| **Model Profile** | `none` | `none` | `none` |
| **Total Entries** | 33 | 33 | 41,504 |
| **Binary Size** | 3,109 bytes | 3,109 bytes | 4,866,364 bytes |
| **Frequency / Bigram / Trigram Count** | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| **Data License Set** | Apache-2.0 | Apache-2.0 | Apache-2.0, CC BY-SA 4.0 |

### Generated Report Artifact Hashes

Although generated under `data/reports/pack-comparison/` (which is excluded from Git tracking), the evaluation output is preserved via these exact SHA-256 hashes:

- `summary.json`: `f3b485e6ba0d94a88051ea67cec1ae68313f2219b7e0603c183f2ab03664a6f4` (6,220 bytes)
- `case-results.jsonl`: `afeb41208148b9fb616f1d4764c646f4912d493d56647e4f43b6a9a261d333ae` (16,761 bytes)
- `regressions.jsonl`: `07e463f518b5da13f1dc87621f17bb95aa0d78336c5eb49e6be2abf2e85974c1` (3,599 bytes)
- `improvements.jsonl`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (0 bytes)
- `false-acceptances.jsonl`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (0 bytes)
- `README.md`: `21a42c752e6b092bc9818674f0ce9f5894bb7c6940db8bf8dc52501390e815f3` (1,126 bytes)
- `artifacts.sha256`: `4694b6556bd3ba894500ae5fe98fc7868c6418837b396d9f320a5d9e7ddae60d` (667 bytes)

---

## 2. Controlled Three-Pack Evaluation Metrics

Every metric includes its eligible, excluded, and matched counts alongside decimal values.

| Metric | Eligible cases | Excluded cases | `seed` | `reviewed` | `experimental-full` |
| :--- | ---: | ---: | ---: | ---: | ---: |
| **Known-word coverage** | 7 | 13 | `7/7` (1.0000) | `7/7` (1.0000) | `7/7` (1.0000) |
| **False-acceptance rate** | 1 | 19 | `0/1` (0.0000) | `0/1` (0.0000) | `0/1` (0.0000) |
| **Correction Top-1** | 9 | 11 | `9/9` (1.0000) | `9/9` (1.0000) | `8/9` (0.8889) |
| **Correction Top-3** | 9 | 11 | `9/9` (1.0000) | `9/9` (1.0000) | `9/9` (1.0000) |
| **Correction Top-5** | 9 | 11 | `9/9` (1.0000) | `9/9` (1.0000) | `9/9` (1.0000) |
| **MRR** | 9 | 11 | `1.0000` | `1.0000` | `0.9444` |
| **Completion recall** | 3 | 17 | `3/3` (1.0000) | `3/3` (1.0000) | `3/3` (1.0000) |
| **Exact preservation** | 3 | 17 | `3/3` (1.0000) | `3/3` (1.0000) | `3/3` (1.0000) |
| **No-candidate** | 0 | 20 | Unavailable | Unavailable | Unavailable |

> [!NOTE]
> **No-Candidate Metric Details**:
> - `eligible_count`: 0
> - `matched_count`: 0
> - `excluded_count`: 20
> - `value`: Unavailable (not evaluated)
> 
> The current 20-case benchmark contains zero cases with an `allow_no_candidate = true` expectation. Reporting `0.0` would incorrectly imply that no-candidate behavior was evaluated and failed in every case; the evaluator correctly produces `None` (unavailable).

---

## 3. Pairwise Classifications & Case Analysis

- **`reviewed_vs_seed`**: 0 improvements, 0 regressions, 20 unchanged.
  - `seed` and `reviewed` packs are currently byte-identical and behaviorally identical (`binary_sha256: 4e186130f1d0...`).
- **`experimental_vs_seed`**: 0 improvements, 4 regressions, 16 unchanged.
- **False Acceptances**: 0 across all pack comparisons.

### Detailed Regression Breakdown (`experimental-full` vs `seed`)

All 4 regressions in `experimental-full` were caused by unreviewed Hunspell entries interfering with candidate ranking.

| Input | Task / Category | Case ID | Baseline (`seed`) Suggestions & Rank | Experimental Suggestions & Rank | Required Cutoff | Classification Reason |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `pirt` | `complete-prefix`<br>`prefix-completion` | `b780540d6ffe2a4b22058af62ef48053efb17b1f35846567daffdd34eaf371b7` | `['pirtûk']`<br>**Rank 1** | `['pirt', 'pirtî', 'pirtik', 'pirtûk', 'pirtikî', ...]`<br>**Rank 4** | `required_top_k = 5`<br>*(Satisfied: Yes)* | **Regression**: Unreviewed entries (`pirt`, `pirtî`, `pirtik`) match prefix and push expected `pirtûk` from rank 1 down to rank 4. |
| `rojb` | `complete-prefix`<br>`prefix-completion` | `f599d987a78555785b0bab5ae1f92e97a60330472a784f65c064cdd515c5c89b` | `['rojbaş', 'roj', ...]`<br>**Rank 1** | `['Rojbîn', 'rojbaş', 'rojber', 'rojbûn', ...]`<br>**Rank 2** | `required_top_k = 5`<br>*(Satisfied: Yes)* | **Regression**: Unreviewed proper noun `Rojbîn` matches prefix and ranks ahead of `rojbaş`, dropping expected rank from 1 to 2. |
| `şe` | `complete-prefix`<br>`prefix-completion` | `eccacd0ada60909aa409baf71741d75f19843f34e2ec7655d2fe17cb1085c001` | `['şev', 'şevbaş', ...]`<br>**Rank 1** | `['şe', 'se', 'sê', 'şeh', 'şen', 'şep', 'şeq', 'şer', 'şev', ...]`<br>**Rank 9** | `required_top_k = 5`<br>*(Satisfied: **No**)* | **Regression**: Short 2-3 char unreviewed entries (`şe`, `se`, `sê`, etc.) push expected `şev` down to rank 9, violating `required_top_k = 5`. |
| `welad` | `correct-word`<br>`substitution` | `03ed6e50a4cdd49be3294ed911f1a4a92fb09de63e9c4931625b28dfb686531d` | `['welat']`<br>**Rank 1** | `['wela', 'welat', 'welqa', 'Belar', ...]`<br>**Rank 2** | `required_top_k = 5`<br>*(Satisfied: Yes)* | **Regression**: 1-deletion edit `wela` ranks ahead of 1-substitution edit `welat` lexically, dropping expected rank from 1 to 2. |

---

## 4. Licensing & Attribution Consequences

Distributing or embedding compiled binary language packs carries distinct licensing obligations:

1. **`seed` Pack**:
   - Source: Handcrafted manual seed (`data/reviewed/lexicon.jsonl`).
   - License: **Apache-2.0**.
   - Consequence: Free commercial and open-source embedding with standard copyright notice.
2. **`reviewed` Pack**:
   - Source: Handcrafted manual seed plus explicitly approved external entries.
   - License: **Apache-2.0** (currently contains the same final entry set as `seed`).
   - Consequence: Retains Apache-2.0 status until external entries requiring attribution are approved into decisions.
3. **`experimental-full` Pack**:
   - Source: Handcrafted seed plus mechanically imported KurdishHunspell dictionary (`kurdish-hunspell-kmr`).
   - License: **CC BY-SA 4.0** (ShareAlike attribution obligations derived from KurdishHunspell source).
   - Consequence: Cannot be treated as plain Apache-2.0. Any distribution or integration of `experimental-full` requires preserving CC BY-SA 4.0 attributions and ShareAlike terms.

---

## 5. Benchmark Limitations

The 20-case benchmark establishes workflow validity, but has major structural limitations:

1. **Small Sample Size**: Only 20 total cases exist.
2. **Seed Vocabulary Bias**: Seven positive acceptance cases all draw directly from the small seed vocabulary.
3. **Sparse Coverage**:
   - Only 1 false-acceptance case (`pirtûkk`).
   - Only 3 completion cases (`pirt`, `rojb`, `şe`).
   - Zero no-candidate cases.
   - Zero regional, morphology, proper-name, or contextual prediction cases.
4. **Asymmetric Measurement**: The benchmark cannot yet measure the primary potential benefit of `experimental-full`—valid vocabulary coverage for everyday Kurmancî words outside the 33-entry seed.
5. **Ranking Sensitivity**: Zero improvements does not prove the imported Hunspell dictionary is without value; four regressions demonstrate that adding the unreviewed 41,000-entry lexicon can degrade candidate ranking on the current benchmark.


---

## 6. Milestone 4C Structure & Default-Pack Decision

### Milestone 4C: Pack Quality Assessment & Reviewed Lexicon Enrichment

- **4C.1 Initial Pack Quality Assessment** (*Completed by this document*): Established baseline comparison across `seed`, `reviewed`, and `experimental-full`.
- **4C.2 Benchmark-Driven Lexical Review** (*Next Active Stage*): For each observed benchmark gap or regression, identify the exact imported entries affecting the result. Human-review those entries for lexical validity and provenance, and separately determine whether any remaining ranking problem requires an engine-policy change. Do not reject a linguistically valid entry solely because it ranks ahead of the benchmark target.
- **4C.3 Reviewed-Pack Rebuild & Re-evaluation**: Rebuild `reviewed` pack after targeted human decisions, rerun authoritative evaluation, and verify zero regressions against `seed`.
- **4C.4 Explicit Default-Pack Decision**: Submit a dedicated policy PR presenting benchmark evidence before considering any change to `default_pack`.

### Default Pack Recommendation

`default_pack = seed` **remains the default policy**.

No production linguistic readiness claim is made. `seed` provides a clean, zero-regression baseline while Milestone 4C performs targeted human review of imported lexical entries.
