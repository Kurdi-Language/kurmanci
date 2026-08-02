# Human Review Decisions — kurdish-hunspell-kmr

This directory contains human-authored review decisions for `kurdish-hunspell-kmr`.

- **Source of Truth**: `decisions.jsonl` is the sole source of truth for human decisions.
- **Schema Version**: `review-decision-v1`.

> **IMPORTANT**: Never edit generated review queues in `data/review-queues/`. Only edit `decisions.jsonl`. The queue generator is allowed to replace `review-queues/`. It is never allowed to rewrite `review-decisions/`. If a queue appears incorrect, fix the importer or audit—not the generated queue.

## Example Decision Record

```json
{
  "schema_version": "review-decision-v1",
  "target_type": "entry",
  "target_id": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "source_id": "kurdish-hunspell-kmr",
  "review_status": "approved",
  "reviewer_id": "maintainer-1",
  "review_date": "2026-08-02",
  "review_notes": "Verified against classical dictionary",
  "evidence": ["dictionary-reference-p42"]
}
```
