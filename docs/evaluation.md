# Kurmancî Evaluation Dataset & Review Specification

## Overview

Milestone 4B.1 establishes a human-reviewed benchmark schema, canonical case identity, dataset validator, and provenance overlap reporter for Kurmancî (`ku-Latn`).

```
                              ┌───────────────────────────────┐
                              │  evaluation/spelling/         │
                              │  draft-cases.jsonl            │
                              └───────────────┬───────────────┘
                                              │
                                              ▼
┌───────────────────────────────┐     ┌───────────────┐     ┌───────────────────────────────┐
│  evaluation/spelling/         ├──►  │  Validator    ├──►  │  data/reports/                │
│  reviewed-cases.jsonl         │     │ (4B.1 Schema) │     │  evaluation-provenance/       │
└───────────────────────────────┘     └───────────────┘     └───────────────────────────────┘
```

---

## 1. File & Module Structure

- **Rust Crate Modules**: `data-builder/src/evaluation/`
  - `schema.rs`: Typed schemas (`BenchmarkTask`, `BenchmarkCategory`, `BenchmarkReviewStatus`, `BenchmarkSourceInfo`, `BenchmarkExpectation`, `BenchmarkCaseRecord`).
  - `validator.rs`: Integrity validator, task/category compatibility matrix, contradiction detection, and duplicate detection.
  - `provenance.rs`: Source overlap reporter against `manual-seed`, Hunspell, and corpus partitions.
  - `reports.rs`: JSONL loader and report SHA-256 artifact manifest tools.
- **Data Files**:
  - `evaluation/spelling/draft-cases.jsonl`: Unreviewed, AI-assisted, or mechanically generated draft cases (`review_status = "draft"`).
  - `evaluation/spelling/reviewed-cases.jsonl`: Human-reviewed authoritative benchmark dataset (`review_status = "human-reviewed"`).

---

## 2. Benchmark Case Schema

Each benchmark record in JSONL format conforms to `schema_version = "benchmark-case-v1"`:

```json
{
  "schema_version": "benchmark-case-v1",
  "case_id": "96b6b772c91df01041c2c2fbeee5041ff346bb6ecfc873b22cfc1b7538be28f1",
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

```
payload = encode(BENCHMARK_CASE_DOMAIN_TAG) + encode(task.as_str()) + encode(category.as_str()) + encode(input_nfc) + encode_array(context) + encode_array(sorted(expected_candidates))
case_id = hex(sha256(payload))
```

Mutable reviewer metadata (`reviewer_id`, `review_date`, `notes`) is excluded from identity calculation so case IDs remain stable as reviews are updated.

---

## 4. Validation & Overlap Commands

```bash
# Validate benchmark cases and generate provenance overlap report
cargo run -p kurmanci-data-builder -- validate-eval-cases
```
