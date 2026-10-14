# Kurmancî Evaluation Dataset & Review Specification

## Overview

The evaluation infrastructure defines the benchmark schema, canonical case identity, dataset validation, provenance reporting, deterministic three-pack comparison, and governed human-review transitions for Kurmancî (`ku-Latn`). Benchmark review authority and promotion rules are specified in [Benchmark Review Governance](benchmark-review.md).

```
                              ┌───────────────────────────────┐
                              │  evaluation/spelling/         │
                              │  draft-cases.jsonl            │
                              └───────────────┬───────────────┘
                                              │
                                              ▼
┌───────────────────────────────┐     ┌───────────────┐     ┌───────────────────────────────┐
│  evaluation/spelling/         ├──►  │  Validator    ├──►  │  data/reports/                │
│  reviewed-cases.jsonl         │     │ (Benchmark Schema) │     │  evaluation-provenance/       │
└───────────────────────────────┘     └───────────────┘     └───────────────────────────────┘
```

---

## 1. File & Module Structure

- **Rust Crate Modules**: `data-builder/src/evaluation/`
  - `schema.rs`: Typed schemas (`BenchmarkTask`, `BenchmarkCategory`, `BenchmarkReviewStatus`, `BenchmarkSourceInfo`, `BenchmarkExpectation`, `BenchmarkCaseRecord`).
  - `validator.rs`: Integrity validator, task/category compatibility matrix, contradiction detection, and duplicate detection.
  - `provenance.rs`: Source overlap reporter against `manual-seed`, Hunspell, and corpus partitions.
  - `reports.rs`: JSONL loader and report SHA-256 artifact manifest tools.
  - `transition.rs`: Base-to-candidate snapshot validator for metadata-only human-review promotion.
- **Data Files**:
  - `evaluation/spelling/draft-cases.jsonl`: Unreviewed, AI-assisted, or mechanically generated draft cases (`review_status = "draft"`).
  - `evaluation/spelling/reviewed-cases.jsonl`: Human-reviewed authoritative benchmark dataset (`review_status = "human-reviewed"`).

---

## 2. Benchmark Case Schema

Each benchmark record in JSONL format conforms to `schema_version = "benchmark-case-v1"`:

```json
{
  "schema_version": "benchmark-case-v1",
  "case_id": "465043f4b858ae3a5d5c74aed2e80a35c482b822490f2a69b929fcd4f05e166e",
  "task": "accept-word",
  "category": "exact-preservation",
  "input": "spas",
  "expectation": {
    "accepted": true,
    "preserve_exact": true
  },
  "review_status": "human-reviewed",
  "reviewer_id": "reviewer-001",
  "review_date": "2026-08-03",
  "review_notes": "Exact word preservation test case",
  "source": {
    "kind": "manual"
  }
}
```

---

## 3. Canonical Case Identity

`case_id` is computed using the project's shared canonical u64 big-endian length-prefixed field encoder over SHA-256 (`kurmanci-spelling-case-v1`):

```text
canonical_expectation =
    encode_optional_bool(accepted)
    + encode_optional_bool(preserve_exact)
    + encode_sorted_string_array(expected_candidates)
    + encode_sorted_string_array(forbidden_candidates)
    + encode_optional_bool(allow_no_candidate)
    + encode_optional_usize(required_top_k)

payload =
    encode_string(BENCHMARK_CASE_DOMAIN_TAG)
    + encode_string(task.as_str())
    + encode_string(category.as_str())
    + encode_string(input_nfc)
    + encode_context_in_order(context)
    + canonical_expectation

case_id = hex(sha256(payload))
```

Strings and array elements use checked u64 big-endian length prefixes. Expected and forbidden candidate arrays are sorted before encoding; context order is preserved. Optional values include explicit absent/present markers, and `required_top_k` is encoded as a checked u64 value when present.

Mutable reviewer metadata (`reviewer_id`, `review_date`, `notes`) is excluded from identity calculation so case IDs remain stable as reviews are updated.

