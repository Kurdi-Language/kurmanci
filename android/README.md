# Kurmancî Android SDK (`kurmanci-android`)

`kurmanci-android` is an idiomatic, zero-Rust-dependency Android SDK built on top of the stable Kurmancî C ABI (`kurmanci-ffi`) via a JNI bridge (`libkurmanci_jni.so`).

- **Architecture**: Kotlin SDK → JNI Bridge → Stable C ABI → Rust Engine.
- **Native ABI Support**: `arm64-v8a`, `armeabi-v7a`, `x86_64` (compiled with NDK `r26b`, `minSdk = 23`).
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
    // Pending 0.1.0 release on Maven Central
    implementation("io.github.ferhatguneri:kurmanci-android:0.1.0")
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

Contributors can build native shared libraries, assemble the AAR, and publish to local `dist/android/maven`:

```bash
# 1. Build seed data pack
cargo run -p kurmanci-data-builder -- build-pack seed

# 2. Build AAR and publish locally
./scripts/android/build-aar.sh

# 3. Test clean consumer application against local Maven
./scripts/android/test-consumers.sh
```
