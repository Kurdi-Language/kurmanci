# iOS try-out screen

A way to hold a phone and see what the published engine answers while typing. It is the
remote consumer test host (`integration/apple/ios-remote-consumer`) with one SwiftUI screen,
built against the published `kurmanci-swift` package the host pins and the packs of a
published release bundle. It is an internal integration harness, not a keyboard, and it ships
nowhere: the project's deliverable stays the engine, the packs and the SDKs that platform
vendors integrate into their own keyboards (`docs/vendor-summary.md`).

## What the screen shows

- **Pack**: a switch between the `reviewed` and `experimental-full` packs of the staged
  release, with entry count, pack format, size, load time and the pack's SHA-256.
- **Type**: a text field with the system keyboard's own correction switched off, so only the
  engine's answers appear.
- **Word being typed**: whether the engine knows it, and the `suggest`, `complete` and
  `correct` results (up to five each) with their kind and edit cost.
- **Next word after** the last two words before the one being typed: the `predictNext`
  results with their source (`trigram`, `backoff`, `bigram`), count and probability.
- Every call shows its measured latency in microseconds. Tapping an answer inserts it.
- The release version, the SDK version and the bundle identity (SHA-256 of the bundle's
  `SHA256SUMS`) the packs came from.

Words go to the engine exactly as typed (the engine applies its own normalization); only
leading and trailing punctuation is stripped from the two context words, as the evaluation
tokenizer does. Nothing on the screen ranks, filters or rewrites what the engine returns.

## Running it

```bash
# Connected, unlocked iPhone that trusts this Mac; TEAMID is your Apple Development team.
scripts/apple/tryout.sh --version 0.1.1 --team TEAMID

# Or a simulator (unsigned).
scripts/apple/tryout.sh --version 0.1.1 --simulator
```

The script, fail-closed at every step:

1. Takes the bundle `kurmanci-ku-Latn-<V>` from `dist/vendor-evaluation` (the vendor evaluation
   kit's directory), fetching and verifying it through `scripts/vendor/evaluate.sh fetch` when
   it is absent. The bundle's `VERSION` must be `<V>`, and each of the two packs must hash
   exactly as the bundle's `SHA256SUMS` lists it.
2. Requires the remote consumer to pin `kurmanci-swift` at exactly `<V>` in both the project
   and its `Package.resolved`, so a release's packs are never tried through another SDK
   version.
3. Copies the packs to `integration/apple/ios-remote-consumer/Packs/<pack_id>.bin` and writes
   `Packs/packs.json` (release version, bundle identity, SDK version, each pack's SHA-256).
   `Packs/` is a folder reference the project copies into the app bundle; it is gitignored
   apart from `.gitkeep`, so the committed host carries no pack and CI builds it empty. The
   screen re-hashes every pack it loads and refuses one that differs from `packs.json`.
4. Builds the app with its own DerivedData and package cache under `dist/vendor-evaluation`,
   then installs and launches it with `xcrun devicectl` (physical device, automatic Apple
   Development signing for `--team`) or `xcrun simctl` (`--simulator`). With neither
   `--device` nor `--simulator` it picks the one physical iPhone `devicectl` reports as
   connected and refuses when there is none or several.

`--stage-only` stops after step 3. `scripts/apple/test-tryout.sh` (CI) proves the staging
record, the refusals and the device build arguments against a fake bundle, a fake `xcodebuild`
and a fake `xcrun`.

For scripted runs the screen takes its initial text from the environment variable
`KURMANCI_TRYOUT_TEXT` (`simctl launch` passes it as `SIMCTL_CHILD_KURMANCI_TRYOUT_TEXT`).

## What it is not

It measures nothing that the device benchmark (`docs/device-benchmark.md`) does not already
record with proper repetition, and the latencies it shows are single calls on a debug build
with the screen updating; use the benchmark harness for figures. It makes no linguistic or
review decision: a word that looks wrong here is reported through the review process
(`docs/lexicon-review.md`), not fixed in the harness.
