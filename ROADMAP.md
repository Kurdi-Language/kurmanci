# Kurmancî Language Platform Roadmap

Production-grade offline language infrastructure for Kurmancî (`ku-Latn`).

## Capabilities Overview

The platform currently provides:
- **Rust Core Engine** (`kurmanci-engine`): Fast Trie autocomplete, weighted edit distance, diacritic-sensitive candidate ranking, and n-gram context prediction.
- **Data Compiler & Pipeline** (`kurmanci-data-builder`): Reproducible binary language pack compilation (`lexicon.bin`, format v4), corpus ingestion, and deterministic partitioning.
- **Lexicon & Evaluation Infrastructure**: Rule-driven review queue generation, human decision validation, multi-pack policy staging, and tri-pack evaluation comparison engine.
- **Public Native Interfaces**: High-level thread-safe Rust API and panic-safe C99/C++11 ABI (`kurmanci-ffi`).
- **Apple Integration**: SwiftPM package distribution and precompiled Apple XCFramework (`KurmanciFFI.xcframework`).
- **Android Integration**: Idiomatic Kotlin SDK (`org.kurmanci.KurmanciEngine`), JNI native libraries, cross-compiled ABIs, AAR packaging, and Maven Central release (`io.github.ferhatguneri:kurmanci-android`).

---

## Forward-Looking Product Priorities

### Language Data
- Substantially expand the reviewed core vocabulary.
- Integrate additional properly licensed lexical sources.
- Improve corpus coverage and source/provenance evidence.
- Improve handling of variants where supported by reviewed data.

### Evaluation & Benchmarks
- Grow the human-reviewed benchmark toward 200–500 useful cases.
- Expand typo, diacritic, completion, preservation, ranking, morphology, and keyboard-error coverage.
- Use benchmark failures to drive ranking and correction improvements.

### Core Engine
- Correction, autocomplete, and ranking improvements driven by evaluation.
- Morphology and diacritic handling improvements.
- Performance and memory footprint optimization.

### Public APIs
- Preserve stable Rust API and C ABI contracts.
- Evolve APIs only where concrete integration requirements justify it.

### Apple Integration
- Maintain SwiftPM and XCFramework distribution.
- Develop reference iOS keyboard integration.

### Android Integration
- Maintain Kotlin SDK, JNI bindings, and Maven Central distribution.
- Develop reference Android keyboard/IME integration.

### Platform Integrations
- System spell-check and text-service integration.

### Performance
- Real-device latency and memory measurement across supported mobile ABIs.
- Production hardening and load efficiency.

### Future Platform Integrations
- WebAssembly client bindings where appropriate.

---

## Contributing & Feedback

Suggestions and technical proposals are welcome via [GitHub Issues](https://github.com/Kurdi-Language/kurmanci/issues).
