# Release provenance

What a Kurmancî release bundle contains, where every byte of it comes from, what its
licensing state is, and how anyone can reproduce it. This document describes the state of
the repository; it makes no legal determination.

## The release unit

```
cargo run -p kurmanci-data-builder -- build-release-bundle            # → dist/release/kurmanci-ku-Latn-<version>/
cargo run -p kurmanci-data-builder -- verify-release-bundle dist/release/kurmanci-ku-Latn-<version>
```

`build-release-bundle` refuses unless `verify-production-state` passes and every pack is
built and current, and unless the working tree is clean (no uncommitted and no untracked
non-ignored files, because `NOTICE` and `data/licenses/` go into the bundle); it then assembles
the bundle in memory and installs it by failure-safe staged replacement:

| Path | Content | Reproducible |
|---|---|---|
| `VERSION` | release version (the engine crate version unless `--release-version` is given) | yes |
| `compatibility.json` | engine version, C ABI major/minor (read from `kurmanci.h`), pack magic, pack schema and supported schemas, language-model schema and supported schemas, language tag | yes |
| `provenance.json` | everything below, with hashes; includes the full `verify-production-state` report the bundle was built from | yes |
| `SHA256SUMS` | SHA-256 of every other file; its own hash is the identity of the bundle | yes |
| `ATTRIBUTION` | the attribution text of every pack | yes |
| `LICENSES/` | repository `LICENSE` and `NOTICE`, per-source licence and notice files from `data/licenses/` | yes |
| `include/kurmanci.h` | the C ABI header | yes |
| `packs/<pack-id>/` | `lexicon.bin`, `manifest.json`, `collision-report.jsonl`, `attribution.txt`, `artifacts.sha256` for `seed`, `reviewed`, `experimental-full` | yes |
| `language-model/<id>/` | the committed language model: vocabulary ids, numeric unigram/bigram/trigram tables, manifest, hashes (no corpus text) | yes |
| `apple/`, `android/` | optional; files or directories passed with `--apple` / `--android`, produced by `scripts/apple` and `scripts/android`. Symbolic links are never followed and fail the build, so supply symlink-bearing trees (an unpacked XCFramework) as the archive the scripts already produce | hashed as received, not asserted |

Nothing in the reproducible part carries a timestamp, a host name or a build path.
`scripts/release/verify-clean-checkout-determinism.sh` clones the repository twice at a
commit, runs the derivation steps (Hunspell import, quality audit, review queues,
validated decision reports, the three packs) in each clone, builds a bundle in each and fails unless the two
are byte-identical. CI runs it on every push and pull request.

## What data exists

Three packs, ordered by trust:

| Pack | Content | Policy (`data/pack-policy.toml`) |
|---|---|---|
| `seed` | the manually reviewed seed lexicon only | the current default pack (`default_pack = "seed"`) |
| `reviewed` | seed plus every external entry a human approved, plus next-word prediction from the committed language model | may be chosen as default (`allow_as_default = true`) but is not the current default; not opt-in |
| `experimental-full` | seed plus every mechanically valid imported entry, plus prediction | opt-in only (`opt_in = true`); never allowed as default |

`seed ⊆ reviewed ⊆ experimental-full` is verified before every bundle. The language model
(`data/language-model/<corpus>-<version>/`) is derived from the TRAIN partition of one
registered corpus, restricted to the current pack vocabulary, and contains statistics only.
Every pack's `manifest.json` names the model it embeds and the model manifest hash.

## Source provenance

`provenance.json` records, for the release as a whole:

- the source commit and the git tree id of `data/` at that commit (the data revision), and
  whether tracked files were dirty when the bundle was built;
- the Rust toolchain channel from `rust-toolchain.toml`;
- for every pack: entry count, the hash of each artifact, the pack policy hash, the review
  decision, queue and report manifest hashes, per-source review provenance, the licences of
  the included sources, and the language model id;
