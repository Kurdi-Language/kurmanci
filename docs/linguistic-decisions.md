# Linguistic Decisions & Orthography Standard

## ISO & BCP 47 Language Tag Standard

- Primary locale tag: **`ku-Latn`**
- Regional sub-tags:
  - `ku-Latn-TR` (Turkey)
  - `ku-Latn-SY` (Syria)
  - `ku-Latn-IQ` (Iraq)

## Kurmancî Alphabet (`ku-Latn`)

31 letters:
`a, b, c, ç, d, e, ê, f, g, h, i, î, j, k, l, m, n, o, p, q, r, s, ş, t, u, û, v, w, x, y, z`

Diacritic character pairs:
- `ç` (c-cedilla)
- `ê` (e-circumflex)
- `î` (i-circumflex)
- `ş` (s-cedilla)
- `û` (u-circumflex)

## Weighted Diacritic Penalties

When typed without diacritics (e.g. `rojbas`), the distance solver assigns a reduced penalty score (0.25 vs 1.0) to candidate matches containing `ş`, `î`, `ê`, `û`, `ç`.

## Casing & Canonical Display Forms

- Lexicon entry display forms (`word`) store **lowercase** orthography for common vocabulary (`rojbaş`, `bijî`, `şevbaş`), even if source glossaries presented them capitalized for demonstration.
- Proper nouns (`Kurdî`, `Kurmancî`) maintain canonical capitalization.
- Keyboard engines and UI layers dynamically handle sentence-initial capitalization based on input context.
