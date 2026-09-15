# Pack compatibility contract

What a consumer of the Kurmancî engine can rely on when pairing an engine build with a
language pack. The rules below are implemented in `engine/src/compat.rs` and enforced by
`Engine::load_binary_pack`; they are the source of truth for the C ABI error codes, the Swift
and Kotlin wrappers, and the release compatibility manifest.

## Versions

| Identity | Where it lives | Current value |
|---|---|---|
| Engine version | `kurmanci_engine::ENGINE_VERSION` (crate version) | 0.1.0 |
| C ABI version | `kmr_abi_version_major()` / `kmr_abi_version_minor()` | 1.0 |
| Pack schema version | bytes 4..8 of every pack; `PACK_SCHEMA_VERSION` | 4 |
| Supported pack schemas | `SUPPORTED_PACK_SCHEMA_VERSIONS` | [4] |
| Language-model schema | implied by the pack schema; `LANGUAGE_MODEL_SCHEMA_VERSION` | 1 (carried by pack schema 4) |
| Language tag | length-prefixed string after the schema version in every pack; `SUPPORTED_LANGUAGE_TAG` | `ku-Latn` |
| Data revision, model id, model provenance | `manifest.json` next to the pack; release provenance | not read by the runtime |

`CompatibilityTable::current()` returns all of the above as one serializable value, and
`probe_pack_header(bytes)` reads a pack's declared schema, language tag, entry count and
payload length without decoding it, reporting `is_supported` instead of failing for an
unsupported schema or language.

## Rules

A pack loads if and only if every rule holds. There is no partial load and no fallback.

| Condition | Rust error | Class | C status |
|---|---|---|---|
| fewer than 12 bytes | `TooShort` | Truncated | `KMR_ERROR_INVALID_PACK` |
| magic bytes are not `KRM1` | `InvalidMagicBytes` | Corrupt | `KMR_ERROR_INVALID_PACK` |
| schema version not supported (older **or** newer) | `UnsupportedVersion { found }` | UnsupportedSchema | `KMR_ERROR_UNSUPPORTED_PACK` |
| language tag is not `ku-Latn` (exact, case-sensitive) | `IncompatibleLanguage { found }` | WrongLanguage | `KMR_ERROR_INCOMPATIBLE_LANGUAGE` |
| header or payload shorter than declared | `TruncatedPayload` | Truncated | `KMR_ERROR_INVALID_PACK` |
| payload SHA-256 differs from the stored checksum | `ChecksumMismatch` | Corrupt | `KMR_ERROR_CHECKSUM` |
| any structural rule violated (see below) | `InvalidPayload { message }` | Corrupt | `KMR_ERROR_INVALID_PACK` |
| file cannot be read | `EngineError::Io` | – | `KMR_ERROR_IO` |

`PackLoadError::failure_class()` gives the class; wrappers map from the class or the variant,
never from message text. Checks run in the order of the table: an unsupported schema is
reported before the language, the language before the checksum.

Structural rules of schema 4, all checked with the checksum already verified: entry count,
bigram context count and trigram context count must fit in the payload; every string is
valid UTF-8; context and next indexes point inside the lexicon; no duplicate contexts and no
duplicate next indexes within a context; one to 16 bigram predictions and one to 12 trigram
predictions per context; counts are non-zero; probabilities are at most 1,000,000; the
payload is consumed exactly, with no trailing bytes.

## Fail-safety guarantees

- Loading never panics on malformed input. `engine/tests/pack_compatibility_test.rs` flips
  every bit of every byte of a valid pack, sets every byte to 0x00 and 0xFF, truncates at every
  length and appends bytes; every case returns an error of the documented class.
- Counts declared in the header are untrusted. Pre-allocation is bounded by what the payload
  can physically contain (42 bytes per entry, 22 per bigram context, 26 per trigram context at
  minimum), so a corrupt count is rejected instead of requesting a huge allocation.
- A failed load leaves the engine unchanged: structures are staged and swapped in only after
  the whole payload has been validated.
- The language tag is inside the binary, so the runtime rejects a pack for another language
  without consulting any manifest.

## Backward compatibility

`engine/tests/fixtures/pack-v4-minimal.bin` is a committed schema-4 pack. The test suite
asserts that it is byte-identical to today's encoder output and that it loads with the
expected answers. When a schema 5 is introduced, this fixture stays in the suite as long as
schema 4 remains in `SUPPORTED_PACK_SCHEMA_VERSIONS`, a schema-5 fixture is added, and a
schema-6 pack must fail with `UnsupportedVersion`. Compatibility is declared by that list,
never discovered.

## What this contract does not cover

- Ranking, prediction and normalization behaviour: unchanged by this contract and documented
  elsewhere.
- Which words a pack contains: a human review decision recorded in the data pipeline.
- Licensing and provenance of a pack's statistics: recorded in `manifest.json` and the release
  provenance, and checked by `validate-pack-manifest` at build time.
