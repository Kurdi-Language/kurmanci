# Kurmancî

High-performance offline language infrastructure for Northern Kurdish / Kurmancî (`ku-Latn`). It provides spell checking, autocomplete, typo correction, diacritic restoration, and context prediction in a lightweight Rust core with native bindings for Apple (Swift/XCFramework) and Android (Kotlin/JNI).

Canonical Repository: [https://github.com/Kurdi-Language/kurmanci](https://github.com/Kurdi-Language/kurmanci)

---

## What It Provides

- **Spell Checking**: Fast Trie lookup and NFC-normalized exact match verification.
- **Autocomplete**: Prefix completion over compiled lexicon entries.
- **Typo Correction**: Weighted Damerau-Levenshtein edit distance tuned for Kurmancî.
- **Diacritic Restoration**: Diacritic-aware candidate scoring (`i ↔ î`, `u ↔ û`, `s ↔ ş`, `c ↔ ç`, `e ↔ ê`).
- **Frequency/Context Ranking**: Zipf-scaled frequency metadata and n-gram scoring.
- **Next-Word Prediction**: Statistical bigram and trigram backoff prediction.
- **Offline Deterministic Packs**: Zero network dependency compiled `.bin` language packs.
- **Stable Core APIs**: High-level thread-safe Rust API (`kurmanci-engine`) and panic-safe C ABI (`kurmanci-ffi`).
- **Apple SDK**: Idiomatic SwiftPM package and precompiled XCFramework.
- **Android SDK**: Idiomatic Kotlin wrapper, JNI bridge, and Maven Central distribution.

---

## Quick Start

### Prerequisites
- Rust 1.85.0+ (managed via [rust-toolchain.toml](rust-toolchain.toml))

### Building & Testing
```bash
# Run all workspace unit and integration tests
cargo test --workspace

# Check formatting and Clippy linting
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Compiling Language Packs (`data-builder`)
```bash
# Build controlled language packs
cargo run -p kurmanci-data-builder -- build-pack seed
cargo run -p kurmanci-data-builder -- build-pack reviewed
cargo run -p kurmanci-data-builder -- build-pack experimental-full
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

## Language Packs & Data Model

The platform compiles source JSONL records into deterministic, zero-copy binary language packs (`lexicon.bin`, format v4) verified by embedded SHA-256 manifests.

- **`seed` / `reviewed` (Default)**: Contains handcrafted seed entries and external entries that have passed human lexical review. Guaranteed zero regression baseline for production distribution.
- **`experimental-full`**: Contains unreviewed imported dictionary sources (e.g. 41,000+ entries from KurdishHunspell). Used in controlled evaluation to identify gaps and triage candidates.
- **Deterministic Generation**: Executing `build-pack` produces 100% byte-identical output across platforms.
- **Human Lexical Review**: Unreviewed imported entries are held in staging until reviewed for spelling validity, morphology, and provenance.

For binary format details, see [`docs/BINARY_PACK_SPEC.md`](docs/BINARY_PACK_SPEC.md).

---

## Core Engine (Rust)

Add `kurmanci-engine` to your `Cargo.toml`:

```rust
use kurmanci_engine::{KurmanciEngine, CorrectionOptions, SuggestOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load binary pack from disk
    let engine = KurmanciEngine::from_pack_file("data/build/packs/seed/lexicon.bin")?;

    // Is 'welat' a known word?
    if engine.is_known_word("welat") {
        println!("Known word!");
    }

    // Combined suggest query (typo correction + diacritics + autocomplete)
    let suggestions = engine.suggest("spaz", SuggestOptions { limit: 5 });
    for sug in suggestions {
        println!("Candidate: {} (kind: {:?})", sug.text, sug.kind);
    }

    Ok(())
}
```

For API documentation, see [`docs/architecture.md`](docs/architecture.md).

---

## Apple SDK

### Swift Package Manager
Add the remote package dependency to your `Package.swift` or Xcode project:
```swift
.package(url: "https://github.com/Kurdi-Language/kurmanci-swift", from: "0.1.0")
```

### Swift Usage Example
```swift
import Kurmanci

let engine = try KurmanciEngine(packURL: packURL)

let isKnown = try engine.isKnownWord("welat")
let suggestions = try engine.suggest("spaz", limit: 5)
for sug in suggestions {
    print("\(sug.text) (editCost: \(sug.editCost))")
}
```

For Apple integration details, see [`swift/README.md`](swift/README.md).

---

## Android SDK

### Maven Central Dependency
Add `mavenCentral()` to your repositories and add the dependency:
```kotlin
dependencies {
    implementation("io.github.ferhatguneri:kurmanci-android:0.1.0")
}
```

### Kotlin Usage Example
```kotlin
import org.kurmanci.KurmanciEngine

KurmanciEngine.open(packBytes).use { engine ->
    val isKnown = engine.isKnownWord("welat")
    val suggestions = engine.suggest("spaz", limit = 5)
    for (sug in suggestions) {
        println("${sug.text} (${sug.kind})")
    }
}
```

For Android integration details, see [`android/README.md`](android/README.md).

---

## Language Data & Provenance

Language data is managed under strict provenance and licensing rules in [`data/source-registry/sources.toml`](data/source-registry/sources.toml).

- **Seed Lexicon**: Handcrafted entries (`manual-seed`, Apache-2.0).
- **Imported Lexicons**: Upstream dictionaries (e.g. `kurdish-hunspell-kmr`, CC BY-SA 4.0) acquired reproducibly with SHA-256 verification.
- **Controlled Review Policy**: Imported candidates are **not** automatically promoted to default packs. Every candidate undergoes automated queue generation (`review-queue-v1`) and human review (`review-decision-v1`) before entry into `reviewed` language packs.

For review procedures, see [`docs/lexicon-review.md`](docs/lexicon-review.md).

---

## Evaluation & Benchmarks

The platform includes an automated three-pack comparison engine (`evaluate-packs`) and benchmark validation framework.

- **Benchmark Dataset**: Authoritative human-reviewed cases (`evaluation/spelling/reviewed-cases.jsonl`) testing spelling correction, diacritic restoration, and completion tasks.
- **Three-Pack Comparison**: Simultaneously evaluates `seed`, `reviewed`, and `experimental-full` packs to measure Top-1/3/5 accuracy, Mean Reciprocal Rank (MRR), false acceptance rates, and candidate ranking regressions.
- **Benchmark Governance**: Rules for benchmark promotion and transition safety documented in [`docs/benchmark-review.md`](docs/benchmark-review.md).

For evaluation documentation, see [`docs/evaluation.md`](docs/evaluation.md).

---

## Repository Structure

```
kurmanci/
├── .github/                  # CI workflows and release automation
├── android/                  # Android SDK (Kotlin/JNI) & integration tests
├── cli/                      # Command-line interface tool (kurmanci-cli)
├── data/                     # Source registry, seed entries & review stores
├── data-builder/             # Data compiler and binary generator crate
├── docs/                     # Specifications, architecture & evaluation guides
├── engine/                   # Core Rust language engine & test suite
├── ffi/                      # Stable C ABI bindings (kurmanci-ffi)
├── integration/              # Xcode and Android app test hosts
├── scripts/                  # XCFramework, AAR, and verification scripts
└── swift/                    # Swift Package SDK & Clang module wrapper
```

---

## Contributing

Contributions are welcome! Key high-value contribution areas include:
- **Linguistic Review**: Reviewing candidate queues in `data/review-queues/` to approve entries for default packs.
- **Corpus & Lexical Sources**: Registering new open-licensed Kurmancî text corpora and word lists.
- **Benchmark Expansion**: Adding realistic human-reviewed test cases to `evaluation/spelling/`.
- **Platform Integrations**: Building native IME keyboard extensions for iOS and Android.
- **Engine Optimization**: Improving search Trie performance and memory efficiency.

Please review [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`GOVERNANCE.md`](GOVERNANCE.md) before submitting pull requests.

---

## Roadmap

Platform priorities, planned features, and integration roadmap are documented in [`ROADMAP.md`](ROADMAP.md).

---

## License & Attribution

- **Software Logic**: All code across `engine/`, `data-builder/`, `ffi/`, `swift/`, `android/`, `cli/`, and `scripts/` is licensed under the [Apache License 2.0](LICENSE).
- **Language Data**: Datasets in `data/` retain their upstream open licenses (e.g. CC BY-SA 4.0, Apache-2.0) as declared in [`data/source-registry/sources.toml`](data/source-registry/sources.toml).
