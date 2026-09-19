# Human review pack

Decisions that only the project owner (or a linguist or lawyer the owner names) can make, with the facts each one needed and the answers given. Tooling prepared the facts; nothing here is a legal or linguistic conclusion.

**Status (2026-09-19): all twelve answers were given on 2026-09-19.** Decision 1 (licensing, 1a to 1d) was applied by PR #73. Decisions 2 (word-punctuation policy, 2a to 2f) and 3 (orthography references, 3a and 3b) are applied by PR #75. The "answer sheet" at the end is the record of the decisions; each decision section keeps, clearly labelled, the historical facts the decision was made on (the state of `main` at `5ed81d9`, after PR #71) and states the current state separately.

Hands-on items are listed near the end with their completion state.

## Decision 1: licensing and redistribution determinations

**Current state (applied by PR #73, merged as 2a7a92a on 2026-09-19):** the language model `kuwiki-20260801` records `redistribution_determination = allowed` with the owner's statement as its basis (registry table `[corpora.redistribution]`, copied into the model manifest and re-checked against the registry on every load); the four source determinations are confirmed; the OpenSubtitles corpus is no longer registered: its files, registry entry and active code and test dependencies were removed, with no replacement corpus in the production registry (the corpus pipeline tests and CI use a test-only fixture on an isolated root). A release bundle built from a clean tree is a production release.

**Historical inventory: the state used to make the decision (main at `5ed81d9`, before PR #73).** Values in this table are as they were then, not as they are now; in particular the language model's determination was `pending-review` and `corpus:opensubtitles-kmr` was still registered.

| Subject | What it was | Licence recorded then | Attribution recorded then | Determination recorded then | Where it was used then |
|---|---|---|---|---|---|
| `language-model:kuwiki-20260801` | bigram and trigram tables derived from the Kurmancî Wikipedia dump of 2026-08-01 (vocabulary 42,247 forms; no article text) | CC BY-SA 4.0 | Wikipedia contributors, Wîkîpediya (Kurmancî edition), CC BY-SA 4.0 | pending-review (historical; now `allowed`) | reviewed and experimental-full packs (next-word prediction); release bundle |
| `source:manual-seed` | the 33 hand-written seed entries (`data/reviewed/lexicon.jsonl`) | Apache-2.0 | none required | allowed | seed, reviewed, experimental-full |
| `source:kurdish-hunspell-kmr` | KurdishHunspell Kurmancî dictionary and affix files, pinned to commit `88131d68…` (https://github.com/sinaahmadi/KurdishHunspell) | CC BY-SA 4.0 (upstream LICENSE preserved under `data/original/`) | required | allowed | reviewed (approved entries only) and experimental-full |
| `source:kuwiki-batch-001` | 1,000 reviewed word candidates from the Wikipedia corpus (words and counts only) | CC BY-SA 4.0 | required | allowed | reviewed and experimental-full |
| `source:kuwiki-batch-002` | second batch, same kind | CC BY-SA 4.0 | required | allowed | reviewed and experimental-full |
| `corpus:kuwiki` | the Wikipedia dump (43 MB) and the extracted prose (63 MB), never committed, acquired on demand | CC BY-SA 4.0 | as above | none recorded then (the corpus is not shipped) | builds the language model and the review batches |
| `corpus:opensubtitles-kmr` (historical; deleted by PR #73) | a line-delimited corpus that was committed under `data/original/opensubtitles-kmr/` | recorded as CC BY-SA 4.0 | OpenSubtitles project contributors and Kurdi-Language maintainers | none recorded | registered for frequency tables; in no shipped artifact |

The questions the determination had to answer for the language model, as put to the owner:

1. Are bigram and trigram counts derived from CC BY-SA text a derivative work that must itself be CC BY-SA, or data that can be shipped under the project's terms with attribution? The repository takes no position; the release tooling copies the recorded answer.
2. If CC BY-SA applies, is that acceptable for the target vendors, who ship the pack inside proprietary software?
3. Does the answer differ between the reviewed pack (model cut to its vocabulary) and experimental-full?

The options that were offered for the language-model line were `allowed`, `not-allowed` and `pending-review`; the owner chose `allowed` with the statement quoted in the answer sheet. The separate question on `corpus:opensubtitles-kmr` (keep, re-verify, or remove) was answered "delete completely".

Watch-list for future sources, so nothing is imported by accident: NLP Kurdî (reference only unless rights are clear), KurdishLex (CC BY-NC-SA, commercially problematic), OPUS/OpenSubtitles (needs review), Tatoeba (verify current terms), OSCAR/Common Crawl (legal review).

## Decision 2: word-internal hyphen and apostrophe policy

**Current state (applied by PR #75):** a lexical form containing a hyphen or an apostrophe (U+0027 or U+2019) is not approved into the reviewed/default pack until a linguist has reviewed it; both apostrophe code points are held alike because the canonical one is undecided; forms that differ only by such punctuation are flagged as possible duplicates and are never merged nor both admitted. The 145 Hunspell forms (74 with U+0027, 71 with a hyphen, none with U+2019 in the current source) are held in `punctuation-policy-needs-linguist.jsonl` and are out of the ordinary review pool. `'azîm` was approved on 2026-08-24 and was moved to `needs_linguist` on 2026-09-19, its former approval preserved in the note and evidence of its decision record; it is no longer in the reviewed pack. Canonical review identity stays the exact normalized form (no punctuation is folded); the stripped form is only a duplicate flag. Hosts are told to query the full token first, then punctuation-aware splits, and to deduplicate. Policy text: `docs/lexicon-review.md`, "Word-Punctuation Policy".

**Historical evidence: the state used to make the decision (main at `5ed81d9`).** Before this decision the alphabet policy (#67) deliberately left hyphens and apostrophes undecided, so such forms passed through review like any other, and one had been approved into the reviewed pack: `'azîm` (Hunspell entry, approved 2026-08-24). The Hunspell review pool then held 41,403 forms; the two Wikipedia batches and the seed lexicon held no such form.

| Character | Forms in the pool then | Pattern | Examples (dictionary forms, not corpus text) | Decisions taken before this one |
|---|---|---|---|---|
| apostrophe U+0027 | 74 | 70 word-initial (`'a…`, `'e…`, `'i…`, `'î…`), 4 word-internal, 1 bare `'` | `'abd`, `'adil`, `'aqilane`, `'aqildar`, `'edl`, `'ecêb`, `'ezîz`, `'ezîm`; internal: `be'ecok`, `ber'aqil`, `ni'or` | `'azîm` approved on 2026-08-24 (now `needs_linguist`) |
| hyphen U+002D | 71 | reduplications (`gurme-gurm`, `hew-hew`, `piste-pist`), coordinations with `-û-` (`bi-nan-û-xwê`, `rabûn-û-rûniştin`), prefix compounds (`bin-av`, `bê-êş`, `proto-arî`), loans (`e-name`, `sit-com`, `tax-free`), 1 bare `-` | `alî-palî`, `erê-na`, `kurdî-krîlî`, `mîkro-organîzma`, `self-determînasyon` | `proto-samî` marked `needs_linguist` |
| right single quotation U+2019 | 0 | none in the source | none | none |

The word-initial apostrophe in the Hunspell source marks a consonant of Arabic-origin words; the same words also exist without it (`adil`, `ezîz`, `ecl`, `ingilîzî`). Whether that mark is part of Kurmancî orthography, an acceptable variant, or a source convention to drop is a judgement this project leaves to a linguist; the policy holds the forms until then.

The questions that were put to the owner:

1. **Lexical validity.** Are word-internal `-` and `'` valid characters of a Kurmancî lexical entry at all? If yes, in which positions? (Answer 2a/2b: held for a linguist.)
2. **Which apostrophe.** U+0027 or U+2019 as canonical? (Answer 2c: undecided until the linguist review; both held.)
3. **Canonical identity.** How are `'azîm` / `azîm` and `bin-av` / `binav` related? (Answer 2d: canonical review identities remain the exact normalized forms, kept separate, with no punctuation folded into identity; punctuation variants are flagged as possible duplicates, never auto-merged, never both approved under the current policy, and whether a pair is the same lexical item is resolved by a human per pair; tooling infers no lexical equivalence.)
4. **Default-pack eligibility.** (Answer 2a/2b: not in the default pack until reviewed.)
5. **Host tokenization.** (Answer 2f: try both, deduplicate.)

The options that were offered (A: not lexical; B: word-internal only; C: lexical in any position; D: defer but hold) are recorded here for the history of the decision; the owner's answers combine D's hold with an explicit linguist gate and the duplicate-flag rule, and set `'azîm` to `needs_linguist` rather than rejected. The mechanics follow #67: one explicit dated human policy, one shared function, enforced before review, at resolution, at validation and on Review Desk exports, with the existing decision corrected transparently and never silently. Nothing is inferred from corpus frequency.

## Decision 3: reference orthography source to cite

**Current state (applied by PR #75):** on 2026-09-19 the project owner selected and approved Bedir Khan & Lescot, *Grammaire kurde (dialecte kurmandji)*, Paris 1970, Part I §2, p. 3 as the reference for the 31-letter alphabet, and Unicode default case mapping (The Unicode Standard, Version 18.0.0, §3.13 and §5.18, with `UnicodeData.txt` for the simple case mappings; no Turkish-specific dotted or dotless i tailoring for ku-Latn) as the casing reference. Both are recorded in the orthography contract's `references` with `selected_by = ferhatguneri` and `selected_on = 2026-09-19`, and both keyboard contracts are `human-reviewed` (reviewed by `ferhatguneri` on 2026-09-19). Recorded qualification of the source: §2 lists the 31 core characters and states that there are 33 if two optional characters are added; the project's production alphabet stays exactly the 31-letter policy and the optional characters are not added. The contract test checks the presence and fields of both references.

**Historical: what was put to the owner.** Until this decision the two contracts carried `review_status = draft-pending-human-review`, and the candidate references below were listed as leads, none of which the tooling had opened or verified; the owner made the selection.

| Candidate offered | What it is | Fits which need |
|---|---|---|
| Celadet Alî Bedirxan, the Hawar alphabet (Hawar journal, Damascus, first issue 1932) | origin of the Latin alphabet used for Kurmancî; the historical primary source | alphabet |
| Bedir Khan and Lescot, *Grammaire kurde (dialecte kurmandji)*, Paris 1970 (selected) | the standard descriptive grammar in that alphabet | alphabet, casing |
| Institut kurde de Paris, *Kurmancî* bulletin (language standardisation seminars, 1987 onward) | the working group that maintains standard written Kurmancî | alphabet, casing, later norms |
| Michael L. Chyet, *Kurdish-English Dictionary: Kurmanji-English*, Yale 2003 (three-volume edition 2020) | a widely used reference in the Hawar alphabet | alphabet |
| Baran Rizgar, *Kurdish-English English-Kurdish Dictionary*, London 1993 | a common learner reference in the same alphabet | alphabet |
| Unicode default case mapping (selected) | the technical basis for the casing rule the contract states | casing |

The citation changes no pack and no decision. The alphabet stays exactly the 31 letters the policy enforces; the source's optional-character qualification is recorded as a finding, not a reason for tooling to change the alphabet.

## Hands-on tasks

- [x] **Review and merge PR #72** (consolidated performance baseline): merged as 3d987a6 on 2026-09-19.
- [x] **Samsung run**: Rosetta installed, the Remote Debug Bridge connected, the Galaxy SM-S948B measured on both packs; recorded by PR #74 (merged as 3faf783). Historical instruction kept for reference: `sudo softwareupdate --install-rosetta --agree-to-license`.
- [ ] **Keep the Review Desk moving.** Queue `hunspell-kuwiki-002` (5,000 attested entries; it contains none of the held punctuation forms) is live. To merge the next batch, export the decisions with the desk's download button and give the file name in Downloads; the merge path (prepare, validate, merge, rebuild, PR) is the one used for batch 001.
- [ ] **Linguist review of the 145 held forms** (`data/review-queues/kurdish-hunspell-kmr/punctuation-policy-needs-linguist.jsonl`), including the canonical apostrophe question; recorded through the ordinary review artifacts when it happens.
- [ ] **Optional, evidence only:** on the iPhone, add Apple's Kurdish (Latin) keyboard under Settings, General, Keyboard, Keyboards, and confirm the five letters and their uppercase forms on the real device; the inspection doc marks the Apple row as simulator evidence. Not required for any decision.

## Answer sheet

| # | Decision | Answer |
|---|---|---|
| 1a | `language-model:kuwiki-20260801` redistribution: allowed / not-allowed / pending-review | **allowed** (2026-09-19) |
| 1b | Basis for 1a (who determined it, date, reading of CC BY-SA for derived n-gram tables) | Project owner, 2026-09-19: "The Kurmancî project is intended for unrestricted broad reuse, including commercial use. Project-owned code and data are made available under permissive terms; third-party materials remain subject to their recorded upstream licences, attribution requirements, and any applicable ShareAlike obligations. The project adds no additional restriction on reuse." Recorded verbatim in `NOTICE`, `corpora.toml` and the model manifest. |
| 1c | The four `allowed` source determinations (manual-seed, kurdish-hunspell-kmr, kuwiki-batch-001, kuwiki-batch-002) stand: yes / re-check | **yes**, confirmed 2026-09-19 (noted in `sources.toml`) |
| 1d | `corpus:opensubtitles-kmr`: keep as registered / re-verify terms / remove from the registry | **Delete completely** (owner, 2026-09-19): the corpus files, the registry entry and the active code and test dependencies were removed by PR #73; this record retains the historical facts the decision was made on. No replacement corpus is registered; the corpus pipeline tests and CI use a test-only fixture on an isolated root instead. |
| 2a | Policy option for hyphen: A / B / C / D (and any position rule) | **Needs linguist** (owner, 2026-09-19): no form containing a hyphen is approved into the reviewed/default pack until linguistically reviewed. Applied in PR #75. |
| 2b | Policy option for apostrophe: A / B / C / D (and any position rule) | **Needs linguist** (owner, 2026-09-19): no form containing an apostrophe is approved into the reviewed/default pack until linguistically reviewed. Applied in PR #75. |
| 2c | Canonical apostrophe code point, if admitted: U+0027 / U+2019 | **Undecided until linguistic review**; both code points are held by the policy (applied in PR #75). |
| 2d | Identity: how `'azîm` / `azîm` (and `bin-av` / `binav`) are related | **Canonical review identities remain exact and separate (no punctuation folded); punctuation variants are flagged as possible duplicates, never auto-merged, never both approved; whether a pair is the same lexical item is resolved by a human per pair, never inferred by tooling.** Applied in PR #75. |
| 2e | The approved `'azîm`: reject from default pack / move to needs_linguist / keep | **needs_linguist**: approved on 2026-08-24, moved to `needs_linguist` on 2026-09-19 with the former approval preserved in the note and evidence. Applied in PR #75. |
| 2f | Host tokenizer guidance at `-` and `'`: split / keep / try both | **Try both**: query the full token first, then punctuation-aware fallback/splitting, and deduplicate the returned suggestions. Applied in PR #75 (`word-punctuation-lookup` requirement and the integration guide). |
| 3a | Alphabet reference to cite (work, edition, location) | **Bedir Khan & Lescot, *Grammaire kurde (dialecte kurmandji)*, Paris 1970, Part I §2, p. 3** for the 31-letter alphabet, selected and approved by the owner on 2026-09-19 (the source also notes 33 characters with two optional ones; the project keeps 31). Applied in PR #75. |
| 3b | Casing reference to cite (work, edition, location), or "Unicode default case mapping" | **Unicode default case mapping** (The Unicode Standard, Version 18.0.0, §3.13 and §5.18; `UnicodeData.txt`); no Turkish-specific dotted/dotless i tailoring, selected and approved by the owner on 2026-09-19. Applied in PR #75. |

Where the answers live: the language-model manifest, the corpus and source registries and `NOTICE` for decision 1 (PR #73); `docs/lexicon-review.md`, the shared policy functions in `data-builder/src/alphabet.rs`, the review-queue generator, the resolver, the validator, the Review Desk scripts and the corrected decision record for decision 2 (PR #75); the two `data/keyboard/*.json` contracts, which become `human-reviewed`, for decision 3 (PR #75).
