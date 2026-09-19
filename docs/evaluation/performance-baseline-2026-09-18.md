# Performance baseline, 2026-09-18

An engineering baseline of the Kurmancî engine and its language packs as they stand after
PRs #67 to #71: what the packs cost in memory, how long they take to load and how long each
query takes, on a development Mac, on a real iPhone and on an Android emulator. It exists so
that a platform vendor can judge integration cost from measured numbers, and so that later
engine or data changes can be compared against a fixed point. It makes no linguistic claim
and changes nothing about ranking, prediction or the packs.

Every number below comes from a committed report; the report files are the evidence and this
document only summarises them. Two provenance records sit next to the M4 reports:
`performance-baseline/m4-provenance-20260918.json` (source commit, pack and report hashes,
machine, RAM, macOS, toolchain, command) and
`performance-baseline/release-bundle-dry-run-20260918.json` (the manual release-bundle run
cited at the end). Nothing was rerun to improve a figure.

## Artifacts measured

| Artifact | Identity |
|---|---|
| Engine | `kurmanci-engine` 0.1.0, C ABI 1.1, pack schema 4, language-model schema 1, language tag `ku-Latn` (as recorded in `release-bundle-dry-run-20260918.json`) |
| seed pack | `lexicon.bin` sha256 `4e186130f1d0…`, 33 entries, 3,109 bytes, `model_profile = none` |
| reviewed pack | `lexicon.bin` sha256 `485b9d70d25b6708af8e0b3f63f924cab23c0eab8110a5e9a3b6b25e88d34508`, 2,144 entries, 1,138,440 bytes, `model_profile = prediction` (19,319 bigram and 27,929 trigram predictions) |
| experimental-full pack | `lexicon.bin` sha256 `65764b14928a0fde3eff31c085a37322d8d5ee6815279d81ae9e171e950d04f7`, 42,249 entries, 6,860,008 bytes, `model_profile = prediction` |
| language model | `kuwiki-20260801`, `language-model-v1`, vocabulary fingerprint `62963ced…`, 14,206 unigrams, 44,820 bigrams, 55,140 trigrams; redistribution determination `pending-review` |

The reviewed pack is the trustworthy, human-reviewed vocabulary; experimental-full is the
evidence reservoir (see `docs/pack-policy.md`). Both carry the same language model, cut to
their own vocabulary.

## Environments

| Label | What runs | Path measured | Report |
|---|---|---|---|
| M4 | `kurmanci-bench` (release build, Rust 1.85.0) on an Apple M4 (`Mac16,12`), 24 GiB, macOS 27.0, source commit `5ed81d9`, under the bench's counting global allocator | Rust engine called directly, in-process; the counting allocator adds benchmark overhead to every allocation | `performance-baseline/m4-release-<pack>-20260918.json`; the reports themselves record only pack path, bytes, entries, build profile and `host: macos aarch64`, so commit, hashes, machine, toolchain and command are in `m4-provenance-20260918.json` |
| iPhone | `DeviceBenchmarkTests` in the iOS consumer test host on an iPhone 14 Pro (`iPhone15,2`), iOS 26.7, Xcode 27.0, USB, phone unlocked | Swift SDK → C FFI → release-built engine inside the XCFramework, from the XCTest host (Debug app) | `device-benchmarks/ios-iPhone15-2-<pack>-…json` |
| Emulator | `DeviceBenchmarkTest` in the Android consumer on the Pixel image `sdk_gphone16k_arm64`, Android 17 (API 37), 16 KB pages, arm64 image on the same M4 host | Kotlin SDK → JNI → release-built engine in the AAR, from the instrumentation host | `device-benchmarks/android-emulator-sdk_gphone16k_arm64-<pack>-…json` |

