# Human review pack

Decisions that only the project owner (or a linguist or lawyer the owner names) can make, with the facts each one needs and a fill-in answer sheet at the end. Tooling prepared the facts; nothing here is a legal or linguistic conclusion, and where candidates are listed they are leads for a human to verify, not choices. Counts come from the committed data on `main` at `5ed81d9` (after PR #71).

How to answer: fill the answer sheet at the bottom of this file and commit it, or reply in chat with the decision numbers. Blank means undecided and nothing is acted on. Once the sheet is filled, one PR writes the answers where they belong (registry fields, policy document, contract citations) for the usual review.

Hands-on items that need the owner's machine or hands rather than a decision are listed last.

## Decision 1: licensing and redistribution determinations

Decided on 2026-09-19 (answer sheet 1a to 1d) and applied in this PR: the language model `kuwiki-20260801` is now `allowed`, with the owner's statement recorded as the basis; the four source determinations are confirmed; the OpenSubtitles corpus files, registry entry and active dependencies are removed, with no replacement corpus in the production registry. The inventory below is kept as the historical record of what the decision was made on, which is why the OpenSubtitles row still appears in it.

| Subject | What it is | Licence recorded | Attribution recorded | Determination recorded | Where it is used |
|---|---|---|---|---|---|
| `language-model:kuwiki-20260801` | bigram and trigram tables derived from the Kurmancî Wikipedia dump of 2026-08-01 (vocabulary 42,247 forms; no article text) | CC BY-SA 4.0 | Wikipedia contributors, Wîkîpediya (Kurmancî edition), CC BY-SA 4.0 | **pending-review** | reviewed and experimental-full packs (next-word prediction); release bundle |
| `source:manual-seed` | the 33 hand-written seed entries (`data/reviewed/lexicon.jsonl`) | Apache-2.0 | none required | allowed | seed, reviewed, experimental-full |
| `source:kurdish-hunspell-kmr` | KurdishHunspell Kurmancî dictionary and affix files, pinned to commit `88131d68…` (https://github.com/sinaahmadi/KurdishHunspell) | CC BY-SA 4.0 (upstream LICENSE preserved under `data/original/`) | required | allowed | reviewed (approved entries only) and experimental-full |
| `source:kuwiki-batch-001` | 1,000 reviewed word candidates from the Wikipedia corpus (words and counts only) | CC BY-SA 4.0 | required | allowed | reviewed and experimental-full |
| `source:kuwiki-batch-002` | second batch, same kind | CC BY-SA 4.0 | required | allowed | reviewed and experimental-full |
| `corpus:kuwiki` | the Wikipedia dump (43 MB) and the extracted prose (63 MB), never committed, acquired on demand | CC BY-SA 4.0 | as above | none recorded (the corpus is not shipped) | builds the language model and the review batches |
| `corpus:opensubtitles-kmr` | a line-delimited dialogue corpus committed under `data/original/opensubtitles-kmr/` | recorded as CC BY-SA 4.0 | OpenSubtitles project contributors and Kurdi-Language maintainers | none recorded | registered for frequency tables; not part of the current language model or the release bundle |

What the determination has to answer for the language model:

1. Are bigram and trigram counts derived from CC BY-SA text a derivative work that must itself be CC BY-SA, or data that can be shipped under the project's terms with attribution? The repository takes no position; the release tooling copies the recorded answer.
2. If CC BY-SA applies, is that acceptable for the target vendors, who ship the pack inside proprietary software? ShareAlike on a data file inside a keyboard is the question a vendor's counsel will ask.
3. Does the answer differ between the reviewed pack (model cut to 2,144 words) and experimental-full?

Separate check on `corpus:opensubtitles-kmr`: the registry records CC BY-SA 4.0 and a project-internal URL, but the project handoff notes that OpenSubtitles material needs its own licensing review. It is in no shipped artifact today, so it blocks nothing; decide whether to keep it registered, re-verify its terms, or remove it.

Options for the language-model line:

- **allowed**: redistribution is determined to be permitted; the bundle becomes a production release once built from a clean tree. Record the basis (who determined it, when, on what reading) and it is written into the manifest and the registry notes.
- **not-allowed**: the model stays for evaluation but leaves production packs; prediction would then need a differently licensed corpus. A product decision with real cost; not a default.
- **pending-review**: unchanged; releases stay evaluation-only.

Watch-list for future sources, so nothing is imported by accident: NLP Kurdî (reference only unless rights are clear), KurdishLex (CC BY-NC-SA, commercially problematic), OPUS/OpenSubtitles (needs review), Tatoeba (verify current terms), OSCAR/Common Crawl (legal review).

## Decision 2: word-internal hyphen and apostrophe policy

The alphabet policy (#67) deliberately left hyphens and apostrophes undecided: an entry containing one is neither excluded nor admitted by the 31-letter rule, so such forms pass through review like any other. One has already been approved and sits in the reviewed pack: `'azîm` (Hunspell entry, approved 2026-08-24, leading apostrophe). The orthography contract says the same: "whether and how they occur inside words is a separate, human-reviewed tokenization policy". This decision closes that gap.

What the data holds today (Hunspell review pool of 41,403 forms; the two Wikipedia batches and the seed lexicon have none):

| Character | Forms in the pool | Pattern | Examples (dictionary forms, not corpus text) | Decisions taken so far |
|---|---|---|---|---|
| apostrophe U+0027 | 74 | 70 word-initial (`'a…`, `'e…`, `'i…`, `'î…`), 4 word-internal, 1 bare `'` | `'abd`, `'adil`, `'aqilane`, `'aqildar`, `'edl`, `'ecêb`, `'ezîz`, `'ezîm`; internal: `be'ecok`, `ber'aqil`, `ni'or` | `'azîm` approved (in the reviewed pack) |
| hyphen U+002D | 71 | reduplications (`gurme-gurm`, `hew-hew`, `piste-pist`), coordinations with `-û-` (`bi-nan-û-xwê`, `rabûn-û-rûniştin`), prefix compounds (`bin-av`, `bê-êş`, `proto-arî`), loans (`e-name`, `sit-com`, `tax-free`), 1 bare `-` | `alî-palî`, `erê-na`, `kurdî-krîlî`, `mîkro-organîzma`, `self-determînasyon` | `proto-samî` marked needs_linguist |
| right single quotation U+2019 | 0 | none | none | none |

The word-initial apostrophe in the Hunspell source marks a consonant of Arabic-origin words; the same words also exist without it (`adil`, `ezîz`, `ecl`, `ingilîzî`). Whether that mark is part of Kurmancî orthography, an acceptable variant, or a source convention to drop is a judgement this project leaves to a human.

The questions:

1. **Lexical validity.** Are word-internal `-` and `'` valid characters of a Kurmancî lexical entry at all? If yes, in which positions (initial, internal, final)?
2. **Which apostrophe.** If an apostrophe is admitted, which code point is canonical: U+0027 (as in the Hunspell source) or U+2019 (typographic)? Keyboards on all three platforms insert U+0027 by default; iOS smart punctuation may substitute U+2019.
3. **Canonical identity.** Should review identity treat `'azîm` and `azîm` as two entries (as now) or one? Same for `bin-av` versus `binav`. Today identity is exact after NFC and lowercase, so they are two.
4. **Default-pack eligibility.** May such forms enter the reviewed (default) pack, or only experimental-full, or neither?
5. **Host tokenization.** What should a vendor's tokenizer do at a hyphen or apostrophe when it looks up a word: split, keep, or try both? The engine only answers for the string it is given.

Options and what each does mechanically (choose per character; A and B are the two clean ones):

- **A. Not lexical characters.** Hyphen and apostrophe are excluded from default-pack entries like digits are. Effect: `'azîm` is corrected to `rejected_from_default_pack` by the same mechanical policy step used for the 13 Kuwiki alphabet corrections (original decision preserved as evidence); the 74 + 71 pool forms move to an `excluded_by_tokenization_policy` queue and never reach the desk; hosts split at these characters. Simplest for vendors; loses hyphenated compounds and reduplications as entries.
- **B. Lexical, word-internal only.** A hyphen or apostrophe is admitted between two letters, never at the edges. Effect: `bin-av`, `hew-hew`, `be'ecok` stay reviewable; the 70 word-initial apostrophe forms and the two bare characters are excluded; `'azîm` is corrected as under A. Needs question 2 answered so the identity rule can fold the two apostrophes.
- **C. Lexical in any position.** Everything stays reviewable, including the initial-apostrophe forms; `'azîm` stands. Needs questions 2 and 3 answered, and vendors must be told that entries may begin with an apostrophe.
- **D. Defer, but stop the leak.** Keep the policy open, but until it is decided such forms are held back from the review desk (neither excluded nor approved). `'azîm` is moved to `needs_linguist` rather than rejected, with the original decision preserved. Cheapest now; it parks the question.

Whatever is chosen, the mechanics follow #67: one explicit dated human policy in `docs/lexicon-review.md`, one shared eligibility function, enforced before review, at resolution and at validation, with existing decisions corrected transparently and never silently. Nothing is inferred from corpus frequency.

## Decision 3: reference orthography source to cite

The orthography contract (`data/keyboard/ku-Latn-orthography.json`) and the keyboard requirements stay `draft-pending-human-review` until they cite an external reference for the 31-letter alphabet and the casing rule. The project's own policy of 2026-09-17 stays normative internally; the citation tells a vendor where the alphabet comes from. Choosing the authority is a human call. Below are the sources usually named for the Kurmancî Latin alphabet, in no order of preference; none has been opened or verified for this pack, so each line is a lead to confirm.

| Candidate | What it is | What to verify before citing | Fits which need |
|---|---|---|---|
| Celadet Alî Bedirxan, the Hawar alphabet (Hawar journal, Damascus, first issue 1932) | origin of the Latin alphabet used for Kurmancî; the historical primary source | exact issue and page where the letters are set out; whether the inventory there equals the 31 letters | alphabet |
| Bedir Khan and Lescot, *Grammaire kurde (dialecte kurmandji)*, Paris 1970 | the standard descriptive grammar in that alphabet | edition and page for the alphabet table and for capitalisation | alphabet, casing |
| Institut kurde de Paris, *Kurmancî* bulletin (language standardisation seminars, 1987 onward) | the working group that maintains standard written Kurmancî | which issue states the alphabet or casing; availability online | alphabet, casing, later norms |
| Michael L. Chyet, *Kurdish-English Dictionary: Kurmanji-English*, Yale 2003 (three-volume edition 2020) | a widely used reference in the Hawar alphabet with the alphabet in its front matter | the front-matter pages describing the alphabet | alphabet |
| Baran Rizgar, *Kurdish-English English-Kurdish Dictionary*, London 1993 | a common learner reference in the same alphabet | front-matter statement of the alphabet | alphabet |
| Unicode default case mapping (Unicode Standard, Chapter 3 and `UnicodeData.txt`) | the technical basis for the casing rule the contract already states (i ↔ I, î ↔ Î, ş ↔ Ş, ç ↔ Ç, û ↔ Û; no Turkish-style dotted or dotless i rule) | the Unicode version to cite | casing (technical) |

A citation needs three things so the contract can carry it as data: the work and edition, the location (page, section or issue), and which claim it supports (letter inventory, letter names, casing). One source for the alphabet and one for the casing convention is enough.

The citation changes no pack and no decision. The alphabet stays exactly the 31 letters the policy enforces; if a chosen source lists a different inventory, that is a finding to record, not a reason for tooling to change the alphabet.

## Hands-on tasks

- [ ] **Review and merge PR #72** (consolidated performance baseline, documentation only): https://github.com/Kurdi-Language/kurmanci/pull/72. Head `bb82b4f`; CI run 463 green.
- [ ] **Install Rosetta for the Samsung run.** Samsung's Remote Debug Bridge is an Intel-only program. In Terminal, paste the line below, press Return, type the Mac login password (nothing shows while typing), wait for "installed", then say so. The reserved Galaxy device is then connected to adb and both packs are measured, giving the first physical Android row. If the Samsung download page offers an Apple Silicon build, that avoids Rosetta.

  ```
  sudo softwareupdate --install-rosetta --agree-to-license
  ```

- [ ] **Keep the Review Desk moving.** Queue `hunspell-kuwiki-002` (5,000 attested entries) is live. To merge the next batch, export the decisions with the desk's download button and give the file name in Downloads; the merge path (prepare, validate, merge, rebuild, PR) is the one used for batch 001.
- [ ] **Optional, evidence only:** on the iPhone, add Apple's Kurdish (Latin) keyboard under Settings, General, Keyboard, Keyboards, and confirm the five letters and their uppercase forms on the real device; the inspection doc marks the Apple row as simulator evidence. Not required for any decision.

## Answer sheet

| # | Decision | Answer |
|---|---|---|
| 1a | `language-model:kuwiki-20260801` redistribution: allowed / not-allowed / pending-review | **allowed** (2026-09-19) |
| 1b | Basis for 1a (who determined it, date, reading of CC BY-SA for derived n-gram tables) | Project owner, 2026-09-19: "The Kurmancî project is intended for unrestricted broad reuse, including commercial use. Project-owned code and data are made available under permissive terms; third-party materials remain subject to their recorded upstream licences, attribution requirements, and any applicable ShareAlike obligations. The project adds no additional restriction on reuse." Recorded verbatim in `NOTICE`, `corpora.toml` and the model manifest. |
| 1c | The four `allowed` source determinations (manual-seed, kurdish-hunspell-kmr, kuwiki-batch-001, kuwiki-batch-002) stand: yes / re-check | **yes**, confirmed 2026-09-19 (noted in `sources.toml`) |
| 1d | `corpus:opensubtitles-kmr`: keep as registered / re-verify terms / remove from the registry | **Delete completely** (owner, 2026-09-19): the corpus files, the registry entry and the active code and test dependencies were removed in this PR; this record retains the historical facts the decision was made on. No replacement corpus is registered; the corpus pipeline tests and CI use a test-only fixture on an isolated root instead. |
| 2a | Policy option for hyphen: A / B / C / D (and any position rule) | **Needs linguist** (owner, 2026-09-19): no form containing a hyphen is approved into the reviewed/default pack until linguistically reviewed. Applied in the next PR. |
| 2b | Policy option for apostrophe: A / B / C / D (and any position rule) | **Needs linguist** (owner, 2026-09-19): no form containing an apostrophe is approved into the reviewed/default pack until linguistically reviewed. Applied in the next PR. |
| 2c | Canonical apostrophe code point, if admitted: U+0027 / U+2019 | **Undecided until linguistic review**; both code points are held by the policy. |
| 2d | Identity: `'azîm` and `azîm` (and `bin-av` / `binav`) are one entry or two | **Flag punctuation variants as possible duplicates; never auto-merge; never allow both to be approved.** Human resolution per pair. Applied in the next PR. |
| 2e | The approved `'azîm`: reject from default pack / move to needs_linguist / keep | **needs_linguist**, former approval preserved as evidence. Applied in the next PR. |
| 2f | Host tokenizer guidance at `-` and `'`: split / keep / try both | **Try both**: query the full token first, then punctuation-aware fallback/splitting, and deduplicate the returned suggestions. Applied in the next PR (keyboard requirements + integration guide). |
| 3a | Alphabet reference to cite (work, edition, location) | **Bedirxan & Lescot, *Grammaire kurde (dialecte kurmandji)*, Part I §2, p. 3** for the 31-letter alphabet (owner, 2026-09-19). Applied in the next PR. |
| 3b | Casing reference to cite (work, edition, location), or "Unicode default case mapping" | **Unicode default case mapping**; no Turkish-specific dotted/dotless i behaviour (owner, 2026-09-19). Applied in the next PR. |

All twelve answers were given on 2026-09-19. Decision 1 is applied in PR #73; decisions 2 and 3 are applied in the following PR from main. The mapping: the language-model manifest and source registry for 1; `docs/lexicon-review.md`, the shared eligibility function, the review-queue generator, the resolver and the corrected decisions for 2 (every original decision preserved as evidence); the two `data/keyboard/*.json` contracts for 3, which then leave draft status.
