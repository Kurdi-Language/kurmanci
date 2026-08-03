# Kurmancî Language Platform

Early-stage offline Kurmancî (`ku-Latn`) language engine and deterministic lexicon compiler written in Rust. Current capabilities include prefix completion, ranked correction candidates, bigram and trigram context prediction models, compiled binary language-pack loading, and a CLI demonstration. Mobile SDKs and custom keyboards are planned.

Canonical Repository: [https://github.com/Kurdi-Language/kurmanci](https://github.com/Kurdi-Language/kurmanci)

---

## 🌟 Capabilities & Roadmap

### Current Capabilities
- [x] **Rust Core Engine** (`kurmanci-engine`): Fast Trie prefix autocomplete, weighted Damerau-Levenshtein distance, and diacritic-sensitive candidate ranking (`i ↔ î`, `u ↔ û`, `s ↔ ş`, `c ↔ ç`, `e ↔ ê`).
- [x] **Deterministic Data Compiler** (`kurmanci-data-builder`): Compiles source JSONL records into binary language packs (`lexicon.bin`, pack format v4) with embedded SHA-256 payload validation and a deterministic release manifest.
- [x] **CLI Demonstration** (`kurmanci-cli`): Command-line tool supporting `suggest` query demonstration and `predict-next` context prediction.
- [x] **Provenanced Lexical Data**: Handcrafted canonical seed entries (`manual-seed`, Apache-2.0) and preserved KurdishHunspell dictionary (`kurdish-hunspell-kmr`, CC BY-SA 4.0).
- [x] **Corpus Infrastructure & Partitioning** *(Milestone 3C1)*: Format-sensitive registry validation, canonical JSONL document ingestion, atomic staging import transactions, inventory/audit reports, and leakage-free train/dev/eval partitioning.
- [x] **Controlled Review Infrastructure** *(Milestone 4A.1)*: Length-prefixed u64 SHA-256 canonical entry/group identity, rule-driven review queue generator (`review-queue-v1`), decision validator (`review-decision-v1`), and merged audit reporting (`controlled-review-report-v1`).
- [x] **Controlled Pack Policy & Builds** *(Milestone 4A.2)*: Multi-pack policy configuration (`seed`, `reviewed`, `experimental-full`), explicit conflict resolution, atomic pack staging, and licensing attribution (`model_profile = "none"`).
- [x] **Evaluation Schema & Workflow** *(Milestone 4B.1)*: Typed benchmark schema (`benchmark-case-v1`), canonical case ID generator, contradiction & duplicate validator (`validator.rs`), computed provenance overlap reporting (`data/reports/evaluation-provenance/`), and CLI command `validate-eval-cases`.
- [x] **Three-Pack Comparison Engine** *(Milestone 4B.2 - Completed)*: Deterministic comparison engine (`evaluate-packs`) simultaneously evaluating `seed`, `reviewed`, and `experimental-full` packs against authoritative reviewed cases, generating task-specific metrics, case result logs, and pairwise classification reports (`data/reports/pack-comparison/`).
- [ ] **Benchmark Review Governance** *(Milestone 4B.3A - Active)*: Reviewer/date validation, metadata-only promotion rules, snapshot transition validation, evidence standards, and CI protection. This milestone adds no benchmark cases.
- [ ] **Initial Human-Reviewed Dataset** *(Milestone 4B.3B - Planned)*: A separate data-review milestone for a small authoritative dataset reviewed by real Kurmancî speakers.

### Experimental Capabilities
> [!NOTE]
> **Statistical & Language Models Status**: Engineering implemented; linguistic validation pending.
> Frequency tables, bigrams, and trigrams are experimental. They are disabled in current production packs (`model_profile = "none"`).

### Planned Features
- [ ] **Mobile SDKs & Keyboards** *(Planned)*: Swift SDK (iOS), Kotlin SDK (Android), and custom keyboard extensions.
- [ ] **WebAssembly Bindings** *(Planned)*: Browser and WebAssembly client library target.

---

## 📁 Monorepo Overview

```
kurmanci/
├── .github/                  # CI workflows, PR and issue templates
├── cli/                      # Command-line interface tool (kurmanci-cli)
├── data/                     # Source registry and canonical seed entries
│   ├── reviewed/             # Manually reviewed canonical JSONL entries
│   └── source-registry/      # Single authoritative registry (sources.toml)
├── data-builder/             # Data compiler and binary generator crate
├── docs/                     # Format and architecture specifications
├── engine/                   # Core language engine library & test suite
├── .editorconfig
├── .gitignore
├── CHANGELOG.md              # Version history
├── CODE_OF_CONDUCT.md        # Contributor Covenant v2.1 code of conduct
├── CONTRIBUTING.md           # Development workflow & provenance guidelines
├── Cargo.lock                # Root Cargo workspace lockfile
├── Cargo.toml                # Root workspace Cargo manifest
├── GOVERNANCE.md             # Maintainer structure & decision rules
├── LICENSE                   # Apache License 2.0
├── NOTICE                    # Copyright & attribution notice
├── README.md                 # Project overview and quickstart
├── ROADMAP.md                # Development roadmap & feature milestones
├── SECURITY.md               # Offline security guarantees
└── rust-toolchain.toml       # Pinned Rust toolchain (1.85.0)
```

---

## ⚡ Quick Start

### Prerequisites

- Rust 1.85.0+ (managed via [rust-toolchain.toml](rust-toolchain.toml))

### Building & Testing

```bash
# Run all workspace unit and integration tests
cargo test --workspace

# Execute formatting check
cargo fmt --all --check

# Execute Clippy linter
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Compiling Language Packs (`data-builder`)

```bash
# Build controlled language packs (Milestone 4A.2)
cargo run -p kurmanci-data-builder -- build-pack seed
cargo run -p kurmanci-data-builder -- build-pack reviewed
cargo run -p kurmanci-data-builder -- build-pack experimental-full

# Validate pack manifests and invariants
cargo run -p kurmanci-data-builder -- validate-pack-manifest
```

### Evaluation Governance and Validation

```bash
# Validate the current draft and reviewed benchmark files
cargo run -p kurmanci-data-builder -- validate-eval-cases

# Evaluate controlled packs against authoritative reviewed cases
cargo run -p kurmanci-data-builder -- evaluate-packs

# Show the explicit snapshot-transition validator interface
cargo run -p kurmanci-data-builder -- validate-eval-transition --help
```

Benchmark promotion policy, evidence standards, and reviewer responsibilities are documented in [`docs/benchmark-review.md`](docs/benchmark-review.md).

### Command-Line Querying (`cli`)

```bash
# Prefix autocomplete: 'rojb' -> 'rojbaş'
cargo run -p kurmanci-cli -- suggest rojb

# Diacritic restoration: 'biji' -> 'bijî'
cargo run -p kurmanci-cli -- suggest biji

# Typo correction: 'spaz' -> 'spas'
cargo run -p kurmanci-cli -- suggest spaz

# Diagnostic ranking explanation
cargo run -p kurmanci-cli -- suggest spaz --explain
```

---

## 🎯 Frequency-Aware Suggestion Ranking

Milestone 2E integrates frequency metadata into the binary pack (`PACK_VERSION = 2`) and candidate suggestion ranking.

### 1. Frequency-to-Lexicon Join
During `cargo run -p kurmanci-data-builder -- build`, frequency records from `data/build/frequencies.jsonl` are joined to canonical lexicon entries by exact normalized form. Zipf values are stored as fixed-point integers (`zipf_milli`, e.g. `4.823` -> `4823`). Missing entries default to zero values.

### 2. Candidate Ranking Policy
Frequency is used strictly as a **secondary** signal after exact match and edit distance:
- **Exact Matches**: `SuggestionKind::Exact` candidates are strictly prioritized above non-exact candidates.
- **Spelling Corrections**: 1) Kind priority (`Exact` -> `DiacriticCorrection` -> `Completion` -> `Correction`), 2) Edit cost (ascending), 3) Exact diacritic match, 4) Zipf frequency (`zipf_milli`), 5) Document count, 6) Lexical tie-breaker.
- **Prefix Completions**: 1) Kind priority, 2) Prefix match quality, 3) Zipf frequency, 4) Document count, 5) Completion length, 6) Lexical tie-breaker.

### 3. Ranking Evaluation & Benchmarking
To evaluate candidate ranking accuracy on reviewed spelling cases:
```bash
cargo run -p kurmanci-data-builder -- evaluate-ranking
```
Compares baseline (`use_frequency: false`) against experiment (`use_frequency: true`) and writes evaluation reports to `data/reports/ranking-evaluation/`.
*Note*: The evaluation suite validates pipeline integration, exact-word preservation, and determinism. Statistical effectiveness will scale as larger text corpora are registered.

---

## 📊 Corpus Frequency Pipeline

The platform includes a deterministic corpus frequency pipeline (`kurmanci-data-builder`) for importing text corpora and building word and document frequency metadata.

### 1. Corpus Registry & Format
Corpora are registered in [`data/source-registry/corpora.toml`](data/source-registry/corpora.toml). Every corpus entry requires:
- `corpus_id`, `language` (`ku-Latn`), `license`, `url`, `sha256`, `description`, `attribution`.
- Preserved text files in `data/original/<corpus-id>/` validated against SHA-256 checksums.

### 2. Importing a Corpus
To import a registered corpus:
```bash
cargo run -p kurmanci-data-builder -- import-corpus opensubtitles-kmr
```
This verifies registration, validates SHA-256 checksums, copies preserved text files into `data/imported/<corpus-id>/`, and writes `data/reports/corpora/<corpus-id>/import-summary.json`.

### 3. Tokenizer Rules
The tokenizer operates deterministically:
1. **Unicode Normalization**: Canonical Composition (NFC).
2. **Case Normalization**: Lowercase, preserving full Kurmancî diacritics (`ç`, `ê`, `î`, `ş`, `û`).
3. **Word Boundaries**: Splits on whitespace and Unicode Punctuation (`P*`) and Symbols (`S*`). Apostrophes and hyphens split clitics and compound forms (e.g. `l'amour` -> `l`, `amour`).
4. **Filtering**: Discards pure numbers, letterless strings, and empty tokens.

### 4. Frequency Builder & Output
To generate word and document frequencies across all imported corpora:
```bash
cargo run -p kurmanci-data-builder -- build-frequencies
```
- **Document Boundary**: In text corpus files, each non-empty line represents a single document boundary (line-delimited document format). `document_count` tracks how many distinct lines contain a given token.
- **Output Record**: `data/build/frequencies.jsonl` containing `word`, `token_count`, `document_count`, `normalized_frequency`, and `zipf` (`log10(count_per_billion)`).
- **Deterministic Sort**: Primary sort by `token_count` descending, secondary sort by `word` ascending.
- **Statistical Reports**: Generated under `data/reports/frequencies/` (`summary.json`, `top-100.json`, `length-distribution.json`, `character-analysis.json`, `coverage.json`, `README.md`, `artifacts.sha256`).

### 5. Determinism & Provenance Guarantees
- **Strict Provenance Filtering**: `build-frequencies` loads `corpora.toml` and processes *only* registered corpora and explicitly declared corpus files verified against SHA-256 checksums. Unregistered directories or undeclared stale files under `data/imported/` are strictly ignored.
- **Staged Generation & Rollback**: `import-corpus` and `build-frequencies` use atomic staged directory replacement with backup-and-rollback safety.
- **Artifact Manifest**: `data/reports/frequencies/artifacts.sha256` covers `data/build/frequencies.jsonl` as well as all statistical report files. Executing `build-frequencies` consecutively produces 100% byte-identical output verified in CI.

---

## ⚠️ Data Licensing & Provenance Notice

- **Source Code**: All software logic across `engine/`, `data-builder/`, and `cli/` is licensed under the [Apache License 2.0](LICENSE).
- **Linguistic Datasets**: Datasets in `data/` preserve their upstream open licenses as explicitly registered in [data/source-registry/sources.toml](data/source-registry/sources.toml).
- **Generated Packs**: Binary language packs (`data/build/lexicon.bin`) are build outputs generated reproducibly by `data-builder`.

For contribution guidelines, see [CONTRIBUTING.md](CONTRIBUTING.md).
For security policy, see [SECURITY.md](SECURITY.md).
