# Platform keyboard inspection for ku-Latn (2026-09-17)

Human-observed implementation evidence from the three major mobile keyboard ecosystems,
gathered by the project owner and the project tooling on 2026-09-17. It supports the
vendor-neutral capability contract in `data/keyboard/ku-Latn-keyboard-requirements.json`.
It is evidence, not a rule: no observation below becomes a keyboard requirement, a
prescribed arrangement, or a change to the orthography contract. Screenshots are kept outside
the repository.

Conclusion: Apple, Google/Gboard and Samsung each ship a first-party Kurdish/Kurmancî
keyboard implementation on which all 31 lowercase letters of the alphabet were observed
typeable. On Apple and Gboard the uppercase forms of the five distinct letters were observed
as well; on Samsung `i` → `I` and standard shift were observed, but shifted long-press
access to `Ç Ê Î Ş Û` was not captured, so uppercase access to the five letters on Samsung is
not established by this evidence. Their physical arrangements and their access mechanisms
for `ç ê î ş û` differ. The project therefore
prescribes no universal physical layout or access mechanism (`layout_arrangement.status =
intentionally-unspecified`, a project decision recorded in the requirements file).

## Apple

Evidence level: preliminary. iOS 27.0 Simulator (iPhone 17, Xcode 27); real iPhone
confirmation still pending. Long-press behaviour was not observable without touch input.

Observed:

- input mode `ku_Latn`, software layout `QWERTY-Kurdish-Kurmanji`, hardware layout
  `Kurdish-Kurmanji`; language indicator "kurdî", space key "valahî";
- all 31 letters available; `ç ê î ş û` are dedicated keys;
- rows: `q w e r t y u i o p ê û` / `a s d f g h j k l ş î` / `z x c v b n m ç`;
- uppercase of the five observed: shift shows `Ê Û Ş Î Ç` on the same keys; `i` shifts to `I`;
- autocapitalization offered.

## Google (Gboard)

Evidence level: Android 17 emulator (Pixel 10 image) with a shipping Gboard build (18.0).

Observed:

- Gboard offers "Kurdish" (Latin script, space key "Kurdî") with the layouts "Kurdish",
  "Handwriting" and "QWERTY"; the default "Kurdish" layout was inspected;
- all 31 letters available; `ç ê î ş û` are dedicated keys;
- rows: `q w e r t y u i o p û` / `a s d f g h j k l ê î` / `z x c v b n m ç ş`, an
  arrangement different from Apple's;
- uppercase of the five observed: shift shows `Û Ê Î Ç Ş` on the same keys; `i` shifts to `I`;
- long press offers digits on the top row and vendor extras that are not part of the
  Kurmancî alphabet.

## Samsung (Samsung Keyboard)

Evidence level: real Galaxy S26 hardware through Samsung Remote Test Lab, driven by the
project owner; five screenshots.

Observed:

- language shown as "Kurdî", described as Kurdish (Kurmanji);
- a plain QWERTY arrangement with a dedicated digit row: `q w e r t y u i o p` /
  `a s d f g h j k l` / `z x c v b n m`; no dedicated keys for `ç ê î ş û`;
- all 31 lowercase letters observed: `ç ê î ş û` are reached by long press on their base
  letters, in popups that also offer accented variants outside the Kurmancî alphabet;
- `i` shifts to `I`; standard shift observed;
- shifted long-press access to `Ç Ê Î Ş Û` was not captured, so uppercase access to the five
  letters is not established by this evidence (the contract's requirement of lower and upper
  case typeability is unchanged; this is an evidence gap, not a finding against Samsung);
- Kurmancî next-word prediction and diacritic restoration observed (a suggestion of
  "Evîndar" for the typed "Evindar"), the linguistic-correction case the
  `distinct-letter-identity` requirement leaves open.

Not captured: the settings language list, the exact long-press popup of each base letter,
shifted long press, and layout options.

## What follows for the contracts

- Requirement kept: all 31 letters, in lower and upper case, directly or conveniently
  typeable; `access_mechanism = vendor-choice` (dedicated keys on Apple and Gboard, long
  press on Samsung are all acceptable mechanisms).
- No rows, key counts, dedicated keys, long press or QWERTY variant are mandated.
- The orthography contract is unchanged: the 31 letters, `ç ê î ş û` as distinct letters,
  Unicode-default casing with `i ↔ I`, canonical normalization. Characters a vendor shows in
  a long-press menu are irrelevant to the Kurmancî lexical contract.
- `data/keyboard/layout_ku.json` (engine typo adjacency) assumes the five letters sit next
  to their base letters, which corresponds to none of the observed vendor layouts. It stays
  non-authoritative and unchanged here; evaluating adjacency and edit-cost evidence against
  real vendor layouts is a separate future engine task requiring correction and ranking
  evaluation.

## Remaining human questions

- Reference orthography citation for the alphabet and casing.
- Word-internal hyphen and apostrophe tokenization policy.
- Further platform verification is welcome as interoperability evidence (real iPhone,
  Samsung settings, Samsung shifted long press for `Ç Ê Î Ş Û`) but is not a prerequisite
  for the contract.
