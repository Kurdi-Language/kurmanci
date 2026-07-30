# Contributing to the Kurmancî Language Platform

Thank you for helping build an open, high-quality language platform for Kurmancî (`ku-Latn`).

## How You Can Contribute

1. **Linguistic Data**: Add verified words, lemma annotations, frequency data, or typo pairs under `data/`.
2. **Core Rust Engine**: Enhance performance, edit distance rules, or n-gram prediction algorithms in `engine/`.
3. **Platform SDKs**: Improve Swift, Kotlin, WASM, or C bindings under `bindings/`.
4. **Reference Applications**: Contribute to iOS, Android, or Web implementations under `apps/`.
5. **Documentation**: Clarify integration guides, linguistic rules, or API references under `docs/`.

## Development Guidelines

- **Code Style**: Rust code must pass `cargo fmt` and `cargo clippy`.
- **Testing**: Every engine change must include corresponding unit tests in `engine/tests/`.
- **License Integrity**: All data contributions must have verifiable source provenance and permissive licensing (CC BY 4.0 or public domain).

## Submitting Pull Requests

1. Fork the repository and create a feature branch.
2. Ensure `cargo test` passes in `engine/`.
3. Submit a pull request with a descriptive title and detailed notes.
