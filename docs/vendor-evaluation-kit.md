# Vendor evaluation kit

How a platform or keyboard team reproduces, on its own hardware and in an afternoon, everything `docs/vendor-summary.md` claims: the published artifacts, their provenance, and the measured cost. No Rust toolchain is needed; the kit uses only published artifacts.

## Two versions, kept apart

- **The release being evaluated** is `0.1.1`: source commit `2c6b1537031414c1cca371ca6e5005152a8d415e`, tagged `v0.1.1` (bundle), `android-v0.1.1` and `swift-v0.1.1` (SDK workflows). Everything the kit downloads and measures belongs to that release, and the kit verifies it against that release's own `SHA256SUMS` and `provenance.json`.
- **The kit's source** is distributed separately, because the release tag `v0.1.1` predates the kit and does not contain `scripts/vendor/evaluate.sh`, this document, the remote consumer's benchmark harness, or the remote consumer's `0.1.1` package pin. The kit for release `0.1.1` is the immutable tag `vendor-kit-0.1.1`, created on the commit that merged the kit into `main` after the release. Later kit fixes for the same release, if any, get `vendor-kit-0.1.1-r2` and so on; the tags are never moved.

Get the kit source exactly like this (either form; the tarball is the same tree):

```bash
git clone --depth 1 --branch vendor-kit-0.1.1 https://github.com/Kurdi-Language/kurmanci
cd kurmanci
# or: https://github.com/Kurdi-Language/kurmanci/archive/refs/tags/vendor-kit-0.1.1.tar.gz
```

