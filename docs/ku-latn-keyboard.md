# Kurmancî (ku-Latn) orthography contract and keyboard requirements

Two small, vendor-neutral data files describe what a platform keyboard must be able to type
for the project's Kurmancî target. They are contracts, not a keyboard application and not a
layout proposal. Both carry `review_status = human-reviewed` (project owner, 2026-09-19): the machine-checkable
parts are verified by `data-builder/tests/ku_latn_contracts_test.rs`; the linguistic choices
belong to Kurmancî speakers and the review process. The contracts introduce no new
linguistic decision: the orthography contract codifies the project's existing human-approved
31-letter alphabet policy, and tooling makes no linguistic decisions.

## `data/keyboard/ku-Latn-orthography.json` (normative, `ku-latn-orthography-v1`)

| Section | Content | Basis |
|---|---|---|
| `locale_tag` | `ku-Latn` (BCP 47: language `ku`, script `Latn`), the tag the language pack and the engine use | project contract |
| `alphabet` | the 31 letters `a b c ç d e ê f g h i î j k l m n o p q r s ş t u û v w x y z` | the project's default-pack alphabet policy (`data-builder/src/alphabet.rs`, `docs/lexicon-review.md`); cited in `references`: Bedir Khan & Lescot, *Grammaire kurde (dialecte kurmandji)*, Paris 1970, Part I §2, p. 3 (alphabet; the source lists 31 core characters and states 33 with two optional ones, which the project does not add) and The Unicode Standard, Version 18.0.0, §3.13 and §5.18 with `UnicodeData.txt` (casing), both selected and approved by the project owner on 2026-09-19 |
| `distinct_letters` | `ç ê î ş û` with base letter, uppercase form, code point and combining-mark decomposition; distinct Kurmancî alphabet letters, not interchangeable representations of `c e i s u` (the decomposition is an encoding property, the letter identity is orthographic) | Unicode; the engine's normalization recomposes decomposed input |
| `casing` | 31 lower/upper pairs, rule `unicode-default`: `i` pairs only with `I`; no other locale's dotted or dotless i rules | Unicode default casing, verified by the test |
| `normalization` | the engine's rule: control characters, U+200B and U+FEFF removed, NFC, lowercase; decomposed input accepted | engine contract (`docs/integration.md`) |
| `not_covered_by_this_contract` | word-internal hyphen and apostrophes (held for linguist review by the word-punctuation policy of 2026-09-19, `docs/lexicon-review.md`); digits, punctuation, symbols (host layers); characters observed in source data (the generated alphabet audit) | scope statement |

## `data/keyboard/ku-Latn-keyboard-requirements.json` (`ku-latn-keyboard-requirements-v1`)

Eight requirements, each with an id and a statement:

1. `letters-typeable`: all 31 letters, lower and upper case, directly or conveniently
   typeable, including `ç ê î ş û`.
