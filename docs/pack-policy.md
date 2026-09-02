# Kurmancî Controlled Pack Policy Specification (`pack-policy-v1`)

## Overview

The Kurmancî Controlled Pack Policy mechanism (`data/pack-policy.toml`) specifies explicit inclusion criteria, model profiles, default status, and opt-in settings for compiled binary language packs.

## Pack Definitions

1. **`seed`**:
   - Manually reviewed baseline lexicon only (`data/seed/`).
   - Default pack (`is_default = true`).
   - `model_profile = "none"`.
   - Independent of external source imports or review queues.

2. **`reviewed`**:
   - Manual seed plus explicitly approved imported entries from registered sources:
     - `kurdish-hunspell-kmr`: Hunspell `Approved` and `ApprovedWithMetadataChange` entries.
     - `kuwiki-batch-001`: Kuwiki Wikipedia OOV candidate `Approved` entries (with technical metadata fallbacks `part_of_speech = "unknown"`, `lemma = surface`).
   - Excludes unreviewed, experimental, rejected (`rejected_from_default_pack`), or `needs_linguist` records.
   - `model_profile = "none"`.

3. **`experimental-full`**:
   - Manual seed plus mechanically valid imported entries from registered sources (including `ExperimentalOnly` and `Unreviewed`).
   - Excludes rejected entries and unresolved conflict groups.
   - Opt-in only (`is_experimental = true`, `opt_in = true`).
   - `model_profile = "none"`.

## Multi-Source Provenance & Selection Architecture

Multi-source pack payloads are constructed deterministically across registered review sources:
- All source candidates (`seed`, Hunspell, Kuwiki) are accumulated into `raw_candidates` before passing through a single common `resolve_collisions()` resolver.
- Provenance is recorded in `manifest.json` under `source_provenance: Vec<SourceReviewProvenance>`, deterministically sorted by `source_id`.
- Legacy single-source Hunspell fields (`review_decisions_sha256`, `review_queue_manifest_sha256`, `controlled_review_report_manifest_sha256`) remain preserved for backward compatibility.

## Technical Metadata Fallbacks

For sources lacking POS/morphology metadata (such as `kuwiki-batch-001`), technical pack representation fallbacks are assigned:
- `part_of_speech = "unknown"`
- `lemma = surface` (display token)
- `morphology = []`
- `flags = ""`

These fallbacks represent technical pack serialization requirements and are not human-reviewed morphological classifications.

## Build Verification

Language packs are built under `data/build/packs/<pack_id>/` and contain exactly 5 artifacts:
- `lexicon.bin`
- `manifest.json`
- `collision-report.jsonl`
- `attribution.txt`
- `artifacts.sha256`
