# Changelog

All notable changes to the Kurmancî Language Platform will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial Rust core language engine (`kurmanci-engine`) with Trie prefix autocomplete, weighted Damerau-Levenshtein edit distance, and diacritic-sensitive scoring (`i ↔ î`, `u ↔ û`, `s ↔ ş`, `c ↔ ç`, `e ↔ ê`).
- Reproducible binary data-builder compiler (`kurmanci-data-builder`) generating serialized `.bin` language packs and release manifests.
- Command-line interface (`kurmanci` binary) supporting `suggest`, `complete`, `correct`, `contains`, and `benchmark` commands.
- Canonical handcrafted Kurmancî seed lexicon (`data/reviewed/lexicon.jsonl`, 32 entries) and typo dataset (`data/errors/typos.json`).
- Registered, acquired, and preserved first real Kurmancî Hunspell lexical source (`kurdish-hunspell-kmr`, CC BY-SA 4.0, commit `88131d6878ef7fa3ee114aa554adc385ff85b44c`) byte-for-byte in `data/original/kurdish-hunspell-kmr/` with `PROVENANCE.toml` records, license attribution, deterministic acquisition (`data-builder acquire-source`), and automated source integrity verification (`data-builder verify-sources`). (Engine parser integration deferred to Milestone 2B).
- Automated workspace unit test suite, integration tests, binary integrity checks, source registry verification, and CI pipeline.
- Platform architecture documentation, binary pack specification, governance rules, code of conduct, and privacy policies.