- for the language model: schema, manifest hash, vocabulary fingerprint and size, corpus id
  and version, the upstream dump artifact hash, the derived document file hash, TRAIN
  document count and document-set hash, the minimum n-gram counts, the hash of each file,
  and its licensing block;
- every registered source (`sources.toml`): version, licence identifier and URL,
  redistribution field;
- every registered corpus (`corpora.toml`): version, licence identifier and URL,
  acquisition mode, upstream artifact hash.

`inspect-word` shows the same provenance for one word; `verify-production-state` checks that
the tracked inputs, the committed model and the built packs agree.

## Review process

Vocabulary decisions are made by people and committed as review decisions; tooling derives
queues, reports and packs from them and never assigns, infers, changes or promotes a
status. `verify-production-state` fails on orphan decisions, duplicate decision identities,
repeated candidates across review batches and any corpus context left in tracked review
artifacts. The bundle embeds that report.

## Licensing state

`provenance.json › licensing` lists the SPDX identifiers of everything included and one
redistribution record per subject, copied verbatim from the registries and the model:

- sources declare `redistribution` in `data/source-registry/sources.toml`;
- corpora may declare a `[corpora.redistribution]` table (`determination`, `determined_by`,
  `determined_on`, `basis`) in `data/source-registry/corpora.toml`; the language model copies
  it into its manifest as `redistribution_determination` with the determiner, date and basis,
  and records `pending-review` when the registry has none. For the current Kuwiki model the
  determination is `allowed`, made by the project owner on 2026-09-19 under the project's
  licensing stance (recorded in `NOTICE`): unrestricted broad reuse including commercial use,
  with third-party materials remaining subject to their recorded upstream licences,
  attribution requirements and any applicable ShareAlike obligations. The bundle's corpus
  records carry each registered corpus's determination for information; the release gate is
  the language model's and the sources' determinations, which are what the bundle ships.

A bundle is labelled `release_kind = "production"` only when every determination is
`allowed` and the tracked tree was clean when it was built; otherwise it is `"evaluation"`
and `evaluation_notice` names what is unresolved. A bundle built with `--allow-dirty` records
`worktree_dirty = true`, is always an evaluation release, and is not reproducible from the
recorded source commit and data tree. `verify-release-bundle` enforces these rules
independently.
Neither label is a legal conclusion; they restate the recorded determinations so an
evaluator sees them before anything else.

## Reproducibility

| Claim | How it is checked |
|---|---|
| packs are a pure function of the committed decisions, policy and model | `verify-production-state` assembles every pack twice in memory and compares every artifact with the built pack |
| the language model is the committed one | loader-enforced hashes, invariants and vocabulary fingerprint |
| the bundle is a pure function of the commit | `scripts/release/verify-clean-checkout-determinism.sh` (two clean clones, byte-identical bundles) |
| a bundle is intact | `verify-release-bundle`: every file listed in `SHA256SUMS` with its hash, no unlisted file, no symbolic link, provenance and compatibility consistent with the files, release kind consistent with the recorded determinations and dirty state |
| replacing a bundle never loses the previous one | failure-safe staged replacement: the new bundle is staged completely, the previous one is parked as a backup, the stage is renamed into place, and the backup is restored if that fails; a backup that is the only copy of the previous release is restored before any new attempt. Not a no-gap atomic exchange |
| the model can be regenerated | `rebuild-production` (needs the corpus; `--acquire` is the only path that downloads) reports every artifact as unchanged |

Platform binaries (XCFramework, AAR) are outside this claim: they are produced by the
existing Apple and Android scripts, attached on request and hashed as received.

## Publishing a release

A production release `X.Y.Z` is four immutable tags on `main` and one GitHub release, in this
order. Tags are never moved; a mistake gets a new version.

1. Merge the release commit (every version source at `X.Y.Z`: workspace and crate
   manifests, `android/gradle.properties` `kurmanciVersion`, the Apple scripts' defaults, the
   READMEs; a `CHANGELOG.md` section).
