# Data-Builder Subsystem & Pipeline Guide

## Overview

`data-builder` is the dedicated compiler for language resources in the Kurmancî platform. It reads version-controlled source files, normalizes Unicode, validates schema constraints, merges duplicate records, and generates deterministic production binary packs (`lexicon.bin`) and release manifests (`manifest.json`).

## Command Usage

```bash
# Execute full reproducible build pipeline
cargo run -p data-builder -- build

# Specify custom configuration
cargo run -p data-builder -- build --config data-builder/config/builder.toml
```

## Build Pipeline Steps

1. **Source Loading**: Reads `data/reviewed/lexicon.jsonl`.
2. **Unicode Normalization**: Canonical NFC normalization preserving Kurmancî diacritics (`ç`, `ê`, `î`, `ş`, `û`).
3. **Schema Validation**: Validates non-empty fields, max word length, allowed character set, and script consistency.
4. **Deduplication & Merging**: Merges entries sharing identical normalized keys while preserving all source references (`sources`).
5. **Binary Pack Encoding**: Writes compact zero-parse binary file `data/build/lexicon.bin`.
6. **Manifest & Report**: Generates `data/build/manifest.json` and `data/reports/build-report.json` containing SHA-256 hash checksums.

## Display Form Casing Rules

- Common words are stored with **lowercase** display forms (`"word": "rojbaş"`). Capitalized forms in source lexicons are normalized to lowercase unless the record represents a proper noun.
- Engine/UI presentation layers handle sentence-initial capitalization dynamically based on context.

## Importer Test Matrix Requirements

Every new data importer added to `data-builder` must include fixture-based unit/integration tests covering the following test matrix:

1. **Valid Source**: Parses valid source format accurately into `SourceLexiconEntry` items.
2. **Malformed Input**: Rejects invalid syntax, missing fields, or invalid UTF-8 cleanly.
3. **Duplicate Entries**: Handles duplicate entries within the same source file deterministically.
4. **Conflicting Metadata**: Correctly merges or resolves conflicting POS, frequency, or regional metadata.
5. **Unicode Edge Cases**: Handles zero-width spaces, combined diacritics, and Kurdish special characters (`ç`, `ê`, `î`, `ş`, `û`, `Ê`, `Î`, `Ş`).
6. **Missing Source Registration**: Verifies that un-registered source strings trigger strict validation errors when configured.
