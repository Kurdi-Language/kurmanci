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
   - Manual seed plus explicitly approved imported entries (`Approved` and `ApprovedWithMetadataChange`).
   - Excludes unreviewed, experimental, rejected, or unresolved records.
   - `model_profile = "none"`.

3. **`experimental-full`**:
   - Manual seed plus mechanically valid imported entries (including `ExperimentalOnly` and `Unreviewed`).
   - Excludes rejected entries and unresolved conflict groups.
   - Opt-in only (`is_experimental = true`, `opt_in = true`).
   - `model_profile = "none"`.

## Build Verification

Language packs are built under `data/build/packs/<pack_id>/` and contain exactly 5 artifacts:
- `lexicon.bin`
- `manifest.json`
- `collision-report.jsonl`
- `attribution.txt`
- `artifacts.sha256`
