# Data Schemas & Formats

## 1. Lexicon Entry Schema (`data/lexicon/*.json`)

```json
{
  "word": "rojbaş",
  "normalized": "rojbaş",
  "lemma": "rojbaş",
  "part_of_speech": "interjection",
  "frequency": 84231,
  "regions": ["general"],
  "status": "verified",
  "sources": ["source-id-1"]
}
```

## 2. Typo Pair Schema (`data/errors/*.json`)

```json
{
  "incorrect": "rojbas",
  "intended": "rojbaş",
  "category": "missing_diacritic",
  "source": "handcrafted_benchmarks",
  "count": 1420
}
```

## 3. Provenance Schema (`data/licenses/provenance.json`)

```json
{
  "source_id": "seed-lexicon-v1",
  "name": "Kurmancî Language Platform Seed Corpus",
  "license": "CC BY 4.0",
  "retrieval_date": "2026-07-30",
  "url": "https://github.com/Kurdi-Language/kurmanci",
  "permitted_uses": ["distribution", "commercial_integration", "modification"],
  "attribution_text": "Kurmancî Language Platform Contributors (CC BY 4.0)"
}
```
