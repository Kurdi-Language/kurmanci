# Kurmancî Language Platform

Early-stage offline Kurmancî (`ku-Latn`) language engine and deterministic lexicon compiler written in Rust. Current capabilities include prefix completion, ranked correction candidates, compiled binary language-pack loading, and a CLI demonstration. Prediction models, mobile SDKs, and custom keyboards are planned.

Canonical Repository: [https://github.com/Kurdi-Language/kurmanci](https://github.com/Kurdi-Language/kurmanci)

---

## 🌟 Capabilities & Roadmap

### Current Capabilities
- [x] **Rust Core Engine** (`kurmanci-engine`): Fast Trie prefix autocomplete, weighted Damerau-Levenshtein distance, and diacritic-sensitive candidate ranking (`i ↔ î`, `u ↔ û`, `s ↔ ş`, `c ↔ ç`, `e ↔ ê`).
- [x] **Deterministic Data Compiler** (`kurmanci-data-builder`): Compiles source JSONL records into binary language packs (`lexicon.bin`) with embedded SHA-256 payload validation and a deterministic release manifest.
- [x] **CLI Demonstration** (`kurmanci-cli`): Command-line tool supporting `suggest` query demonstration for completions and corrections.
- [x] **Provenanced Lexical Data**: Handcrafted canonical seed entries (`manual-seed`, Apache-2.0) and preserved KurdishHunspell dictionary (`kurdish-hunspell-kmr`, CC BY-SA 4.0).
- [x] **Deterministic Hunspell Importer** (`kurmanci-data-builder import-hunspell`): Parses preserved `.dic` files into provenanced lexicon JSONLs and reports (`.aff` affix expansion remains planned).

### Planned Features
- [ ] **N-Grams & Next-Word Prediction** *(Planned)*: Probabilistic context-aware prediction models.
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
# Execute deterministic language data compiler
cargo run -p kurmanci-data-builder -- build

# Output binary pack: data/build/lexicon.bin
# Output manifest:    data/build/manifest.json
```

### Command-Line Querying (`cli`)

```bash
# Prefix autocomplete: 'rojb' -> 'rojbaş'
cargo run -p kurmanci-cli -- suggest rojb

# Diacritic restoration: 'biji' -> 'bijî'
cargo run -p kurmanci-cli -- suggest biji

# Typo correction: 'spaz' -> 'spas'
cargo run -p kurmanci-cli -- suggest spaz
```

---

## ⚠️ Data Licensing & Provenance Notice

- **Source Code**: All software logic across `engine/`, `data-builder/`, and `cli/` is licensed under the [Apache License 2.0](LICENSE).
- **Linguistic Datasets**: Datasets in `data/` preserve their upstream open licenses as explicitly registered in [data/source-registry/sources.toml](data/source-registry/sources.toml).
- **Generated Packs**: Binary language packs (`data/build/lexicon.bin`) are build outputs generated reproducibly by `data-builder`.

For contribution guidelines, see [CONTRIBUTING.md](CONTRIBUTING.md).
For security policy, see [SECURITY.md](SECURITY.md).
