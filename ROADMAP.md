# Kurmancî Language Platform Roadmap

Production-grade offline language infrastructure for Kurmancî (`ku-Latn`).

## Platform Capabilities Roadmap

| Feature / Milestone | Status | Description |
|---|---|---|
| **Deterministic Language-Pack Pipeline** | Completed | Reproducible binary compiler (`data-builder`), zero-copy pack encoding, and SHA-256 manifest verification. |
| **Deterministic Hunspell Importer** | Completed | Parsed preserved Hunspell .dic dataset into provenanced lexicon JSONL and reports (.aff expansion remains planned). |
| **Lexical Data Quality Audit** | Completed | 15-report deterministic quality audit with importer cross-check, Unicode analysis, conflict grouping, suspicious entry detection, and manual-seed comparison. Verdict A — suitable for controlled evaluation only. |
| **Typo & Keyboard Distance Model** | Completed | Weighted Damerau-Levenshtein edit distance with Kurmancî diacritic penalties. |
| **Word & Document Frequencies** | Experimental | Engineering implemented; linguistic validation pending. Frequency pipeline (`import-corpus`, `build-frequencies`) and frequency table generation. Disabled in controlled packs (`model_profile = "none"`). |
| **Frequency-Aware Suggestion Ranking** | Experimental | Engineering implemented; linguistic validation pending. Fixed-point Zipf (`zipf_milli`) in binary pack v2 and `--explain` CLI. Disabled in controlled packs (`model_profile = "none"`). |
| **Deterministic Bigram Language Model** | Experimental | Engineering implemented; linguistic validation pending. Sentence-isolated bigram extraction and binary pack v3 indices. Disabled in controlled packs (`model_profile = "none"`). |
| **Deterministic Trigrams & Backoff Prediction** | Experimental | Engineering implemented; linguistic validation pending. Trigram model, binary pack v4 indices, and CLI hard backoff engine API. Disabled in controlled packs (`model_profile = "none"`). |
| **Corpus Infrastructure & Partitioning (3C1)** | Completed | Format-sensitive registry validation, canonical JSONL document ingestion, atomic sibling-staging import transactions, inventory/audit reports, and leakage-free train/dev/eval partitioning. |
| **Controlled Review Infrastructure (4A.1)** | Completed | Length-prefixed u64 SHA-256 canonical entry/group identity, rule-driven review queue generator (`review-queue-v1`), decision store validator (`review-decision-v1`), and merged audit reporting (`controlled-review-report-v1`). |
| **Controlled Pack Policy & Builds (4A.2)** | Completed | `pack-policy.toml` loader, `seed`, `reviewed`, and `experimental-full` multi-pack builds, model-profile separation (`model_profile = "none"`), pack manifests, and licensing attribution. |
| **Evaluation Schema & Workflow (4B.1)** | Completed | Typed benchmark schema (`benchmark-case-v1`), length-prefixed domain-separated SHA-256 `case_id` generator, contradiction & duplicate validator (`validator.rs`), computed provenance overlap reporting (`data/reports/evaluation-provenance/`), and CLI command `validate-eval-cases`. |
| **Three-Pack Comparison Engine (4B.2)** | Completed | Deterministic comparison engine (`evaluate-packs`) simultaneously evaluates `seed`, `reviewed`, and `experimental-full` packs against authoritative reviewed cases, generating task-specific metrics, case result logs, and pairwise classification reports (`data/reports/pack-comparison/`). |
| **Benchmark Review Governance (4B.3A)** | Completed | Reviewer/date validation, metadata-only promotion semantics, filesystem snapshot transition validation, evidence standards, governance, tests, and PR-scoped benchmark-data protection. Added no benchmark cases. |
| **Initial Human-Reviewed Dataset (4B.3B)** | Completed | Promoted initial 20 human-reviewed benchmark cases following genuine human review, snapshot transition validation, and metadata-only promotion rules. |
| **Pack Quality Assessment & Lexicon Enrichment (4C)** | Active | Initial controlled-pack quality assessment (4C.1), targeted human review of imported lexical entries (4C.2), reviewed-pack rebuild & re-evaluation (4C.3), and explicit default-pack decision (4C.4). |

| **Train-Only Model Binding (3C2)** | Planned | Binding model builders to train partition only, provenance configuration hashes, and evaluation profile pack builds. |
| **Swift SDK** | Planned | Native Apple SDK wrapper around `kurmanci-engine` Rust core. |
| **iOS Reference Keyboard** | Planned | Custom iOS keyboard extension using `ku-Latn` layout and native engine bindings. |
| **Kotlin SDK** | Planned | Native Android/JVM SDK wrapper for Android applications. |
| **Android Reference Keyboard** | Planned | Custom Android IME keyboard layout implementation. |

---

### Contributing & Feedback

Suggestions and technical proposals are welcome via [GitHub Issues](https://github.com/Kurdi-Language/kurmanci/issues).
