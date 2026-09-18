# Device benchmark harness

A small internal harness for measuring the engine on real phones through the two consumer
test hosts. It is an integration test, not a consumer keyboard, and it gates nothing on
timing: it records numbers and asserts only that repeated queries return identical results.

## What is recorded

For the pack under test (`benchmark_pack.bin` in the test bundle):

| Field | Meaning |
|---|---|
| `load_ms_median/min/max` | 5 cold loads of the same pack bytes |
| `rss_after_load_bytes` | resident memory after one engine is loaded (`mach_task_basic_info` on iOS, `/proc/self/statm` on Android) |
| `operations[]` | `known_hit` (welat), `known_miss` (xyzqwv), `suggest` (rojbas), `correct` (spaz), `complete` (ro), `predict` (ez): p50 / p95 / max microseconds over 200 calls and the result count |
| `stability_rounds`, `stable` | 300 rounds of the whole operation set compared with the first round |
| `rss_after_queries_bytes` | resident memory after the stability rounds |
| `pack_sha256`, `pack_bytes`, `entry_count`, `pack_format_version` | the exact pack that was measured |
| `device_model`, `os_version`, `abi`, `simulator` | where it ran |

The report is one JSON line prefixed `KURMANCI_DEVICE_BENCHMARK` in the test output (iOS:
xcodebuild log and an XCTest attachment; Android: logcat tag `KurmanciDeviceBenchmark` and
`kurmanci-device-benchmark.json` in the app's external files directory).

## Running

The committed `benchmark_pack.bin` (iOS: `integration/apple/fixtures/`, Android:
`integration/android/android-consumer/app/src/androidTest/assets/`) is a tiny placeholder so
the harness runs in CI on the simulator and the emulator as a smoke test. To measure a real
pack, the scripts swap it in for the run and restore the placeholder afterwards:

```bash
# iOS simulator (default) or a real iPhone (destination + signing)
scripts/apple/device-benchmark.sh --pack data/build/packs/reviewed/lexicon.bin
XCODEBUILD_EXTRA_ARGS="DEVELOPMENT_TEAM=<team id> -allowProvisioningUpdates" \
  scripts/apple/device-benchmark.sh --pack data/build/packs/reviewed/lexicon.bin --destination "platform=iOS,id=<UDID>"

# Android device or emulator visible to adb (needs the packaged AAR: scripts/android/build-aar.sh)
scripts/android/device-benchmark.sh --pack data/build/packs/reviewed/lexicon.bin
```

The iOS consumer host needs the local Swift package first (`scripts/apple/build-xcframework.sh`,
`verify-xcframework.sh`, `create-release-archive.sh`, `generate-release-package.sh`, as CI runs them).
It targets iOS 15 and uses the scene-based life cycle, which the iOS 26+ SDKs require; it has
been verified under Xcode 27.0 (simulator and a real iPhone) and under the CI runner's Xcode.
On a real iPhone the `XCODEBUILD_EXTRA_ARGS` signing
flags above are required; Developer Mode must be enabled on the phone.

Reports land in `dist/device-benchmarks/<platform>-<model>-<timestamp>.json` and a summary
table is printed, starting with the pack's SHA-256. A run whose XCTest or instrumentation
fails (the stability assertion included) exits non-zero and writes no report; the Apple
wrapper keeps the full xcodebuild log as `failed-xcodebuild-<timestamp>.log` in the output
directory. `scripts/apple/test-device-benchmark-wrapper.sh` checks that contract with a fake
xcodebuild and runs in CI. Measure `reviewed` and `experimental-full` on each device; the M4
release-mode numbers in `docs/memory-attribution.md` are the reference point.

## Results

Real-device reports are committed under `docs/evaluation/device-benchmarks/` (one JSON per
run, named `<platform>-<model>-<pack>-<timestamp>.json`) and summarised here, one row per
device and pack. The host app is the Debug consumer build and every operation is invoked
through the Swift SDK and the C FFI from the XCTest host, so the latencies are end-to-end
SDK-path timings with a release-built engine (the XCFramework carries the release static
library), not isolated engine timings. `RSS after load` is the resident size of the whole
test-host process (UIKit app plus XCTest runner), not of the engine alone; no empty-host run is
committed, so the host's own share is not established here. The difference between the two
pack runs on the iPhone (42.2 MB → 74.2 MB, about 32 MB) is the observed incremental
whole-process RSS between the two runs: it includes the incremental engine and data state
together with allocator, process and runtime effects, and is not an attribution to individual
components. The M4 numbers in `docs/memory-attribution.md` measure the engine heap in
isolation and are the reference for component attribution.

| Device | OS | Pack | Load ms (median of 5) | RSS after load | known / suggest / correct / complete / predict p50 µs | Stable |
|---|---|---|---|---|---|---|
| iPhone 14 Pro (`iPhone15,2`) | iOS 26.7 (23H24) | reviewed `485b9d70…d34508` (2,144 entries, 1.14 MB) | 6.7 | 42.2 MB | 0.5 / 25.7 / 19.5 / 60.5 / 2.4 | 300 rounds |
| iPhone 14 Pro (`iPhone15,2`) | iOS 26.7 (23H24) | experimental-full `65764b14…d04f7` (42,249 entries, 6.86 MB) | 68.4 | 74.2 MB | 0.5 / 125.6 / 121.8 / 368.3 / 2.5 | 300 rounds |
| Android emulator, Pixel image `sdk_gphone16k_arm64` (16 KB pages) on an Apple M-series host | Android 17 (API 37) | reviewed `485b9d70…d34508` | 23.7 | 142.7 MB | 2.4 / 45.6 / 37.8 / 115.5 / 14.5 | 300 rounds |
| Android emulator, Pixel image `sdk_gphone16k_arm64` (16 KB pages) on an Apple M-series host | Android 17 (API 37) | experimental-full `65764b14…d04f7` | 118.7 | 175.8 MB | 2.3 / 188.2 / 193.3 / 601.0 / 14.6 | 300 rounds |

Measured 2026-09-18 with Xcode 27.0 over USB, phone unlocked, no other app in the
foreground. Across the twelve operation rows of the two iPhone reports the p95 / p50 ratio lies
between 1.01 (suggest, reviewed: 25.7 → 26.0 µs) and 1.17 (known_hit, reviewed: 0.500 →
0.583 µs); the per-operation p50, p95 and max values are in the report files. RSS after the
300 stability rounds remained within 0.2 MB of RSS after load (reviewed 42.16 → 42.22 MB,
experimental-full 74.20 → 74.38 MB).
The Android rows are emulator rows (`simulator: true` in the report), recorded as a reference
until a physical Android or Samsung device is measured: the arm64 system image on an arm64
host avoids cross-ISA emulation, but the rows remain emulator reference data whose process
baseline (about 140 MB of instrumentation host), JNI behaviour and scheduling are not
representative of a physical phone. On that 16 KB-page
image the system logged `16kB AppCompat: Library 'libkurmanci_jni.so' is not
PAGE(16384)-aligned - falling back to extraction from apk` at the time of these rows: the AAR's
native library was then linked with 4 KB segment alignment and ran through the platform's
compatibility path. The libraries are now linked with `-z max-page-size=16384` and
`-z common-page-size=16384` (`.cargo/config.toml`), so every LOAD segment is 16 KB-aligned and
the GNU_RELRO region ends on a 16 KB boundary with RELRO enabled; `scripts/android/build-aar.sh`
verifies both invariants on every staged library and on the libraries inside the packaged AAR
(`scripts/android/verify-elf-page-alignment.sh`). The other half is the
app's packaging: with the Android Gradle Plugin 8.2.2 the consumer test app stored the
uncompressed library at a 4 KB offset inside the APK, so the platform still extracted it
before loading; the consumer test app now builds with AGP 8.5.2 on Gradle 8.7, its APK passes
`zipalign -c -P 16 4`, and a rerun on the same image loads the library directly from the APK
(`nativeloader: Load .../base.apk!/lib/arm64-v8a/libkurmanci_jni.so ... ok`) with no
`16kB AppCompat` message. A consuming app needs the same (AGP 8.5.1 or newer, or
`zipalign -P 16`). The rows above are kept as recorded.