The commands below are run from that checkout. `evaluate.sh --version 0.1.1` names the release; the kit refuses to combine it with anything else (the iOS host must require and resolve the Swift package at exactly that version, the bundle's `VERSION` and provenance must say that version).

## What the kit is

1. **The kit source** at `vendor-kit-0.1.1`: the two consumer test hosts with their device benchmark harnesses, the driver, the contracts and the documentation.
2. **The published artifacts of release 0.1.1**, fetched and verified by the driver: the release bundle `kurmanci-ku-Latn-0.1.1.tar.gz` (packs, language model, C header, licences, attribution, `provenance.json`, `SHA256SUMS`) from GitHub release `v0.1.1`; the Android SDK `io.github.ferhatguneri:kurmanci-android:0.1.1` from Maven Central; the Apple XCFramework `KurmanciFFI-v0.1.1.xcframework.zip` from GitHub release `swift-v0.1.1` (the same binary the Swift package `Kurdi-Language/kurmanci-swift` tag `0.1.1` wraps).
3. **The driver** `scripts/vendor/evaluate.sh` with three commands: `fetch`, `android`, `ios`.

## Prerequisites

| For | Needed |
|---|---|
| `fetch` | `curl`, `tar`, `unzip`, `python3`, `shasum` or `sha256sum` |
| `android` | JDK 17, the Android SDK (platform 34, build-tools 34), `adb` on `PATH`, a device with USB debugging or an emulator visible to `adb devices` |
| `ios` | Xcode 26 or 27 with an iPhone simulator, or a real iPhone with Developer Mode on and an Apple Development team for signing |

## Step 1: fetch and verify (about a minute)

```bash
scripts/vendor/evaluate.sh fetch --version 0.1.1
```

What it checks, and fails closed on:

- the bundle tarball against the `.sha256` published next to it;
- the exact file set of the bundle: every path listed in the bundle's own `SHA256SUMS` present and matching, no regular file that is not listed, `SHA256SUMS` itself handled explicitly (its hash is the identity of the release and must equal the one quoted in the release notes);
- the release identity: the requested `--version`, the bundle's `VERSION` file and the `release_version` in `provenance.json` must agree;
- the AAR downloaded from Maven Central and the XCFramework downloaded from the GitHub release against the provenance records of the artifacts attached to the bundle, matched by platform and path (`android/kurmanci-android-0.1.1.aar`, `apple/KurmanciFFI-v0.1.1.xcframework.zip`), not merely by file name.

It prints the release kind (`production` or `evaluation`), the source commit, the engine version, the C ABI, the pack schema, and each pack's entry count. Everything lands under `dist/vendor-evaluation/`.

The bundle is deterministic: anyone with the Rust toolchain can rebuild it from the tagged commit and get byte-identical files (`docs/RELEASE_PROVENANCE.md`; CI does exactly that on every change). `provenance.json` carries the licensing block, the per-source redistribution determinations and the review provenance of every pack.

## Step 2: Android on your device (a few minutes)

```bash
adb devices                       # the device must be listed as "device"
scripts/vendor/evaluate.sh android --version 0.1.1 --pack reviewed
scripts/vendor/evaluate.sh android --version 0.1.1 --pack experimental-full
```

The Android consumer test host (`integration/android/android-consumer`) resolves the SDK from Maven Central only (`CONSUMER_MODE=public`), installs its instrumentation on the device, loads the chosen pack from the verified bundle, and runs `DeviceBenchmarkTest`: load time (median of 5 cold loads), resident memory after load and after the queries, p50/p95/max latency of `known`, `suggest`, `correct`, `complete` and `predict`, and a 300-round stability check that asserts identical results on every round. A run whose instrumentation fails writes no report.

## Step 3: iOS on a simulator or your iPhone (a few minutes)

```bash
# simulator (first available iPhone simulator)
scripts/vendor/evaluate.sh ios --version 0.1.1 --pack reviewed

# a real iPhone: destination plus automatic signing with your team
XCODEBUILD_EXTRA_ARGS="DEVELOPMENT_TEAM=<team id> -allowProvisioningUpdates" \
  scripts/vendor/evaluate.sh ios --version 0.1.1 --pack reviewed --destination "platform=iOS,id=<UDID>"
```

The remote iOS consumer (`integration/apple/ios-remote-consumer`) depends on the published Swift package at the exact version the kit tag pins (`0.1.1` at `vendor-kit-0.1.1`), so Xcode resolves the XCFramework itself and nothing is built from Rust. Before building, the driver checks that the project's package requirement and its `Package.resolved` are exactly the requested `--version` and refuses otherwise, so a release's pack is never measured through another SDK version. The run uses its own DerivedData and package cache under the evaluation directory. `DeviceBenchmarkTests` records the same measures as the Android harness. Simulator numbers characterise the host Mac, not a phone; use a real iPhone for cost figures.

## Reading the results

Reports are JSON files under `dist/vendor-evaluation/reports/` (`android-<model>-<timestamp>.json`, `ios-<model>-<timestamp>.json`), and each run prints a summary table starting with the pack's SHA-256, so a report is always tied to the exact pack it measured. Compare with the project's own measurements in `docs/evaluation/performance-baseline-2026-09-18.md` (iPhone 14 Pro, Samsung Galaxy SM-S948B, Android emulator; provenance sidecars next to each report). The resident-memory figures are those of the whole test-host process; the engine's own heap per pack is in `docs/memory-attribution.md`.

## What to try beyond the harness

- The consumer test hosts' ordinary test suites (`KurmanciConsumerTests` on iOS, the JVM and instrumentation tests on Android) show every API call with its expected results on the committed fixtures.
- With the Rust toolchain, the query CLI answers any word interactively against any pack in the bundle: `cargo run -p kurmanci-cli -- --pack dist/vendor-evaluation/kurmanci-ku-Latn-0.1.1/packs/reviewed/lexicon.bin interactive`.
- The contracts a keyboard is asked to honour are two JSON files: `data/keyboard/ku-Latn-orthography.json` and `data/keyboard/ku-Latn-keyboard-requirements.json` (`docs/ku-latn-keyboard.md`).

## Sending results back

Open an issue on the repository with the JSON reports attached, or keep them private; nothing in the kit reports anywhere. Results on device classes the project has not measured (low-end Android in particular) are the most useful.
