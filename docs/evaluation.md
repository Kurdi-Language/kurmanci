# Kurmancî Evaluation Dataset & Review Specification

## Overview

The evaluation infrastructure defines the benchmark schema, canonical case identity, dataset validation, provenance reporting, deterministic three-pack comparison, and governed human-review transitions for Kurmancî (`ku-Latn`). Benchmark review authority and promotion rules are specified in [Benchmark Review Governance](benchmark-review.md).

```
                              ┌───────────────────────────────┐
                              │  evaluation/spelling/         │
                              │  draft-cases.jsonl            │
                              └───────────────┬───────────────┘
                                              │
                                              ▼
┌───────────────────────────────┐     ┌───────────────┐     ┌───────────────────────────────┐
│  evaluation/spelling/         ├──►  │  Validator    ├──►  │  data/reports/                │
│  reviewed-cases.jsonl         │     │ (Benchmark Schema) │     │  evaluation-provenance/       │
└───────────────────────────────┘     └───────────────┘     └───────────────────────────────┘
```

---

## 1. File & Module Structure

- **Rust Crate Modules**: `data-builder/src/evaluation/`
  - `schema.rs`: Typed schemas (`BenchmarkTask`, `BenchmarkCategory`, `BenchmarkReviewStatus`, `BenchmarkSourceInfo`, `BenchmarkExpectation`, `BenchmarkCaseRecord`).
  - `validator.rs`: Integrity validator, task/category compatibility matrix, contradiction detection, and duplicate detection.
  - `provenance.rs`: Source overlap reporter against `manual-seed`, Hunspell, and corpus partitions.
  - `reports.rs`: JSONL loader and report SHA-256 artifact manifest tools.
  - `transition.rs`: Base-to-candidate snapshot validator for metadata-only human-review promotion.
- **Data Files**:
  - `evaluation/spelling/draft-cases.jsonl`: Unreviewed, AI-assisted, or mechanically generated draft cases (`review_status = "draft"`).
  - `evaluation/spelling/reviewed-cases.jsonl`: Human-reviewed authoritative benchmark dataset (`review_status = "human-reviewed"`).

---

## 2. Benchmark Case Schema

Each benchmark record in JSONL format conforms to `schema_version = "benchmark-case-v1"`:

```json
{
  "schema_version": "benchmark-case-v1",
  "case_id": "465043f4b858ae3a5d5c74aed2e80a35c482b822490f2a69b929fcd4f05e166e",
  "task": "accept-word",
  "category": "exact-preservation",
  "input": "spas",
  "expectation": {
    "accepted": true,
    "preserve_exact": true
  },
  "review_status": "human-reviewed",
  "reviewer_id": "reviewer-001",
  "review_date": "2026-08-03",
  "review_notes": "Exact word preservation test case",
  "source": {
    "kind": "manual"
  }
}
```

---

## 3. Canonical Case Identity

`case_id` is computed using the project's shared canonical u64 big-endian length-prefixed field encoder over SHA-256 (`kurmanci-spelling-case-v1`):

```text
canonical_expectation =
    encode_optional_bool(accepted)
    + encode_optional_bool(preserve_exact)
    + encode_sorted_string_array(expected_candidates)
    + encode_sorted_string_array(forbidden_candidates)
    + encode_optional_bool(allow_no_candidate)
    + encode_optional_usize(required_top_k)

payload =
    encode_string(BENCHMARK_CASE_DOMAIN_TAG)
    + encode_string(task.as_str())
    + encode_string(category.as_str())
    + encode_string(input_nfc)
    + encode_context_in_order(context)
    + canonical_expectation

case_id = hex(sha256(payload))
```

Strings and array elements use checked u64 big-endian length prefixes. Expected and forbidden candidate arrays are sorted before encoding; context order is preserved. Optional values include explicit absent/present markers, and `required_top_k` is encoded as a checked u64 value when present.

Mutable reviewer metadata (`reviewer_id`, `review_date`, `notes`) is excluded from identity calculation so case IDs remain stable as reviews are updated.

Promotion remains metadata-only: `case_id`, task, category, input, context, expectation, and source provenance must not change. If review changes one of those fields, revise and validate the draft before a later promotion.

---

## 4. Validation & Overlap Commands

```bash
# Validate benchmark cases and generate provenance overlap report
cargo run -p kurmanci-data-builder -- validate-eval-cases
```

To validate an explicit base-to-candidate transition:

```bash
cargo run -p kurmanci-data-builder -- validate-eval-transition \
  --base-draft <path> \
  --base-reviewed <path> \
  --candidate-draft <path> \
  --candidate-reviewed <path>
```

The transition validator permits ordinary draft creation, revision, and removal. It protects all existing authoritative reviewed records and requires every new reviewed record to be a metadata-only promotion of a matching base draft.

## 5. Independent Versions

- Benchmark schema: `benchmark-case-v1`.
- Benchmark data: versioned only when an authoritative reviewed dataset is released.
- Engine: versioned independently from evaluation data.
- Comparison policy: `three-pack-comparison-v1`.

Changing one version does not automatically change the others.