The three paths are not the same measurement. M4 numbers measure the engine in-process: the
memory figure is the counting allocator's live-heap delta, and the structure attribution
model explains approximately 100 % of it; iPhone and emulator numbers are end-to-end SDK-path
timings and whole-process memory. The emulator rows are reference data: the arm64 image on an arm64 host avoids
cross-ISA emulation, but its process baseline, JNI behaviour and scheduling are not a
phone's. At the 2026-09-18 baseline, no physical Android device had been measured yet; see
the 2026-09-19 addendum below. The 16 KB-page alignment of the
Android library (#71) landed after the emulator rows were recorded and does not change them.

## Memory

Engine heap on M4 is the measured engine live-heap delta of the counting allocator after load
(`engine_heap_bytes`). `KurmanciEngine::memory_attribution()` is an allocation model computed
from container capacities and element sizes; its coverage of 1.00 means the model explains
approximately 100 % of the measured live heap, not that it is allocator-level ownership
attribution. RSS is the whole bench process.

| Pack | Engine heap | Bytes / entry | Heap / pack | RSS after load | Peak RSS after 4 reloads | Allocations at load |
|---|---|---|---|---|---|---|
| seed | 0.01 MB | 236 | – | 2.6 MB | 3.2 MB | 468 |
| reviewed | 2.43 MB | 1,132 | 2.13× | 7.5 MB | 10.7 MB | 55,753 |
| experimental-full | 14.21 MB | 336 | 2.07× | 37.0 MB | 61.8 MB | 584,371 |

Where the attribution model places the heap (largest structures):

| Pack | Largest | Second | Third | Fourth | Fifth |
|---|---|---|---|---|---|
| reviewed | trigram table 0.67 MB | trigram lists 0.67 MB | bigram lists 0.46 MB | lexicon records 0.15 MB | bigram table 0.14 MB |
| experimental-full | lexicon records 3.04 MB | trie node arrays 1.98 MB | query-index trie 1.83 MB | trigram table 1.34 MB | trigram lists 1.32 MB |

On the reviewed pack the language-model tables account for most of the modelled heap (about
2.0 of 2.43 MB); on experimental-full the lexicon records (3.04 MB), the main trie (1.98 MB)
and the query-index trie (1.83 MB) together account for the largest share, followed by the
trigram tables. Bytes per entry is lower on the larger pack in these two measurements.

Whole-process resident memory on the devices (not engine-only; no empty-host run is
committed, so the host's own share is not established):

| Pack | iPhone RSS after load | iPhone after 300 rounds | Emulator RSS after load | Emulator after 300 rounds |
|---|---|---|---|---|
| reviewed | 42.2 MB | 42.2 MB | 142.7 MB | 145.1 MB |
| experimental-full | 74.2 MB | 74.4 MB | 175.8 MB | 178.2 MB |

The difference between the two pack runs on the iPhone (about 32 MB) is the observed
incremental whole-process RSS between the runs: incremental engine and data state together
with allocator, process and runtime effects, not a per-component attribution. On M4 the
benchmark ended with a 2,409-byte net live-heap delta across its whole query section on both
packs (`heap_growth_during_queries_bytes`, which includes benchmark and report bookkeeping);
that is a net figure for the section, not a per-query measurement.

## Load time (median of 5 cold loads)

| Pack | M4 (in-process) | iPhone (SDK path) | Emulator (JNI path) |
|---|---|---|---|
| seed | 0.1 ms | – | – |
| reviewed | 9.9 ms | 6.7 ms | 23.7 ms |
| experimental-full | 108.5 ms | 68.4 ms | 118.7 ms |

The counting allocator adds benchmark overhead on every load allocation (55,753 and
584,371 respectively), and the three environments differ in CPU, OS, harness and integration
path, so load times are comparable within a column, not across columns.

## Query latency, p50 in microseconds

Operations with the same input on all three paths (M4: 500 iterations; devices: 200):

| Operation (input) | Pack | M4 | iPhone | Emulator |
|---|---|---|---|---|
| known word (welat) | reviewed | 0.5 | 0.5 | 2.4 |
| known word (welat) | experimental-full | 0.5 | 0.5 | 2.3 |
| suggest (rojbas) | reviewed | 34.4 | 25.7 | 45.6 |
| suggest (rojbas) | experimental-full | 173.9 | 125.6 | 188.2 |
| correct (spaz) | reviewed | 25.0 | 19.5 | 37.8 |
| correct (spaz) | experimental-full | 177.6 | 121.8 | 193.3 |
| complete (ro) | reviewed | 100.9 | 60.5 | 115.5 |
| complete (ro) | experimental-full | 585.8 | 368.3 | 601.0 |
| predict next word (ez) | reviewed | 0.7 | 2.4 | 14.5 |
| predict next word (ez) | experimental-full | 0.8 | 2.5 | 14.6 |

Further M4 operations (p50 / p99 µs):

| Operation (input) | reviewed | experimental-full |
|---|---|---|
| suggest, exact hit (welat) | 56.6 / 62.2 | 334.2 / 444.7 |
| complete, longer prefix (rojb) | 30.9 / 34.0 | 157.4 / 191.7 |
| predict, trigram context (ez ê) | 1.2 / 1.3 | 1.3 / 1.4 |
| predict, backoff (xyzqwv ez) | 1.3 / 1.4 | 1.4 / 1.5 |
| predict, no context match | 1.2 / 1.3 | 1.3 / 1.5 |

Tail behaviour on the iPhone: across the twelve operation rows of the two reports the
p95 / p50 ratio lies between 1.01 and 1.17, the largest for the sub-microsecond known-word
lookup (0.500 → 0.583 µs). On the emulator the ratio reaches 3.2 for the same lookup and 2.5
for suggest on the reviewed pack; the emulator's scheduling is part of that. Every device run
was stable over 300 repeated rounds (identical results each round; the harness's only
assertion).

## What the numbers say

- **Individual calls were sub-millisecond on the measured iPhone 14 Pro.** At p50, every
  reviewed-pack call was under 0.07 ms and every experimental-full call under 0.4 ms;
  next-word prediction was 2.4 to 2.5 µs on both packs. How an integrating keyboard schedules
  these calls relative to its UI thread is the platform's decision: no low-end device and no
  production keyboard-extension workload has been measured.
- **Between the two measured packs, the candidate-generating operations grew and the lookups
  did not.** From 2,144 to 42,249 entries on the iPhone: suggest 4.9×, correct 6.2×, the
  two-letter completion 6.1×; known-word lookup stayed at 0.5 µs and prediction at 2.4 to
  2.5 µs. The harness does not report candidate counts, so the growth is recorded as measured,
  not attributed to a cause.
- **Memory.** On the reviewed pack the language-model tables are most of the modelled heap;
  on experimental-full the lexicon records, the main trie and the query-index trie together
  are. Measured engine live-heap delta: 2.43 MB (reviewed, load 9.9 ms in-process, 6.7 ms on
  the phone) and 14.21 MB (experimental-full, 108.5 ms in-process, 68.4 ms on the phone).
  Experimental-full is the evidence reservoir, not the default.
- **No evidence of unbounded growth was observed in these bounded runs.** The M4 benchmark
  ended with a 2,409-byte net live-heap delta; iPhone whole-process RSS changed by +0.07 MB
  (reviewed) and +0.18 MB (experimental-full) between the samples before and after 300 rounds,
  the emulator's by +2.4 MB on both packs. The device harness asserts result stability only
  and samples RSS twice; these runs do not establish leak-freedom.
- **Release state.** A manual release-bundle dry run on main `5ed81d9`, recorded in
  `performance-baseline/release-bundle-dry-run-20260918.json`, built with `--allow-dirty`
  and the locally built XCFramework and 16 KB-aligned AAR attached, verified 44 files against
  `SHA256SUMS` (43 entries, sha256 `3e27f86e…`) with the pack and language-model identities
  above, and is labelled an evaluation release because the language model's redistribution
  determination is `pending-review` and `worktree_dirty = true`. CI's clean-checkout
  determinism job verifies a different bundle, without the optional platform artifacts. The
  licensing determination is a human decision outside this document.

## Addendum, 2026-09-19: first physical Android device

A Samsung `SM-S948B` (Qualcomm SM8850, Android 16, 4 KB pages, 12 GB class RAM), reached
through Samsung's Remote Test Lab and Remote Debug Bridge, ran the same harness on the same
packs (reports and a provenance sidecar under `device-benchmarks/`). JNI path from the
instrumentation host, like the emulator rows; RSS is the whole process.

| Operation (input) | Pack | iPhone 14 Pro | Samsung SM-S948B | Emulator |
|---|---|---|---|---|
| load, median ms | reviewed | 6.7 | 10.2 | 23.7 |
| load, median ms | experimental-full | 68.4 | 102.5 | 118.7 |
| known word (welat), p50 µs | reviewed | 0.5 | 4.4 | 2.4 |
| suggest (rojbas), p50 µs | reviewed | 25.7 | 53.5 | 45.6 |
| correct (spaz), p50 µs | reviewed | 19.5 | 45.2 | 37.8 |
| complete (ro), p50 µs | reviewed | 60.5 | 122.8 | 115.5 |
| predict (ez), p50 µs | reviewed | 2.4 | 17.2 | 14.5 |
| suggest (rojbas), p50 µs | experimental-full | 125.6 | 192.8 | 188.2 |
| correct (spaz), p50 µs | experimental-full | 121.8 | 203.1 | 193.3 |
| complete (ro), p50 µs | experimental-full | 368.3 | 618.2 | 601.0 |
| predict (ez), p50 µs | experimental-full | 2.5 | 16.9 | 14.6 |
| RSS after load, whole process | reviewed / experimental-full | 42.2 / 74.2 MB | 150.8 / 186.2 MB | 142.7 / 175.8 MB |

Every Samsung call was sub-millisecond at p50 on both packs; the p95 / p50 ratio across its
twelve operation rows lies between 1.03 and 1.29, and RSS after 300 rounds stayed within
1.3 MB of RSS after load. The Samsung and emulator columns are the same JNI path and land
close together; the iPhone column is the Swift SDK path. The gap "no physical Android device"
in the section above is closed; "no low-end device" remains.

## Reproduction

```bash
# M4 in-process bench (release build, counting allocator), one JSON per pack
cargo run --release -p kurmanci-bench -- data/build/packs/reviewed/lexicon.bin --json

# iPhone (needs the local Swift package and a signing team; see docs/device-benchmark.md)
XCODEBUILD_EXTRA_ARGS="DEVELOPMENT_TEAM=<team> -allowProvisioningUpdates" \
  scripts/apple/device-benchmark.sh --pack data/build/packs/reviewed/lexicon.bin --destination "platform=iOS,id=<UDID>"

# Android emulator or device (needs the packaged AAR: scripts/android/build-aar.sh)
scripts/android/device-benchmark.sh --pack data/build/packs/reviewed/lexicon.bin
```

The bench operation set is fixed by `bench/src/main.rs`; the device operation set by the two
`DeviceBenchmark` tests. Their overlap (known, suggest `rojbas`, correct `spaz`, complete
`ro`, predict `ez`) is what the cross-environment table uses.

## Gaps

- Physical Android: one device so far (Samsung `SM-S948B`, see the addendum); the emulator
  rows remain as reference.
- No empty-host measurement on the phone, so device RSS cannot be split between host and
  engine; the M4 attribution is the engine-only reference.
- No low-end device: the iPhone 14 Pro and the emulator on an M4 are both fast hosts; a
  budget Android phone would be the useful next point.
- Bench and device operation sets overlap on five operations; the remaining bench operations
  have M4 numbers only.
- The M4 reports carry no commit, machine or toolchain metadata of their own; the sidecar
  records it, and a future bench version should embed it.
