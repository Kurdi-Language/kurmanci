# Kurmancî Language Platform Roadmap

Production-grade offline language infrastructure for Kurmancî (`ku-Latn`).

## Platform Capabilities Roadmap

| Feature / Milestone | Status | Description |
|---|---|---|
| **Deterministic Language-Pack Pipeline** | Completed | Reproducible binary compiler (`data-builder`), zero-copy pack encoding, and SHA-256 manifest verification. |
| **Deterministic Hunspell Importer** | Completed | Parsed preserved Hunspell .dic dataset into provenanced lexicon JSONL and reports (.aff expansion remains planned). |
| **Lexical Data Quality Audit** | Completed | 15-report deterministic quality audit with importer cross-check, Unicode analysis, conflict grouping, suspicious entry detection, and manual-seed comparison. Verdict A — suitable for controlled evaluation only. |
| **Typo & Keyboard Distance Model** | Completed | Weighted Damerau-Levenshtein edit distance with Kurmancî diacritic penalties. |
| **Word & Document Frequencies** | Completed | Deterministic corpus frequency pipeline (`import-corpus`, `build-frequencies`), NFC lowercase tokenizer, JSONL frequency table, and statistical reports suite. |
| **Frequency-Aware Suggestion Ranking** | Completed | Fixed-point Zipf (`zipf_milli`) in binary pack v2, secondary frequency tie-breaking, reviewed evaluation suite (`cases.jsonl`), CLI `--explain`, and `evaluate-ranking` benchmark tool. |
| **Deterministic Bigram Language Model** | Completed | Sentence boundary isolation, original-context checked integer probability (`probability_millionths`), pruning, binary pack v3 with lexicon indices, Engine `predict_next` API, CLI `predict-next`, reviewed benchmark (`cases.jsonl`), and 2-pass determinism verification. |
| **Deterministic Trigrams & Backoff Prediction** | Active | Trigram language model extension, sentence-isolated triple extraction, checked fixed-point probabilities, binary pack v4 with lexicon indices, hard backoff engine API (`predict_next_with_context`), CLI two-word context support, and source selection evaluation benchmark. |
| **Corpus & Benchmark Expansion** | Planned | Expanding corpus datasets, scaling evaluation benchmarks, and improving next-word prediction quality as larger corpus data becomes available. |
| **Trigrams, Backoff & Context Interpolation** | Planned | Trigram language model extension, Stupid Backoff / Kneser-Ney interpolation, and context-aware candidate reranking. |
| **Swift SDK** | Planned | Native Apple SDK wrapper around `kurmanci-engine` Rust core. |
| **iOS Reference Keyboard** | Planned | Custom iOS keyboard extension using `ku-Latn` layout and native engine bindings. |
| **Kotlin SDK** | Planned | Native Android/JVM SDK wrapper for Android applications. |
| **Android Reference Keyboard** | Planned | Custom Android IME keyboard layout implementation. |

---

### Contributing & Feedback

Suggestions and technical proposals are welcome via [GitHub Issues](https://github.com/Kurdi-Language/kurmanci/issues).
