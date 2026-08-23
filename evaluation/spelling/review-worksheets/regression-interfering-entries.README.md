# Regression Entry Review Packet

Non-authoritative review worksheet pre-populating factual provenance and ranking metadata for all 13 imported entries ranked ahead of expected candidates across the 4 candidate-ranking regressions observed in the initial controlled-pack quality assessment.

---

## 1. Overview & Scope

- **Document Purpose**: Guidance for human review of lexical regression candidates.
- **Comparison Baseline SHA**: `7db99ee676bf81c8db25df57ee0fbb7dca74b2a0`
- **Assessment Reference**: `docs/evaluation/initial-pack-quality-assessment.md`
- **Worksheet File**: `evaluation/spelling/review-worksheets/regression-interfering-entries.csv`
- **Total Ranking-Interfering Entries**: 13 entries (excluding `şeş` at rank 10, which appears after expected candidate `şev` at rank 9)
- **Authoritative Status**: **Non-authoritative draft packet**. No human linguistic decision, decision store edit (`decisions.jsonl`), benchmark modification, or pack policy change is included in this PR.

---

## 2. Selection Rule

For each observed candidate-ranking regression in `experimental-full` vs `seed`:
- Every imported Hunspell entry ranking ahead of the expected benchmark candidate (`experimental_rank < expected_candidate_rank_experimental`) is included as a ranking-interfering candidate.
- Candidates ranking after the expected benchmark candidate are excluded from the review set.

### Regression Breakdown (13 Ranking-Interfering Entries)

| Benchmark Input | Case ID | Expected Candidate | Baseline Rank (`seed`) | Experimental Rank (`experimental-full`) | Ranking-Interfering Entries Count | Display Forms |
| :--- | :--- | :--- | ---: | ---: | ---: | :--- |
| `pirt` | `b780540d6ffe2a4b22058af62ef48053efb17b1f35846567daffdd34eaf371b7` | `pirtûk` | 1 | 4 | 3 | `pirt`, `pirtî`, `pirtik` |
| `rojb` | `f599d987a78555785b0bab5ae1f92e97a60330472a784f65c064cdd515c5c89b` | `rojbaş` | 1 | 2 | 1 | `Rojbîn` |
| `şe` | `eccacd0ada60909aa409baf71741d75f19843f34e2ec7655d2fe17cb1085c001` | `şev` | 1 | 9 | 8 | `şe`, `se`, `sê`, `şeh`, `şen`, `şep`, `şeq`, `şer` |
| `welad` | `03ed6e50a4cdd49be3294ed911f1a4a92fb09de63e9c4931625b28dfb686531d` | `welat` | 1 | 2 | 1 | `wela` |

---

## 3. Separation of Review Principles

> [!IMPORTANT]
> **Lexical Validity vs Engine Ranking Policy**:
> 1. **Human Lexical Validity Review**: Assess whether each entry is a valid, correctly spelled Kurmancî word based on authoritative linguistic evidence. **A valid word must not be rejected solely because it ranks ahead of a benchmark target.**
> 2. **Engineering Ranking Policy Triage**: If an entry is linguistically valid but distorts suggestion ranking, address the ranking distortion separately through engine scoring rules (e.g. prefix length cutoffs, proper-noun deprioritization, edit-distance tie-breakers).

---

## 4. Worksheet Schema & Column Reference

| Column | Type | Origin | Description |
| :--- | :--- | :--- | :--- |
| `regression_case_id` | 64-char Hex | Benchmark | Full SHA-256 ID of the benchmark case in `reviewed-cases.jsonl`. |
| `benchmark_input` | Text | Benchmark | Input query string triggering the regression. |
| `expected_candidate` | Text | Benchmark | Target surface word expected by the benchmark. |
| `expected_candidate_rank_seed` | Integer | Evaluation | Rank of expected candidate in `seed` pack. |
| `expected_candidate_rank_experimental` | Integer | Evaluation | Rank of expected candidate in `experimental-full` pack. |
| `required_top_k` | Integer | Benchmark | Cutoff threshold required for benchmark satisfaction (default 5). |
| `expected_candidate_satisfied_seed` | Boolean | Evaluation | Whether `seed` pack satisfied `required_top_k`. |
| `expected_candidate_satisfied_experimental` | Boolean | Evaluation | Whether `experimental-full` satisfied `required_top_k`. |
| `interfering_candidate` | Text | Evaluation | Display form of the imported entry ranking ahead of expected candidate. |
| `experimental_rank` | Integer | Evaluation | Rank of the interfering candidate in `experimental-full`. |
| `candidate_is_ahead_of_expected` | Boolean | Evaluation | Strictly `true` for all 13 rows (`rank < expected_rank`). |
| `entry_id` | 64-char Hex | Lexicon | Length-prefixed u64 SHA-256 canonical identity of imported entry. |
| `display` | Text | Lexicon | Surface display word in Hunspell dictionary. |
| `normalized` | Text | Lexicon | NFC lowercase normalized surface form. |
| `source_line_num` | Integer | Source | Physical line number in `kmr/kmr-Latn.dic`. |
| `flags` | Text | Source | Hunspell flags associated with entry. |
| `morphology` | JSON Array | Source | Compact JSON array of morphology tags (e.g. `["po:noun_fem"]`). |
| `conflict_group_id` | 64-char Hex | Queues | SHA-256 group ID if entry belongs to a metadata conflict group. |
| `review_queue_locations` | JSON Array | Queues | Compact JSON array of repository-relative queue paths. |
| `source_revision` | 40-char Hex | Registry | Commit SHA of source repository (`88131d6878ef7fa3ee114aa554adc385ff85b44c`). |
| `source_provenance` | Text | Registry | Exact `source_id:upstream_path:source_line_num` string. |
| `license_id` | Text | Registry | Registered source license identifier (`CC-BY-SA-4.0`). |
| `comparison_policy_version` | Text | Engine | Evaluator policy schema (`three-pack-comparison-v1`). |
| `candidate_limit` | Integer | Engine | Evaluator query candidate limit (10). |
| `comparison_baseline_sha` | 40-char Hex | Assessment | Base commit SHA for baseline evaluation run. |
| `human_lexical_decision` | Text | **Human Input** | *Left empty*. Approved, Rejected, etc. |
| `evidence_or_reference` | Text | **Human Input** | *Left empty*. Dictionary citation, linguistic reference. |
| `reviewer_id` | Text | **Human Input** | *Left empty*. Genuine reviewer handle. |
| `review_date` | Text | **Human Input** | *Left empty*. Review date (`YYYY-MM-DD`). |
| `review_notes` | Text | **Human Input** | *Left empty*. Explanatory linguistic notes. |
| `uncertainty_or_disagreement` | Text | **Human Input** | *Left empty*. `none`, `uncertain`, etc. |
| `ranking_policy_followup_needed` | Text | **Engineering** | *Left empty*. Algorithmic follow-up flag. |

---

## 5. Serialization & Determinism Rules

- **Encoding**: UTF-8 without BOM, NFC text normalization.
- **Line Endings**: LF (`\n`).
- **Quoting**: Standard RFC 4180 minimal quoting.
- **Multivalued Fields**: `morphology` and `review_queue_locations` serialized as compact JSON arrays (no space after commas).
- **Sorting**: Deterministically sorted by `(regression_case_id, experimental_rank, entry_id)`.
- **Paths**: Repository-relative paths only; zero `file:///` URLs or absolute paths.
