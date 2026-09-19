# Kurmancî (ku-Latn) for platform keyboards: vendor summary

For the platform or keyboard engineering team of a vendor that already ships, or wants to ship, Kurmancî (Northern Kurdish, Latin script, BCP 47 `ku-Latn`) in a built-in keyboard. Ten minutes to read; everything it states is backed by a file in the repository `https://github.com/Kurdi-Language/kurmanci`, named where it matters.

## What this is, and what it is not

Kurmancî is an offline language engine plus language data packs, with a stable C ABI and Swift and Kotlin SDKs, that answers the questions a keyboard asks about a word: is it known, how should it be corrected, how does it complete, what word comes next. It is vendor-neutral and layout-neutral: it prescribes no keyboard layout and replaces no keyboard. Vendors keep their keyboard, their layout and their UI, and call the engine for the language.

It is not a keyboard app, not a cloud service, and not a neural or generative model. Results are deterministic for a given pack and input: lexical answers (known word, correction, completion) come from the pack's vocabulary, and next-word prediction uses deterministic n-gram statistics built from the recorded Kurmancî Wikipedia corpus. Which vocabulary tier is human-reviewed and which is not is stated precisely under "The data".

## What you can pull today

| Artifact | Coordinates | Version | Notes |
|---|---|---|---|
| Android SDK (AAR, Kotlin API over JNI) | `io.github.ferhatguneri:kurmanci-android` on Maven Central | 0.1.1 | `minSdk 23`, `arm64-v8a`, `armeabi-v7a`, `x86_64`; 16 KB page-size compliant; no other runtime dependency |
| Apple SDK (Swift package over an XCFramework) | `https://github.com/Kurdi-Language/kurmanci-swift`, tag `0.1.1` | 0.1.1 | iOS 14+ device and simulator, macOS 11+ (arm64 and x86_64); no Rust toolchain needed |
| Release bundle (packs, language model, C header, licences, provenance) | GitHub release `v0.1.1` of the repository | 0.1.1 | `kurmanci-ku-Latn-0.1.1.tar.gz` with `SHA256SUMS`; byte-identical when rebuilt from the tagged commit |
| C ABI header | `include/kurmanci.h` in the bundle | ABI 1.1 | any language with a C FFI can use the engine directly |

Engine version 0.1.1, C ABI 1.1, pack schema 4, language-model schema 1, language tag `ku-Latn`. The C ABI rule (`docs/integration.md`): a client requires `kmr_abi_version_major() == 1` and `kmr_abi_version_minor() >=` the minor of the header it was compiled against; minor versions only add symbols. A client built against ABI 1.1 therefore needs a library of ABI 1.1 or later, and a library of ABI 1.x serves every client built against 1.0 up to 1.x.

## The language surface a keyboard is asked to support

Two human-reviewed contracts in the repository define it (`data/keyboard/ku-Latn-orthography.json`, `data/keyboard/ku-Latn-keyboard-requirements.json`; readable summary in `docs/ku-latn-keyboard.md`):

- The alphabet is exactly 31 letters: `a b c ç d e ê f g h i î j k l m n o p q r s ş t u û v w x y z`, in lower and upper case. Reference: Bedir Khan & Lescot, *Grammaire kurde (dialecte kurmandji)*, Paris 1970, Part I §2, p. 3.
- `ç ê î ş û` are distinct letters, not decorated `c e i s u`. Storage, comparison and casing must keep the distinction; the engine's spell correction may propose the change `biji` to `bijî` as a correction, never as normalization.
- Casing is Unicode default casing (The Unicode Standard, Version 18.0.0): `i` pairs with `I` only. No dotted or dotless i tailoring of any other locale applies to `ku-Latn`.
- Precomposed or decomposed output is accepted; NFC is preferred. All 31 letters must be typeable in both cases; how `ç ê î ş û` are reached (long press, dedicated keys, a layer) is the vendor's choice.
- Digits, punctuation and symbol layers follow the host. Tokens containing a hyphen or an apostrophe are queried as given first, then as their punctuation-aware splits, with results deduplicated; such words are currently held for linguist review and are not in the default pack.

The contracts prescribe no layout and ask no vendor to replace an existing keyboard; they state what the language needs from whichever keyboard hosts it.

## The API, in one screen

Swift:

