# Apple Integration Test Fixture Pack

This directory contains the authoritative binary language pack fixture used for macOS and iOS integration testing.

- **File**: `apple_consumer_test.bin`
- **Source Definition**: Hand-reviewed seed lexicon (`data/reviewed/lexicon.jsonl`)
- **Pack ID**: `seed`
- **Binary SHA-256**: `4e186130f1d00893f12d3cb7684945fe55c4414a1b31f910571c84ce5a12a8f1`
- **Format Version**: 4 (`PACK_VERSION = 4`)
- **Entry Count**: 33 entries (including `welat`, `spas`, `rojava`, `ez`, `diçim`)
- **Regeneration Command**:
  ```bash
  cargo run -p kurmanci-data-builder -- build-pack seed
  cp data/build/packs/seed/lexicon.bin integration/apple/fixtures/apple_consumer_test.bin
  ```
