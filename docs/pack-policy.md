# Kurmancî Controlled Pack Policy Specification (`pack-policy-v1`)

## Overview

The Kurmancî Controlled Pack Policy mechanism (`data/pack-policy.toml`) specifies explicit inclusion criteria, model profiles, default status, and opt-in settings for compiled binary language packs.

## Pack Definitions

1. **`seed`**:
   - Manually reviewed baseline lexicon only (`data/seed/`).
   - Default pack (`is_default = true`).
   - `model_profile = "none"`.
   - Independent of external source imports or review queues.

2. **`reviewed`**:
   - Manual seed plus explicitly approved imported entries from registered sources:
     - `kurdish-hunspell-kmr`: Hunspell `Approved` and `ApprovedWithMetadataChange` entries.
     - `kuwiki-batch-*`: Kuwiki Wikipedia OOV candidate `Approved` entries (with technical metadata fallbacks `part_of_speech = "unknown"`, `lemma = surface`).
   - Excludes unreviewed, experimental, rejected (`rejected_from_default_pack`), or `needs_linguist` records.
   - `model_profile = "prediction"`, `language_model = "kuwiki-20260801"`.

3. **`experimental-full`**:
   - Manual seed plus mechanically valid imported entries from registered sources (including `ExperimentalOnly` and `Unreviewed`).
   - Excludes rejected entries and unresolved conflict groups.
   - Opt-in only (`is_experimental = true`, `opt_in = true`).
   - `model_profile = "prediction"`, `language_model = "kuwiki-20260801"`.

## Model Profiles & Committed Language Models

`model_profile` selects which statistical data a pack carries; the lexicon selection is unaffected.

| Profile | Frequency metadata (ranking) | Bigrams / trigrams (next-word prediction) | `language_model` |
| :--- | :---: | :---: | :--- |
| `none` | no | no | forbidden |
| `prediction` | no | yes | required |
| `frequency` | yes | no | required |
| `ngram` | yes | yes | required |

The production packs use `prediction`: next-word prediction gains real corpus statistics while suggestion ranking stays byte-for-byte identical to the lexicon-only behaviour. Enabling unigram frequencies (`frequency` / `ngram`) is deliberately held back: on the 357-case reviewed benchmark, raw Wikipedia frequencies rank inflected forms (`zimanê`, `pirtûka`, `hevalê`, `kurdistanê`) above the lemma the case expects (`ziman`, `pirtûk`, `heval`, `kurdî`), costing 3 Top-1 hits on `reviewed` (22 → 19 / 117) with no improvements. That is a ranking-layer problem (lemma-aware frequency) to solve in the engine before frequency-aware ranking ships; the data is committed and the profiles exist so the experiment is one policy line away.

`language_model` names a directory under `data/language-model/<model_id>/` that is **committed to git** so every machine, including CI, builds byte-identical packs without the corpus. The artifact is deliberately **non-prose**: no corpus sentence, snippet, title, or readable word sequence is stored. It contains:

- `vocabulary.txt`: the union vocabulary of the three authoritative packs, one normalized word per line, sorted; the line index is the word id. Single words only (entries containing whitespace or control characters are excluded), all of them already published in the packs.
- `unigrams.tsv`: `id, token_count, document_count, zipf_milli`.
- `bigrams.tsv` / `trigrams.tsv`: numeric edges `prev_id(s), next_id, count, context_count, probability_millionths`, pruned by `bigram_min_count` / `trigram_min_count`.
- `manifest.json` (`language-model-v1`): corpus id, version and `contributing_corpora` (exactly the model's own corpus), SHA-256 of the corpus dump and extracted documents, of the canonical import, partition and TRAIN partition, of the **corpus-scoped** TRAIN document set, frequency, bigram and trigram tables and their build manifest, the pruning config, the vocabulary fingerprint, per-file hashes, and a licensing snapshot (`licensing.license`, `license_spdx`, `attribution`, `redistribution_determination`).
- `artifacts.sha256`: verified fail-closed by `build-pack` and `validate-pack-manifest`.

A model is regenerated locally with `build-language-model --corpus-id <id>` after the corpus pipeline: `acquire-corpus` → `import-all-corpora` → `partition-corpora`. The builder reads the global TRAIN partition and keeps **only the canonical duplicate representatives that belong to the requested corpus**; other registered corpora never contribute, so the licensing snapshot describes every source of the statistics. **Every statistic comes from the TRAIN partition only**; development and evaluation partitions never contribute, so prediction quality can be evaluated on held-out text. The corpus-scoped intermediates (`train-frequencies.jsonl`, `train-bigrams.jsonl`, `train-trigrams.jsonl`, `build-manifest.json`) are written, git-ignored, under `data/build/language-model/<corpus_id>/` and pinned by hash in the model manifest. The builder refuses inputs whose manifests do not match the files on disk.

Loading a model (`build-pack`, `validate-pack-manifest`, `evaluate-packs`) is fail-closed: every file hash, the content invariants (single-token sorted vocabulary, in-range ids, unique sorted n-gram keys, counts bounded by their context, probabilities equal to the canonical rounding) and the vocabulary fingerprint are verified, and the fingerprint must equal the **current** authoritative union vocabulary. When the pack vocabulary changes (new approved or imported words), the committed model must be regenerated; a stale model never compiles silently. Pack manifests record `language_model_id`, `language_model_manifest_sha256`, and a `language_model_provenance` block that `validate-pack-manifest` requires to be identical to what the committed model yields; a `none` pack must carry none of these.

**Licensing.** A pack that carries a language model records the corpus it derives from (id, version, contributing corpora, license name and SPDX identifier, attribution, dump, document and TRAIN document-set hashes) in its manifest, adds exactly one `language-model:<model_id>` entry to `data_licenses` (its `spdx` is the registry's `license_spdx`, for example `CC-BY-SA-4.0`), and appends a language-model section to `attribution.txt`. The manifest field `redistribution_determination` is copied from the corpus registry's `[corpora.redistribution]` table together with who determined it, when and on what basis, and is `pending-review` when the registry records none; the code makes no determination. For `kuwiki-20260801` it is `allowed` (project owner, 2026-09-19, under the licensing stance recorded in `NOTICE`); model-backed packs carry the corpus attribution and any applicable share-alike obligations as recorded.

Frequency and n-gram data influence ranking and prediction only. They never add, remove, or approve lexical entries; lexical membership remains a human review decision.

## Multi-Source Provenance & Selection Architecture

Multi-source pack payloads are constructed deterministically across registered review sources:
- All source candidates (`seed`, Hunspell, Kuwiki) are accumulated into `raw_candidates` before passing through a single common `resolve_collisions()` resolver.
- Provenance is recorded in `manifest.json` under `source_provenance: Vec<SourceReviewProvenance>`, deterministically sorted by `source_id`.
- Legacy single-source Hunspell fields (`review_decisions_sha256`, `review_queue_manifest_sha256`, `controlled_review_report_manifest_sha256`) remain preserved for backward compatibility.

## Technical Metadata Fallbacks

For sources lacking POS/morphology metadata (such as `kuwiki-batch-001`), technical pack representation fallbacks are assigned:
- `part_of_speech = "unknown"`
- `lemma = surface` (display token)
- `morphology = []`
- `flags = ""`

These fallbacks represent technical pack serialization requirements and are not human-reviewed morphological classifications.

## Build Verification

Language packs are built under `data/build/packs/<pack_id>/` and contain exactly 5 artifacts:
- `lexicon.bin`
- `manifest.json`
- `collision-report.jsonl`
- `attribution.txt`
- `artifacts.sha256`
