# Kurmancî Binary Language Pack Layout Specification

This document formally specifies the binary language pack format (`lexicon.bin`) compiled by `kurmanci-data-builder` and consumed by `kurmanci-engine`.

---

## Pack Format Version History

| Version | Milestone | Key Additions |
| :--- | :--- | :--- |
| **v1** | Milestone 1 | Initial binary format storing word, lemma, normalized form, POS, frequency, status, regions, and sources. |
| **v2** | Milestone 2E | Added `FrequencyMetadata` (`token_count`, `document_count`, `zipf_milli`) to Lexicon entries. |
| **v3** | Milestone 3A | Added **Bigram Section** storing context-aware next-word predictions using **lexicon indices** (`u32 LE`). Zero duplicate strings. |

---

## Binary Layout (v3)

Every binary language pack consists of a **Header** followed immediately by the **Payload**.

### Header (Variable Length: `54 + language_tag_length` Bytes)

| Offset | Field | Type | Size | Description |
| :--- | :--- | :--- | :--- | :--- |
| `0..4` | `magic` | `[u8; 4]` | 4 Bytes | Magic bytes: `b"KRM1"` (`0x4B 0x52 0x4D 0x31`). |
| `4..8` | `version` | `u32 LE` | 4 Bytes | Pack format version number (Must equal `3`). |
| `8..10` | `lang_tag_len` | `u16 LE` | 2 Bytes | Length of BCP-47 language tag string in UTF-8 bytes. |
| `10..10+N` | `language_tag` | `[u8; N]` | N Bytes | BCP-47 language tag (e.g. `"ku-Latn"`). |
| `10+N..14+N` | `entry_count` | `u32 LE` | 4 Bytes | Total number of Lexicon entries in Section 1. |
| `14+N..22+N` | `payload_len` | `u64 LE` | 8 Bytes | Exact byte length of Payload following Header. |
| `22+N..54+N` | `checksum` | `[u8; 32]` | 32 Bytes | Raw SHA-256 checksum calculated over exact payload bytes. |

---

### Payload (payload_len Bytes)

The payload is divided into two sequential sections:

#### Section 1: Lexicon Section (`entry_count` items)

For each entry `i` from `0` to `entry_count - 1`:

1. `word`: Length-prefixed string (`u16 LE` len + UTF-8 bytes)
2. `lemma`: Length-prefixed string (`u16 LE` len + UTF-8 bytes)
3. `normalized`: Length-prefixed string (`u16 LE` len + UTF-8 bytes)
4. `part_of_speech`: Length-prefixed string (`u16 LE` len + UTF-8 bytes)
5. `frequency`: `u64 LE` (8 Bytes)
6. `status`: Length-prefixed string (`u16 LE` len + UTF-8 bytes)
7. `regions`: `u16 LE` count + array of length-prefixed strings
8. `sources`: `u16 LE` count + array of length-prefixed strings
9. `token_count`: `u64 LE` (8 Bytes)
10. `document_count`: `u64 LE` (8 Bytes)
11. `zipf_milli`: `u32 LE` (4 Bytes)

#### Section 2: Bigram Index Section

1. `bigram_context_count`: `u32 LE` (4 Bytes)
   - Total number of unique previous-word contexts recorded.
2. For each context from `0` to `bigram_context_count - 1`:
   - `context_lexicon_index`: `u32 LE` (4 Bytes)
     - Must satisfy `context_lexicon_index < entry_count`. Contexts are sorted in ascending order of `context_lexicon_index`.
   - `prediction_count`: `u16 LE` (2 Bytes)
     - Must satisfy `1 <= prediction_count <= 16`.
   - For each prediction from `0` to `prediction_count - 1`:
     - `next_lexicon_index`: `u32 LE` (4 Bytes)
       - Must satisfy `next_lexicon_index < entry_count`. Predictions within context are sorted by `probability_millionths` DESC, `count` DESC, next-word lexical order ASC.
     - `count`: `u64 LE` (8 Bytes)
       - Observed bigram occurrence count (`count > 0`).
     - `probability_millionths`: `u32 LE` (4 Bytes)
       - Fixed-point integer probability (`0 <= probability_millionths <= 1_000_000`).

---

## Decoder Invariants & Validation Rules

When loading a v3 binary pack, `kurmanci-engine` enforces all of the following rules:

1. **Magic Bytes**: Must equal `b"KRM1"`.
2. **Version Compatibility**: Must equal `3`.
3. **Payload SHA-256**: Calculated SHA-256 over exact payload bytes must match header `checksum`.
4. **Context Index Bounds**: `context_lexicon_index < entry_count`.
5. **Context Uniqueness**: Duplicate `context_lexicon_index` entries are rejected.
6. **Prediction Bounds**: `1 <= prediction_count <= 16`.
7. **Next Index Bounds**: `next_lexicon_index < entry_count`.
8. **Next Index Uniqueness**: Duplicate `next_lexicon_index` values within a context are rejected.
9. **Count Range**: `count > 0`.
10. **Probability Range**: `probability_millionths <= 1_000_000`.
11. **Payload Exact Match**: `payload_cursor == payload_len` (no trailing bytes).
