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
- Automated workspace unit test suite, integration tests, binary integrity checks, and CI pipeline.
- Platform architecture documentation, binary pack specification, governance rules, code of conduct, and privacy policies.
