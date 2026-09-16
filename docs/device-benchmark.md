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

Reports land in `dist/device-benchmarks/<platform>-<model>-<timestamp>.json` and a summary
table is printed, starting with the pack's SHA-256. A run whose XCTest or instrumentation
fails (the stability assertion included) exits non-zero and writes no report; the Apple
wrapper keeps the full xcodebuild log as `failed-xcodebuild-<timestamp>.log` in the output
directory. `scripts/apple/test-device-benchmark-wrapper.sh` checks that contract with a fake
xcodebuild and runs in CI. Measure `reviewed` and `experimental-full` on each device; the M4
release-mode numbers in `docs/memory-attribution.md` are the reference point.

## Results

To be filled from real devices (iPhone; Android/Samsung when available). Keep the JSON files
alongside a row per device here:

| Device | OS | Pack | Load ms | RSS after load | known / suggest / correct / complete / predict p50 µs | Stable |
|---|---|---|---|---|---|---|
| _pending_ | | | | | | |