2. `access-mechanism-vendor-choice`: how the five letters are reached (dedicated keys, long
   press, an alternate layer, the vendor's own convention) is the vendor's choice. Nothing is
   prescribed.
3. `distinct-letter-identity`: `ç ê î ş û` are distinct alphabet letters, not
   interchangeable representations of `c e i s u`; storage, normalization, comparison, casing
   and lexical identity preserve the distinction. Spell correction and diacritic restoration
   may still propose or apply a base-letter ↔ distinct-letter change when the correction
   engine determines it is the intended spelling (`biji` → `bijî`); that is a linguistic
   correction, never normalization or character equivalence.
4. `casing`: as the orthography contract.
5. `encoding`: precomposed or decomposed output accepted; NFC preferred.
6. `locale-tag`: `ku-Latn` everywhere.
7. `non-lexical-layers`: digits, punctuation and symbols follow the host; nothing outside the
   31 letters is a lexical requirement. This decides nothing about the held word-punctuation
   characters (`-`, U+0027, U+2019), which stay pending linguist review.
8. `word-punctuation-lookup`: when the token at the cursor contains a hyphen or an apostrophe,
   the host queries the engine with the full token first, then with punctuation-aware
   fallbacks (the token split at those characters), and deduplicates the suggestions it gets
   back. The engine answers only for the exact string it is given. Lexical entries containing
   these characters are held for linguist review and are not in the default pack, so a
   full-token hit is not expected until a linguist admits such a form; nothing here admits
   hyphens or apostrophes linguistically.

`layout_arrangement` is `intentionally-unspecified`, an explicit project decision recorded in
the file (project owner, 2026-09-17): the project does not prescribe a universal physical key
arrangement or access mechanism. Shipping platform keyboards differ in both placement and
access while satisfying the same orthographic requirements, so vendors may use their
established native layout conventions provided all 31 letters and their casing are
supported. The evidence is
`docs/evaluation/ku-latn-platform-keyboard-inspection-2026-09-17.md`: Apple (iOS Simulator,
preliminary) and Gboard (emulator, shipping build) give `ç ê î ş û` dedicated keys in two
different arrangements; Samsung Keyboard on a real Galaxy S26 reaches them by long press. The
file lists these as `observed_implementations` with their evidence level; they are
representative evidence, never requirements. Apple, Google/Gboard and Samsung each ship a first-party Kurdish/Kurmancî keyboard
implementation; the project's language infrastructure is intended for vendor and platform
integration and does not propose replacing their keyboard products.

`data/keyboard/layout_ku.json` is the engine's key-adjacency evidence for typo-cost
estimation on a QWERTY-shaped arrangement. It is evidence, not a layout authority, and the
language contract is never made to conform to it. The inspection found that its
neighbourhoods do not correspond to the observed vendor layouts; evaluating adjacency and
edit-cost evidence against real vendor layouts is a separate future engine task that requires
correction and ranking evaluation and is not bundled with the orthography contract.

## What is verified automatically

`ku_latn_contracts_test.rs` fails the build if:

- either file has an unknown or missing field (strict schema), or a draft carries reviewer
  attribution (a reviewed contract must carry it), or the orthography contract lacks its
  alphabet and casing references;
- the alphabet is not exactly the project's 31-letter policy in order, a distinct letter or
  its base is not in it, its uppercase form or code point disagrees with Unicode, or its
  decomposition does not recompose to the letter under the engine's normalization;
- the casing pairs do not list the alphabet exactly or do not round-trip through Unicode
  default casing (the check that keeps `i`/`I` and rules out dotted or dotless variants);
- the keyboard requirements do not satisfy the contract invariants: `letters-typeable.letters`
  must equal the 31-letter policy constant exactly; no other normative requirement may carry
  a letters list; only `access-mechanism-vendor-choice` may carry an `access_mechanism`
  field and its value must be `vendor-choice`; the distinct-letter identity requirement must
  not forbid what the correction engine does (normalization never maps `i` to `î`, while the
  correction API may return a diacritic correction for a human-reviewed benchmark case such
  as `biji` → `bijî`);
- the layout decision is not recorded as settled: `layout_arrangement.status` must be
  `intentionally-unspecified`; the dated project decision must say that no universal
  physical layout or access mechanism is prescribed; its basis evidence document must exist;
  each current Apple/Gboard/Samsung observation record must carry an evidence level and a
  non-empty observed mechanism, today's observed values being dedicated-keys /
  dedicated-keys / long-press (these are the current records, not a closed universe of valid
  vendor mechanisms: an alternate layer or any other vendor convention is equally valid
  under `vendor-choice`); Apple's record must be marked preliminary; the observations must
  be marked as evidence, not requirements; no open question may ask to select, adopt or
  record a reference or universal layout; the adjacency file must be described as evidence,
  not a layout authority. Prose is not scanned for characters or for mechanism wording:
  evidence may quote vendor labels and observed characters, and none of them enters the
  alphabet because the alphabet invariants above are what the tests enforce;
- the adjacency evidence does not key exactly the alphabet, is not symmetric, or does not
  connect each distinct letter with its base;
- a seed or reviewed vocabulary word contains a character outside the alphabet (word-internal
  hyphen and apostrophes excepted, pending their own policy). The experimental vocabulary is
  an evidence reservoir and is only counted in the test output; the generated alphabet audit
  (`data-builder audit-alphabet`) lists its findings with provenance.

## Decisions taken by human review (2026-09-19)

1. Reference orthography: the alphabet inventory cites Bedir Khan & Lescot, *Grammaire kurde
   (dialecte kurmandji)*, Paris 1970, Part I §2, p. 3, recording the source's own
   qualification (31 core characters, 33 if two optional characters are added; the project's
   alphabet stays exactly the 31-letter policy); the casing rule cites The Unicode Standard,
   Version 18.0.0 (§3.13 Default Case Algorithms, §5.18 Case Mappings, `UnicodeData.txt` for
   the simple case mappings), with no Turkish-specific dotted or dotless i tailoring. Both were
   selected and approved by the project owner on 2026-09-19 and are recorded in the orthography
   contract's `references` with `selected_by` and `selected_on`; the test checks their presence.
2. Word-internal hyphen and apostrophes: the word-punctuation policy (`docs/lexicon-review.md`)
   holds such forms for linguist review, keeps them out of the default pack, flags
   punctuation-only variants as possible duplicates for human resolution, and gives hosts the
   lookup guidance in the `word-punctuation-lookup` requirement (full token first, then
   punctuation-aware splits, deduplicated). Which apostrophe code point is canonical stays
   undecided until that review.

Still open for a linguist: the held forms themselves (see the requirements file's open
questions).

Further platform verification (the real iPhone, Samsung's settings list and long-press
details) is welcome as interoperability evidence but is not a prerequisite: no universal
physical arrangement is prescribed and none is to be chosen.

To complete a review, edit the JSON, set `review_status` to `human-reviewed` with
`reviewed_by` and `review_date`, and run
`cargo test -p kurmanci-data-builder --test ku_latn_contracts_test`. The test enforces the
structure; the content is the reviewer's.
