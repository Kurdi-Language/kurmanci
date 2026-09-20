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

`--project remote` runs the same harness in `integration/apple/ios-remote-consumer`, which
depends on the published `Kurdi-Language/kurmanci-swift` package at the version the project
pins, so no Rust toolchain is needed; this is what the vendor evaluation kit uses
(`scripts/vendor/evaluate.sh ios`, `docs/vendor-evaluation-kit.md`). The default local
iOS consumer host needs the local Swift package first (`scripts/apple/build-xcframework.sh`,
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
| Samsung `SM-S948B` (Qualcomm SM8850, 4 KB pages; Remote Test Lab device over Samsung's Remote Debug Bridge) | Android 16 (API 36), build BP4A.251205.006.S948BXXS1AZC7 | reviewed `485b9d70…d34508` | 10.2 | 150.8 MB | 4.4 / 53.5 / 45.2 / 122.8 / 17.2 | 300 rounds |
| Samsung `SM-S948B` (same device) | Android 16 (API 36) | experimental-full `65764b14…d04f7` | 102.5 | 186.2 MB | 2.9 / 192.8 / 203.1 / 618.2 / 16.9 | 300 rounds |
| Samsung `SM-A055F` (Galaxy A05 class: MediaTek MT6769, 4 GB, 4 KB pages; Remote Test Lab device over the Remote Debug Bridge; published 0.1.1 AAR from Maven Central) | Android 14 (API 34), build UP1A.231005.007.A055FXXS8CYC3 | reviewed `8ad8bbf8…e399b` (2,143 entries, 1.14 MB; release 0.1.1 bundle) | 32.5 | 76.8 MB | 14.2 / 178.2 / 164.7 / 528.3 / 78.4 | 300 rounds |
| Samsung `SM-A055F` (same device) | Android 14 (API 34) | experimental-full `6caf3b56…392a` (42,248 entries, 6.86 MB; release 0.1.1 bundle) | 314.0 | 119.0 MB | 14.5 / 902.4 / 999.1 / 2,393.8 / 79.6 | 300 rounds |

Measured 2026-09-18 with Xcode 27.0 over USB, phone unlocked, no other app in the
foreground. Across the twelve operation rows of the two iPhone reports the p95 / p50 ratio lies
between 1.01 (suggest, reviewed: 25.7 → 26.0 µs) and 1.17 (known_hit, reviewed: 0.500 →
0.583 µs); the per-operation p50, p95 and max values are in the report files. RSS after the
300 stability rounds remained within 0.2 MB of RSS after load (reviewed 42.16 → 42.22 MB,
experimental-full 74.20 → 74.38 MB).
The Samsung rows (2026-09-19; `android-samsung-SM-S948B-*.json` and the provenance sidecar
`android-samsung-SM-S948B-provenance-20260919.json`) are the first physical Android
measurements: a Samsung Remote Test Lab device reached through Samsung's Remote Debug Bridge,
the same JNI path from the instrumentation host as the emulator rows. Across their twelve
operation rows the p95 / p50 ratio lies between 1.03 (suggest and correct, experimental-full)
and 1.29 (known_miss, experimental-full: 2.9 → 3.7 µs); RSS after the 300 rounds is within
1.3 MB of RSS after load (150.8 → 152.1 MB, 186.2 → 187.4 MB). RSS is the whole
instrumentation process, not the engine. The Pixel rows are emulator rows (`simulator: true`
in the report), kept as reference: the arm64 system image on an arm64 host avoids cross-ISA
emulation, but its process baseline (about 140 MB of instrumentation host), JNI behaviour and
scheduling are not a phone's. On that 16 KB-page
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
The Galaxy A05 rows (2026-09-20; `android-samsung-SM-A055F-*.json` and the sidecar
`android-samsung-SM-A055F-provenance-20260920.json`) are the first low-end Android
measurements and the first taken from the published artifacts: the vendor evaluation kit
resolved `kurmanci-android:0.1.1` from Maven Central (`CONSUMER_MODE=public`) and took the
packs from the verified `kurmanci-ku-Latn-0.1.1` bundle, so the packs' identities are the
0.1.1 release's (reviewed 2,143 entries, experimental-full 42,248). Every reviewed-pack call
is under 1 ms at p50; p95 / p50 ratios are wider than the flagship's (1.16 to 2.01 on the
reviewed pack). The 300-round check asserts identical query results, not memory: on
experimental-full the whole-process RSS was 102.1 MB after the rounds against 119.0 MB after
load, so the after-load figure is the larger of two samples; the cause of the decrease cannot
be attributed from this benchmark. The S948B rows measured earlier pack bytes (2,144 and
42,249 entries) and an earlier AAR, so ratios between the two Samsung devices are approximate.
