# Roadmap

The project's purpose is vendor-neutral Kurmancî (`ku-Latn`) language infrastructure that
Apple, Samsung, Google and other platform vendors can integrate into their own built-in
keyboards: reviewed vocabulary, normalization, spell checking, correction, completion,
diacritic-aware behaviour, deterministic ranking, next-word prediction, compact offline
packs, a stable C ABI with Swift and Kotlin SDKs, and reproducible releases with provenance
and licensing information. It is not a keyboard application. The two consumer test hosts
under `integration/` are reference integration harnesses, not products.

## Delivered

- Engine, C ABI 1.1, pack schema 4, language-model schema 1, fail-closed pack loading with a
  deterministic corruption suite (`docs/PACK_COMPATIBILITY.md`, `docs/integration.md`).
- Swift package over an XCFramework and an Android AAR, both published for release 0.1.1 and
  verified by Rust-free clean-room consumers in CI; 16 KB page-size compliant native library.
- Device evidence on an iPhone 14 Pro, a Samsung `SM-S948B`, a Samsung `SM-A055F` and an
  emulator, with provenance sidecars (`docs/device-benchmark.md`,
  `docs/evaluation/performance-baseline-2026-09-18.md`).
- Deterministic release bundle with `SHA256SUMS` identity and full provenance, published as
  GitHub release `v0.1.1`; a fail-closed publication procedure (`docs/RELEASE_PROVENANCE.md`).
- Vendor summary and a Rust-free evaluation kit at the immutable tag `vendor-kit-0.1.1`
  (`docs/vendor-summary.md`, `docs/vendor-evaluation-kit.md`).
- Human-reviewed orthography and keyboard-requirement contracts with cited references
  (`docs/ku-latn-keyboard.md`); the alphabet and word-punctuation policies
  (`docs/lexicon-review.md`); licensing determinations recorded as data and enforced at load.
- Runtime QA CLI (`kurmanci-cli`), the read-only `inspect-word` inspector,
  `verify-production-state` and `rebuild-production`.

## The bottleneck: reviewed vocabulary

The reviewed pack is small and grows only through human review (`docs/lexicon-review.md`,
`docs/human-review/README.md`). Tooling prepares queues, evidence and reports; it never
approves, rejects or reinterprets a word. After each merged batch the mechanical pipeline is:
validate decisions, rebuild the language model and the packs, run the 357-case diagnostic,
the QA CLI and the promotion report (`scripts/review/promotion-report.sh`).

Open human items: Review Desk batches from the Hunspell reservoir; linguist review of the
held hyphen and apostrophe forms and the canonical apostrophe decision; a second reviewer
identity; the owner's decision whether `reviewed` becomes the default pack.

## Deferred until the data is broader

Ranking changes, prediction smoothing and backoff refinement, morphology from reviewed rules,
further corpora (only with clear redistribution rights), and further memory or latency work.
Each is taken up only on measured evidence after the reviewed vocabulary has grown; the
measurement tools exist so that the before and after can be compared.

## Out of scope

A standalone consumer keyboard, cloud inference, general NLP tooling, operating-system
localization, personalization in the core engine, and mixed-language vocabulary in the
Kurmancî lexicon.
