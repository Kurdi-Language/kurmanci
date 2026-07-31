# Kurmancî Language Platform Roadmap

Production-grade offline language infrastructure for Kurmancî (`ku-Latn`).

## Platform Capabilities Roadmap

| Feature / Milestone | Status | Description |
|---|---|---|
| **Deterministic Language-Pack Pipeline** | Completed | Reproducible binary compiler (`data-builder`), zero-copy pack encoding, and SHA-256 manifest verification. |
| **Deterministic Hunspell Importer** | Completed | Parsed preserved Hunspell .dic dataset into provenanced lexicon JSONL and reports (.aff expansion remains planned). |
| **Lexical Data Quality Audit** | Completed | 15-report deterministic quality audit with importer cross-check, Unicode analysis, conflict grouping, suspicious entry detection, and manual-seed comparison. Verdict A — suitable for controlled evaluation only. |
| **Typo & Keyboard Distance Model** | Completed | Weighted Damerau-Levenshtein edit distance with Kurmancî diacritic penalties. |
| **Word & Document Frequencies** | Active | Multi-corpus document frequency estimation and Zipfian frequency calibration. |
| **N-Grams & Next-Word Prediction** | Planned | Compact bigram/trigram probabilistic language models for context-aware prediction. |
| **Swift SDK** | Planned | Native Apple SDK wrapper around `kurmanci-engine` Rust core. |
| **iOS Reference Keyboard** | Planned | Custom iOS keyboard extension using `ku-Latn` layout and native engine bindings. |
| **Kotlin SDK** | Planned | Native Android/JVM SDK wrapper for Android applications. |
| **Android Reference Keyboard** | Planned | Custom Android IME keyboard layout implementation. |

---

### Contributing & Feedback

Suggestions and technical proposals are welcome via [GitHub Issues](https://github.com/Kurdi-Language/kurmanci/issues).