```swift
import Kurmanci
let engine = try KurmanciEngine(packURL: packURL)          // or packData: Data
try engine.isKnownWord("welat")                              // Bool
try engine.suggest("spaz", limit: 5)                         // [Suggestion]  text, kind, editCost
try engine.correct("peşeroj", limit: 5)                      // [Suggestion]
try engine.complete("kurd", limit: 5)                        // [Suggestion]
try engine.predictNext(context: ["navê", "te"], limit: 5)    // [Prediction]  text, count, probabilityMillionths, source
```

Kotlin:

```kotlin
import org.kurmanci.KurmanciEngine
KurmanciEngine.openFile(path).use { engine ->            // or open(bytes)
    engine.isKnownWord("welat")                           // Boolean
    engine.suggest("spaz")                                // SuggestionResult
    engine.correct("peşeroj")
    engine.complete("kurd")
    engine.predictNextWord(listOf("navê", "te"))          // PredictionResult
}
```

Both wrappers delegate everything to the engine; nothing linguistic is reimplemented in Swift or Kotlin. The C ABI has the same five operations plus version and pack probing (`docs/integration.md`).

Contract points that matter for a keyboard integration:

- **Normalization is inside the engine.** Control characters, U+200B and U+FEFF are removed, then NFC and lower-casing. `Baş`, `baş` and the decomposed form query the same word. Hosts tokenize on whitespace and pass tokens through unchanged.
- **A loaded engine is immutable and thread-safe.** Any number of threads may query one handle concurrently and get exactly the single-threaded answers. There are no caches and no background work.
- **Loading is all-or-nothing** and never panics on any bytes. A pack that is not for this engine build or this language fails with a named status before anything is read (`KMR_ERROR_UNSUPPORTED_PACK`, `KMR_ERROR_INCOMPATIBLE_LANGUAGE`, `KMR_ERROR_CHECKSUM`, and so on); `kmr_probe_pack_bytes` explains an unusable pack without loading it.
- **Results are deterministic** for a given pack and input, and `limit` is clamped to 50.

## The data

Three packs, in the release bundle, ordered by trust (`docs/RELEASE_PROVENANCE.md`):

| Pack | Content | Entries | Size | Next-word prediction |
|---|---|---|---|---|
| `seed` | hand-written seed lexicon only | 33 | 3 KB | no |
| `reviewed` | seed plus every external entry a human approved | 2,143 | 1.1 MB | yes |
| `experimental-full` | seed plus every mechanically valid imported entry (opt-in; never a default) | 42,248 | 6.9 MB | yes |

`seed ⊆ reviewed ⊆ experimental-full` is verified before every release. The pack to evaluate for shipping is `reviewed`. Its vocabulary has two tiers: the manually curated seed lexicon shipped with the repository (`data/reviewed/lexicon.jsonl`, 33 entries, Apache-2.0), and external entries that enter only through an explicit approval decision recorded per source with reviewer id and date (Hunspell entries through `data/review-decisions/`, the two Wikipedia-derived batches through their batch decision files). The seed tier is distinct from those Review Desk decisions. The tooling never invents an approval or linguistic judgment. Explicit project policy gates (the alphabet policy and the word-punctuation policy) may mechanically downgrade or block an exported review status while preserving the reviewer's original choice, reviewer, date and policy context as evidence; only entries that satisfy the authoritative policy and carry an eligible human approval can enter the reviewed/default vocabulary. `experimental-full` is opt-in and never a default: it adds every mechanically valid imported entry (`data/pack-policy.toml`: "manual seed plus mechanically valid imported entries"), which is evidence of breadth, not linguistic truth.

Next-word prediction uses deterministic n-gram statistics (14,206 unigrams, 44,820 bigrams, 55,140 trigrams over the pack vocabulary) built from the TRAIN partition of the recorded Kurmancî Wikipedia dump of 2026-08-01 (`data/language-model/kuwiki-20260801/`); no article text is shipped and nothing is learned at run time. It is not a neural or generative model.

