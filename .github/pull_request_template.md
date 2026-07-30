## Summary of Changes

- [ ] Change is focused and documented.
- [ ] Tests were added or updated.
- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] Generated files (`data/build/`) were not edited manually.
- [ ] Data changes include provenance and licensing entries in `data/source-registry/sources.toml`.
- [ ] Documentation was updated.
- [ ] No secrets, credentials, or personal machine paths are included.

## Verification Executed

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo run -p kurmanci-data-builder -- build
cargo run -p kurmanci-cli -- suggest rojb
```
