# Binary Language Pack (`lexicon.bin`) Format Specification

The versioned binary pack (`lexicon.bin`) is deterministically encoded for validated runtime parsing.

---

## 1. Container Header Specification

| Offset | Field | Type | Size | Endianness | Description | Failure Condition |
|---|---|---|---|---|---|---|
| `0x00` | Magic Bytes | ASCII String | 4 bytes | — | Constant `KRM1` (`0x4B 0x52 0x4D 0x31`) | Invalid magic bytes |
| `0x04` | Format Version | `u32` | 4 bytes | Little Endian | Container layout version (Current: `1`) | Unsupported format version |
| `0x08` | Language Tag Length | `u16` | 2 bytes | Little Endian | Byte length of language identifier | Truncated header |
| `0x0A` | Language Tag Bytes | `[u8]` | Variable | UTF-8 | Locale tag (Expected: `"ku-Latn"`) | Incompatible language tag |
| Variable | Entry Count | `u32` | 4 bytes | Little Endian | Number of encoded lexicon entries | Count mismatch / Truncated header |
| Variable | Payload Byte Length | `u64` | 8 bytes | Little Endian | Total size of entry payload in bytes | Payload size mismatch |
| Variable | SHA-256 Checksum | `[u8; 32]` | 32 bytes | — | Raw 256-bit SHA-256 hash of payload bytes | Checksum mismatch / Corruption |

---

## 2. Entry Payload Specification

Each entry in the payload is sequentially encoded without padding bytes.

| Sequence | Field | Type / Encoding | Size | Description & Constraints |
|---|---|---|---|---|
| 1 | `word_len` | `u16` Little Endian | 2 bytes | Byte length of display word. Max: `65,535` bytes (`u16::MAX`). |
| 2 | `word_bytes` | UTF-8 String | `word_len` bytes | Original display orthography (e.g., `rojbaş`, `Kurdî`). |
| 3 | `lemma_len` | `u16` Little Endian | 2 bytes | Byte length of grammatical base lemma. |
| 4 | `lemma_bytes` | UTF-8 String | `lemma_len` bytes | Canonical lemma string (e.g., `rojbaş`). |
| 5 | `norm_len` | `u16` Little Endian | 2 bytes | Byte length of NFC normalized search key. |
| 6 | `norm_bytes` | UTF-8 String | `norm_len` bytes | Canonical lowercase search key (e.g., `rojbaş`). |
| 7 | `pos_len` | `u16` Little Endian | 2 bytes | Byte length of part-of-speech string. |
| 8 | `pos_bytes` | UTF-8 String | `pos_len` bytes | Tag (e.g., `noun`, `verb`, `adjective`, `interjection`). |
| 9 | `frequency` | `u64` Little Endian | 8 bytes | Unsigned 64-bit document/corpus frequency count. |
| 10 | `status_len` | `u16` Little Endian | 2 bytes | Byte length of verification status string. |
| 11 | `status_bytes` | UTF-8 String | `status_len` bytes | Status tag (`verified`, `imported`, `unverified`). |
| 12 | `regions_count` | `u16` Little Endian | 2 bytes | Number of associated region tags (`0` to `65,535`). |
| 13 | `regions` | Array of Strings | Variable | For each region: `u16` length + UTF-8 bytes (`ku-Latn-TR`, `general`). |
| 14 | `sources_count` | `u16` Little Endian | 2 bytes | Number of source identifiers (`0` to `65,535`). |
| 15 | `sources` | Array of Strings | Variable | For each source: `u16` length + UTF-8 bytes (`manual-seed`, etc.). |

---

## 3. String Length Limits & Bounds

- **Maximum String Length**: All string fields are prefixed with a 16-bit unsigned integer (`u16`). Any string exceeding `65,535` bytes (`u16::MAX`) will cause a compilation error.
- **UTF-8 Validation**: Decoded byte sequences must form valid UTF-8. Non-UTF-8 sequences trigger deserialization failure.

---

## 4. Future Extensibility Roadmap

- **Header Length Directory**: Version `2` container formats will introduce an explicit section index offset directory after the language tag to support modular payload sections (frequencies, n-grams, morphology tables) zero-copy.