Said plainly: the reviewed vocabulary is small today and grows only through human review (the Review Desk workflow merges reviewed batches of 1,000 entries). The reservoir behind it is mechanically valid imported dictionary material (the Hunspell import's ordinary review pool holds 41,258 forms); corpus evidence from the Wikipedia dump is a separate signal that is recorded where it was computed and used only to prioritise review, and it is not claimed for the reservoir as a whole. A vendor should judge the engine, the contracts, the cost and the provenance now, and the vocabulary as a growing series.

## Measured cost on real hardware

From `docs/evaluation/performance-baseline-2026-09-18.md` and its 2026-09-19 addendum; each report has a provenance sidecar naming the commit, device and pack hashes. Reviewed pack unless stated; p50 of 300 rounds.

| Measure | iPhone 14 Pro (iOS 26.7, Swift SDK) | Samsung Galaxy SM-S948B (Android 16, Kotlin SDK) |
|---|---|---|
| Load, median of 5 cold loads | 6.7 ms | 10.2 ms |
| Resident memory of the test-host process after load | 42.2 MB | 150.8 MB |
| known word | 0.5 µs | 4.4 µs |
| suggest (`rojbas`) | 25.7 µs | 53.5 µs |
| correct (`spaz`) | 19.5 µs | 45.2 µs |
| complete (`ro`) | 60.5 µs | 122.8 µs |
| predict next word (`ez`) | 2.4 µs | 17.2 µs |
| experimental-full: load / suggest / complete | 68.4 ms / 125.6 µs / 368.3 µs | 102.5 ms / 192.8 µs / 618.2 µs |

The resident-memory figures are whole test-host processes (an XCTest host on iOS, an instrumentation host on Android), not the engine alone; the engine's own heap, measured in-process on an Apple M4 under a counting allocator, is 2.4 MB for the reviewed pack and 14.2 MB for experimental-full (`docs/memory-attribution.md`). Memory is stable over 300 rounds of mixed queries on both devices. A low-end Android device has not yet been measured; the emulator and the flagship numbers above are the current evidence.

## Licensing and provenance

- Code (engine, SDKs, tooling): Apache-2.0.
- Data: the seed lexicon is Apache-2.0; the Hunspell-derived entries are CC BY-SA 4.0 (KurdishHunspell, commit-pinned, upstream licence preserved); the Wikipedia-derived entries and the language model are CC BY-SA 4.0 (Wikipedia contributors, Wîkîpediya). Attribution text for every pack ships in the bundle (`ATTRIBUTION`, `LICENSES/`).
- The project owner's stance (2026-09-19, recorded in `NOTICE`): the project is intended for unrestricted broad reuse including commercial use; project-owned code and data are under permissive terms; third-party materials remain subject to their recorded upstream licences, attribution requirements and any applicable ShareAlike obligations; the project adds no restriction.
- Redistribution determinations are recorded per source and per corpus as data and copied into the release provenance; the tooling makes no legal determination. A vendor's own counsel decides whether CC BY-SA data with attribution fits their product; the facts needed for that decision are in `provenance.json` of the bundle.
- Reproducibility: the bundle is byte-identical when rebuilt from the tagged commit in two clean checkouts (CI runs that on every change); `SHA256SUMS` is the bundle's identity.

## Platform requirements

- Android: `minSdk 23`; `arm64-v8a`, `armeabi-v7a`, `x86_64`; the native library is linked with 16 KB page alignment and RELRO (required for Android 15+ 64-bit devices and by Google Play from 2027); AGP 8.5 packaging; JDK 17 for the consumer build.
- Apple: iOS 14+ (device arm64; simulator arm64 and x86_64), macOS 11+ (arm64, x86_64), Swift Package Manager, Xcode 26 or 27; the XCFramework is about 30 MB zipped across all slices.
- Any C consumer: `include/kurmanci.h` and the platform library; `ffi/include/required_symbols.txt` lists the exported symbols.

## Known limits, stated up front

- Reviewed vocabulary of 2,143 words; growth by human review only.
- 145 words with a hyphen or an apostrophe are held for linguist review; the canonical apostrophe code point is undecided; neither is in the default pack.
- All review decisions so far carry one reviewer identity.
- No low-end Android measurement yet.
- Deferred by design until the data is broader: ranking changes, prediction smoothing, morphology, further corpora.

## How to evaluate it in an afternoon

`docs/vendor-evaluation-kit.md` describes the kit: the published release bundle and SDK artifacts with their checksums, the two consumer test hosts, and a script that verifies everything and runs the same device benchmarks that produced the numbers above on a device you plug in. The kit's own source is distributed at an immutable `vendor-kit-<version>` tag that is later than the release it evaluates (`vendor-kit-0.1.1` for release 0.1.1); the release tag `v0.1.1` itself does not contain the kit.

Contact: the repository owner, through the repository's issue tracker.
