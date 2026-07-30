# Architecture Guide - Kurmancî Core Engine

## Overview

The Kurmancî Core Engine is a pure Rust library that provides deterministic, high-performance spelling correction, autocomplete, diacritic restoration, and n-gram prediction.

```
                    ┌─────────────────────────┐
                    │     Platform App UI     │
                    │ (iOS, Android, WASM, CLI)│
                    └────────────┬────────────┘
                                 │  C FFI / Foreign Bindings
                    ┌────────────▼────────────┐
                    │      Public Engine API  │
                    └────────────┬────────────┘
                                 │
     ┌───────────────────────────┼───────────────────────────┐
     │                           │                           │
┌────▼──────┐             ┌──────▼─────┐             ┌───────▼──────┐
│ Trie Index│             │  Distance  │             │ Multi-Factor │
│ (Prefix)  │             │ (Weighted) │             │    Ranker    │
└───────────┘             └────────────┘             └──────────────┘
```

## Suggestion Pipeline

1. **Input Normalization**:
   - Convert to NFC Unicode normalization form.
   - Convert to lowercase.
2. **Exact & Prefix Match**:
   - Trie traversal for sub-millisecond lookup.
3. **Diacritic Restoration**:
   - Fast fallback scan for stripped diacritic forms (`i ↔ î`, `u ↔ û`, `s ↔ ş`, `c ↔ ç`, `e ↔ ê`).
4. **Weighted Damerau-Levenshtein Edit Distance**:
   - Custom edit distance calculation with weighted penalties for Kurmancî diacritics (penalty 0.25 vs standard 1.0).
5. **Multi-Factor Ranking**:
   - Score calculation combining log frequency metric, edit distance penalty, prefix match bonus, and suggestion type weight.

## Performance Guarantees

- **Latency**: < 20 ms per query (typically < 1 ms for seed corpus).
- **Memory Footprint**: < 50 MB target RAM usage.
- **Dependencies**: 0 external runtime network dependencies; fully offline.
