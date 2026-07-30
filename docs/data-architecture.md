# Chapter 2: Data Architecture & Language Resources Specification

## Overview

The Kurmancî Language Platform follows an independent language resource architecture where linguistic datasets are managed, versioned, and built separately from the Rust core engine.

```
data/
├── source/         # Human editable JSONL sources
├── build/          # Compiled binary language packs (.bin)
├── lexicon/        # Canonical lexicon entries
├── frequencies/    # Unigram, bigram, and document frequencies
├── typos/          # Real error & misspelling datasets
├── morphology/     # Suffix rules and affix tables
├── corpus/         # Domain-specific text corpora (news, literature, etc.)
├── ngrams/         # Language models for prediction
├── keyboard/       # Physical key layout adjacency matrices
├── benchmarks/     # Permanent gold benchmark evaluation sets
└── metadata/       # Source provenance and SHA256 checksums
```

## Data Build Pipeline (`kurmanci-builder`)

1. **Source Input**: `data/source/lexicon.jsonl` (Human-editable, version-controlled line-delimited JSON).
2. **Validation & Normalization**: Unicode NFC normalization, lowercase conversion, and schema assertion.
3. **Frequency Ranking**: Sort entries descending by document frequency score.
4. **Binary Packing**: Encodes entries into zero-overhead binary format `data/build/lexicon.bin`.

```
data/source/lexicon.jsonl
         │
         ▼
 kurmanci-builder compile
         │
         ▼
data/build/lexicon.bin
         │
         ▼
Engine::load_binary_pack() (< 1 ms load time)
```

## Keyboard Layout Matrix (`data/keyboard/layout_ku.json`)

Stores physical key adjacencies and substitution costs for QWERTY/QWERTZ layouts (`r ↔ t`, `a ↔ s`, `e ↔ w`, `ş ↔ s`, `ç ↔ c`, `î ↔ i`, `û ↔ u`, `ê ↔ e`).
