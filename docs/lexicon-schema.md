# Lexicon Entry JSONL Schema Specification

Each entry in `data/reviewed/lexicon.jsonl` is a single valid JSON object representing a canonical Kurmancî lexeme:

```json
{
  "word": "rojbaş",
  "lemma": "rojbaş",
  "normalized": "rojbaş",
  "part_of_speech": "interjection",
  "frequency": 84231,
  "status": "verified",
  "variants": [],
  "sources": ["wiktionary", "seed-lexicon-v1"],
  "regions": ["general"]
}
```

## Field Constraints

- `word` (string, required): Original display orthography.
- `lemma` (string, required): Base grammatical lemma form.
- `normalized` (string, required): Lowercase NFC normalized search index form.
- `part_of_speech` (string, required): POS tag (`noun`, `verb`, `adjective`, `adverb`, `pronoun`, `preposition`, `conjunction`, `interjection`).
- `frequency` (unsigned integer, required): Raw/normalized document frequency count.
- `status` (string, required): Verification status (`verified`, `accepted_variant`, `unverified`).
- `sources` (array of strings, optional): Source provenance identifiers.
- `regions` (array of strings, optional): Geographical sub-regions (`general`, `ku-Latn-TR`, `ku-Latn-SY`, `ku-Latn-IQ`).