2. Push `android-vX.Y.Z` and `swift-vX.Y.Z` on the merge commit. The Maven Central workflow
   publishes `io.github.ferhatguneri:kurmanci-android:X.Y.Z`; the Apple workflow publishes
   `KurmanciFFI-vX.Y.Z.xcframework.zip` with `release-manifest.json` on GitHub release
   `swift-vX.Y.Z` (the manifest's C ABI is derived from `ffi/include/kurmanci.h`,
   `scripts/apple/release-manifest.sh`) and the tag `X.Y.Z` in `Kurdi-Language/kurmanci-swift`
   (needs the repository secret `SWIFT_DIST_TOKEN`). Both workflows validate after publication
   becomes visible, so only their final success counts.
3. Tag `vendor-kit-X.Y.Z`: every production release has one, on the commit whose tree is the
   evaluation kit for that release. When no kit change was needed it is the release commit
   itself; when the kit changed after the release, it is the commit that merged the change.
   A later kit correction for the same release gets `vendor-kit-X.Y.Z-r2`, `-r3`, and so on.
   The publisher accepts the tag only if its commit is on `origin/main`, the tree carries
   `docs/vendor-evaluation-kit.md`, `scripts/vendor/evaluate.sh` and the remote iOS consumer,
   the document names `vendor-kit-X.Y.Z` and `--version X.Y.Z` and no other release, and the
   remote consumer's package requirement and `Package.resolved` pin the Swift package at
   exactly `X.Y.Z`; a stale kit tree cannot be published under a new release's notes.
4. `scripts/release/publish-bundle.sh --version X.Y.Z` (tested by
   `scripts/release/test-publish-bundle.sh` in CI) fails closed on each of these before it
   creates anything: the remote tags `android-vX.Y.Z` and `swift-vX.Y.Z` resolve to one
   commit on `origin/main` (local tags must agree but are not authoritative); the two SDK
   workflow runs for that commit completed successfully; `vendor-kit-X.Y.Z` exists; release
   `vX.Y.Z` does not exist and the tag `vX.Y.Z` is absent or already at that commit; the
   downloaded AAR and XCFramework are what `release-manifest.json` (schema, version, source
   commit, artifact hash, C ABI as the header at the commit) and the Swift distribution tag's
   `source-manifest.json` and `Package.swift` (version, source commit, checksum, immutable
   URL whose bytes equal the release asset) describe; a clean clone at the commit runs the
   derivation steps of `verify-clean-checkout-determinism.sh` and stays clean; the bundle
   built with both artifacts attached is a production release from a clean tree with the
   requested `release_version`, `provenance.source.commit` equal to the commit and exactly the
   two expected platform-artifact records; `vendor-kit-X.Y.Z` passes the checks of step 3; the notes render from
   `release-notes.template.md` with no placeholder left. It then creates the tag `vX.Y.Z` at
   the commit if absent, pushes it, and creates the release on the verified tag with the
   archive, its `.sha256`, `SHA256SUMS` and `provenance.json`. `--skip-publish` runs every
   check and leaves the results under `dist/release-publish/X.Y.Z/out/`.

Identities: the extracted bundle is deterministic, and `SHA256SUMS` with its sha256
identifies the reproducible bundle contents (the value the release notes quote and
`verify-clean-checkout-determinism.sh` reproduces). The `.tar.gz` is not itself claimed
byte-identical across rebuilds, since archiver metadata may differ;
`<bundle>.tar.gz.sha256` identifies the exact archive uploaded to that GitHub release.

Release 0.1.1 predates the derived manifest ABI: its `release-manifest.json` records C ABI
1.0 while the header at its commit says 1.1. Published artifacts and tags are immutable, so
that record stands; reconstructing 0.1.1 with the publisher requires
`--accept-apple-manifest-abi 1.0`, which prints the discrepancy instead of hiding it. The
exception is mechanical (release 0.1.1, recorded 1.0, header 1.1, and the flag); any other
ABI disagreement, for any release, is fatal with or without the flag.