Promotion remains metadata-only: `case_id`, task, category, input, context, expectation, and source provenance must not change. If review changes one of those fields, revise and validate the draft before a later promotion.

---

## 4. Validation & Overlap Commands

```bash
# Validate benchmark cases and generate provenance overlap report
cargo run -p kurmanci-data-builder -- validate-eval-cases
```

To validate an explicit base-to-candidate transition:

```bash
cargo run -p kurmanci-data-builder -- validate-eval-transition \
  --base-draft <path> \
  --base-reviewed <path> \
  --candidate-draft <path> \
  --candidate-reviewed <path>
```

The transition validator permits ordinary draft creation, revision, and removal. It protects all existing authoritative reviewed records and requires every new reviewed record to be a metadata-only promotion of a matching base draft.

## 5. Independent Versions

- Benchmark schema: `benchmark-case-v1`.
- Benchmark data: versioned only when an authoritative reviewed dataset is released.
- Engine: versioned independently from evaluation data.
- Comparison policy: `three-pack-comparison-v1`.

Changing one version does not automatically change the others.

## 6. Prediction coverage on held-out contexts

`data-builder evaluate-prediction-coverage --pack <lexicon.bin> [--partition development|evaluation] [--limit 5] [--json] [--out FILE]`
measures how a pack's next-word prediction behaves on real contexts it was not built from,
bound to the pack's provenance: the pack's `manifest.json` must record the pack's own
`binary_sha256` and it must equal the hash of the supplied `lexicon.bin` (refused otherwise, so a
modified or swapped pack never borrows a manifest's provenance); the manifest names the language
model it embeds and the hash of that model's manifest (refused if the model on disk differs); the model's
manifest names its corpus and the hash of the partition manifest its TRAIN data came from
(refused if the local `data/build/corpus-partitions/manifest.json` differs, since the
held-out partition would then not be held out from this model). It walks every sentence of
the chosen partition (`development` by default, or `evaluation`; `train` is refused), requires
every record to carry that partition name and the file to hold exactly the record count that
pinned partition manifest declares for it (a truncated or augmented partition is refused, the
rule the model builder applies to TRAIN), walks only canonical representatives of the model's
corpus exactly as the model builder does, tokenizes through the same `sentence_token_sequences`
helper the model builder uses, and queries the engine's public prediction API at every
position. It reports, for two-word contexts, the positions answered from the trigram table,
by the deterministic bigram backoff, or not at all, and for one-word contexts the positions
answered from the bigram table or not at all, plus how often the word that actually followed
is among the top 1, 3 and 5 candidates. The report records the pack, pack-manifest, model-manifest,
partition-manifest and partition hashes with the model and corpus ids. Numbers and hashes
only: no token, sentence or document text reaches the report
(`data-builder/tests/prediction_coverage_test.rs`).

These rates are the baseline for any later smoothing or backoff refinement: they are taken
before and after the reviewed vocabulary grows, and a prediction change is justified only by
a measured difference, never by inspection of individual outputs.

### Baseline, 2026-09-21: Kuwiki development partition, release 0.1.1 packs

Reports: `docs/evaluation/prediction-coverage/kuwiki-20260801-development-<pack>-20260921.json`
(the command's JSON output, unmodified). Bound provenance recorded in each report: model
`kuwiki-20260801` (manifest `fa5b507b8be6…`), corpus `kuwiki`, partition
manifest `7d85f85e659d…` (the one the model pins; reproduced locally by
`acquire-corpus`, `import-all-corpora`, `partition-corpora`), development partition
`eb5dd0d4b538…` with 9,328 declared records; 9,285 canonical
documents evaluated (43 near-duplicates skipped), 55,201 sentences,
822,225 tokens; limit 5. Reviewed pack `8ad8bbf8134f…`, experimental-full
`6caf3b564465…` (release 0.1.1).

| Pack | Two-word contexts: trigram / backoff / none | Two-word top-1 / top-3 / top-5 | One-word contexts: bigram / none | One-word top-1 / top-3 / top-5 |
|---|---|---|---|---|
| reviewed (2,143 entries) | 24.9% / 30.7% / 44.4% | 12.6% / 17.1% / 19.2% | 42.8% / 57.2% | 4.1% / 7.9% / 9.6% |
| experimental-full (42,248 entries) | 38.0% / 31.7% / 30.3% | 18.5% / 25.3% / 28.3% | 52.7% / 47.3% | 6.3% / 15.5% / 18.5% |

What the numbers say, without deciding anything: with the reviewed vocabulary, 44.4% of
two-word contexts and 57.2% of sentence-initial one-word contexts get no prediction at all,
and the experimental-full pack answers far more of them. Both packs use the same underlying
language model, but the pack compiler embeds only the n-gram records whose words are in the
pack's lexicon: the reviewed pack embeds 19,319 bigrams and 27,929 trigrams, experimental-full
44,820 and 55,140, so the larger experimental vocabulary allows substantially more of that
model's records to be embedded. The coverage difference is therefore consistent with
vocabulary coverage expanding the usable n-gram table; this comparison does not isolate
vocabulary size from embedded n-gram-table size. These are the rates
to compare against after the next reviewed batches; a smoothing or backoff change is justified
only by a measured difference here.

## 7. TRAIN corpus attestation of a review pool

`train-attestation-report` answers one question about a source's ordinary review pool with
counts and hashes only: for each entry no human has decided yet, does its normalized form occur
in the canonical TRAIN document set the committed language model was built from, and in how many
of those documents?

```bash
cargo run -p kurmanci-data-builder -- train-attestation-report \
  --model-id kuwiki-20260801 \
  --out docs/evaluation/attestation/<corpus>-<source>-<date>.json
```

The corpus is external, so this needs it acquired locally (`acquire-corpus kuwiki`); the report
is committed as evidence and is not recomputable on a clean checkout.

### Why the TRAIN partition only

This signal exists to select the membership of an evaluation pack whose completion, correction
and coverage behaviour is then measured on the DEVELOPMENT partition. Attestation computed over
the whole corpus would admit a form *because* it occurs in a held-out document, and the
benchmark would then find that same form: the measurement would report its own selection rule
back as a result. The language model already restricts itself to TRAIN
(`pack/language_model.rs`: "DEV/EVAL partitions never contribute") and this report uses exactly
that population.

That is enforced rather than documented. Every record read must declare `partition == "train"`,
only the corpus's canonical duplicate representatives contribute, and the report then recomputes
`train_document_set_sha256` over the records it visited and refuses to continue unless it equals
the value the language-model manifest records, alongside that manifest's train document count
and the partition, registry and canonical-import hashes it pins. A walk that admitted one
held-out document, or ran against a different partitioning, cannot produce that hash. The
command therefore cannot be pointed at DEVELOPMENT or EVALUATION material at all.

Attestation is evidence, never a review status. A form in a thousand documents is not approved
by that fact and an unattested form is not rejected by it; both stay exactly as unreviewed as
they were. The report also sizes no pack: an evaluation population's entry count is produced by
the builder that assembles it, which is the component that knows the collision rules.

Two different quantities must not be confused. The Review Desk ranks its queue by whole-corpus
occurrence, which is sound because a ranking is measured against nothing. Pack membership uses
canonical TRAIN occurrence, which is what this report records.

### Baseline, 2026-10-14: KurdishHunspell pool against the Kuwiki TRAIN set

`docs/evaluation/attestation/kuwiki-20260801-kurdish-hunspell-kmr-20261014.json`, over the
73,212 canonical TRAIN documents of `kuwiki-20260801` (document set
`19b6366127b866aa…`, verified against the model manifest) and the pool at Hunspell revision
`88131d68…`:

| funnel | entries |
|---|---|
| review pool records | 41,258 |
| less already decided | 1,511 |
| less metadata conflict group members | 2,193 |
| pending | 37,554 |
| pending, TRAIN-attested | 10,473 |
| pending, TRAIN-unattested | 27,081 |

Every pending entry carries a distinct normalized form. Pending entries by TRAIN document count:

| documents | pending entries |
|---|---|
| 0 | 27,081 |
| 1-4 | 6,034 |
| 5-9 | 1,753 |
| 10-24 | 1,546 |
| 25-49 | 857 |
| 50-99 | 283 |
| 100+ | 0 |

Two readings follow, and neither is a decision. First, no pending entry occurs in 100 or more
TRAIN documents, which is consistent with the earlier frequency-ranked review work having
consumed that band, so an evaluation population built from attestation draws entirely from the
1 to 99 document range.
Second, the 1,511 decisions already recorded distribute across those buckets very unevenly, and
the report publishes their outcomes per bucket. Those entries were not selected at random, so
the distribution is observational and supports no inference about the pending population on its
own. It is published to be superseded by a randomised, blinded sample.

## 8. Evaluation-only pack from corpus evidence

`build-evaluation-pack` assembles one population for measurement: the entries a human approved,
plus the still-undecided entries of a source whose normalized form occurs in the canonical TRAIN
document set of the committed language model. It exists to answer whether corpus attestation is
a good enough membership rule to ship vocabulary nobody has reviewed one by one.

```bash
cargo run -p kurmanci-data-builder -- build-evaluation-pack \
  --model-id kuwiki-20260801 --out dist/evaluation-packs/reviewed-plus-train-attested
```

### It is not a pack this project ships

The pack id is absent from [data/pack-policy.toml](../data/pack-policy.toml) and stays absent
while this is an experiment, so it is invisible to `build-pack`, `verify-production-state`,
`rebuild-production` and the release bundle, and no host can resolve it. Four guards enforce
that:

- a pack id already declared in the pack policy is refused;
- an output directory inside, equal to, or containing `data/build/packs/` is refused, with both
  paths resolved through symlinks and normalized by the same rule before comparison;
- an output path containing `..` is refused;
- the model the attestation is computed against must be the model the base pack embeds.

`provenance.json` records `release_kind = "evaluation"` and `production_eligible = false`, so a
copy separated from its directory still says what it is.

### Built through the production path, by removal only

Selection, collision resolution, licensing, the language-model join and the binary compilation
are the production ones, reached by resolving the authoritative `experimental-full` payload and
then removing entries from it. Nothing is selected or compiled by a rule production does not
use, because a population built by different rules could not be compared with the packs it is
measured against: a difference in the result would not be attributable to the vocabulary.

Membership follows the population the authoritative resolver already assigned. Seed and
human-approved entries are kept. Experimental-only entries are dropped, because a human decided
they are not for the reviewed pack and corpus evidence does not overturn a human decision.
Undecided entries are kept only when TRAIN-attested. No review status is read, written or
inferred beyond the population the resolver assigned, and attestation never becomes one.

### First build, 2026-10-14

Against `kuwiki-20260801` and the KurdishHunspell pool:

| | entries |
|---|---|
| base resolved entries (`experimental-full`) | 42,248 |
| kept, human approved | 2,143 |
| kept, undecided and TRAIN-attested | 10,471 |
| dropped, undecided and unattested | 29,500 |
| dropped, experimental-only decisions | 134 |
| final | 12,614 |

The four categories sum to the base payload exactly. Of the 2,143 approved entries, 2 carry a
form that is also TRAIN-attested, so all 10,473 attested forms of the attestation report are
present: 10,471 as undecided entries and 2 represented by an approved entry of the same
normalized form.

### What the comparison can and cannot attribute

The pack embeds 29,499 bigrams and 29,481 trigrams, against 19,319 and 27,929 for `reviewed` and
44,820 and 55,140 for `experimental-full`, because the compiler embeds only n-gram records whose
words are in the lexicon. So a next-word prediction comparison across the three populations
varies vocabulary and embedded table size together and cannot separate them, the same limit
recorded for the prediction-coverage baseline in section 6. Known-word coverage, completion and
correction comparisons do not have that confound.
