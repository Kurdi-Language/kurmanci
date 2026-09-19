# Kurmancî Android SDK (`kurmanci-android`)

`kurmanci-android` is an idiomatic, zero-Rust-dependency Android SDK built on top of the stable Kurmancî C ABI (`kurmanci-ffi`) via a JNI bridge (`libkurmanci_jni.so`).

- **Architecture**: Kotlin SDK → JNI Bridge → Stable C ABI → Rust Engine.
- **Native ABI Support**: `arm64-v8a`, `armeabi-v7a`, `x86_64` (compiled with NDK `r26b`, `minSdk = 23`).
- **16 KB page size**: every `libkurmanci_jni.so` is linked with `-z max-page-size=16384` and `-z common-page-size=16384` (`.cargo/config.toml`; both are needed with NDK r27 or lower), so every LOAD segment is 16 KB-aligned and the GNU_RELRO region ends on a 16 KB boundary, with RELRO kept enabled. Apps targeting Android 15 (API 35) or newer must support 16 KB page sizes on 64-bit devices, and from 1 February 2027 Google Play no longer accepts non-compliant updates. `scripts/android/verify-elf-page-alignment.sh` checks both invariants (LOAD alignment, RELRO presence and end alignment) on every staged library and on the exact bytes inside the packaged AAR, and `scripts/android/build-aar.sh` fails closed on either; `scripts/android/test-verify-elf-page-alignment.sh` is its fixture-based regression test. The other half of the requirement is the app's packaging: build the consuming app with the Android Gradle Plugin 8.5.1 or newer (or run `zipalign -P 16`), so the uncompressed library sits at a 16 KB-aligned offset inside the APK and the platform maps it directly; the project's consumer test app uses AGP 8.5.2 on Gradle 8.7 and `scripts/android/verify-apk-16k-alignment.sh` (`zipalign -c -P 16 4`) checks its APK in `test-consumers.sh` and the clean-room verification.
- **Zero Rust Setup**: Consuming Android apps do **not** require Rust, Cargo, or NDK build scripts.

---

## 1. Gradle Installation (Maven Central)

Add `mavenCentral()` and the dependency to your application's `settings.gradle.kts` / `build.gradle.kts`:

```kotlin
dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

dependencies {
    implementation("io.github.ferhatguneri:kurmanci-android:0.1.1")
}
```

---

## 2. Basic Kotlin Usage

```kotlin
import org.kurmanci.KurmanciEngine

// 1. Open engine from ByteArray (e.g. from assets or network)
val packBytes: ByteArray = assets.open("lexicon.bin").readBytes()

KurmanciEngine.open(packBytes).use { engine ->
    // 2. Query Pack Metadata
    val info = engine.packInfo
    println("Loaded pack tag=${info.languageTag}, entries=${info.entryCount}")

    // 3. Known Word Lookup
    val isKnown = engine.isKnownWord("welat")
    println("Is 'welat' known? $isKnown")

    // 4. Autocorrection & Suggestions
    val suggestions = engine.suggest("spaz", maxCandidates = 5)
    suggestions.candidates.forEach { candidate ->
        println("Suggestion: ${candidate.text} (cost=${candidate.editCost})")
    }

    // 5. Prefix Completion
    val completions = engine.complete("roj", maxCandidates = 5)
    completions.candidates.forEach { candidate ->
        println("Completion: ${candidate.text}")
    }

    // 6. Next-Word Prediction
    val predictions = engine.predictNextWord(listOf("ez"), maxCandidates = 5)
    predictions.candidates.forEach { pred ->
        println("Predicted next word: ${pred.text} (count=${pred.count}, source=${pred.source})")
    }
}
```

---

## 3. Building & Testing for Contributors

### Clean-room verification (what a consumer actually needs)

CI proves the consumer path in two separate jobs. `Android AAR + Emulator Consumer` builds
the AAR with Rust and cargo-ndk and uploads it; `Android Consumer Clean-Room (Rust-Free)`
runs on a runner whose PATH has been stripped of Rust, installs no NDK, downloads the AAR
into the local Maven layout and runs `scripts/android/verify-clean-room-consumer.sh`, which:

1. fails if `cargo`, `rustc` or `cargo-ndk` is reachable (set `ALLOW_RUST_ON_PATH=1` only for
   local developer runs; the tools are never invoked either way);
2. requires the packaged `kurmanci-android-<version>.aar` and `.pom` under
   `dist/android/maven/` and checks that the AAR carries `libkurmanci_jni.so` for
   `arm64-v8a`, `armeabi-v7a` and `x86_64`, recording its SHA-256;
3. builds the standalone consumer in `integration/android/android-consumer` with
   `CONSUMER_MODE=local` and runs its JVM unit tests;
4. runs the instrumentation suite on the emulator, including
   `testCleanRoomContractKnownCorrectCompletePredict`: load pack → `isKnownWord` →
   `correct` → `complete` → `predictNextWord` through the public Kotlin API only.

Locally, after `scripts/android/build-aar.sh`, run
`ALLOW_RUST_ON_PATH=1 scripts/android/verify-clean-room-consumer.sh` (instrumentation runs
when an emulator or device is connected).

Contributors can build native shared libraries, assemble the AAR, and publish to local `dist/android/maven`:

```bash
# 1. Build seed data pack
cargo run -p kurmanci-data-builder -- build-pack seed

# 2. Build AAR and publish locally
./scripts/android/build-aar.sh

# 3. Test clean consumer application against local Maven
./scripts/android/test-consumers.sh
```
