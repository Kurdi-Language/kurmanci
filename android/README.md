# Kurmancî Android SDK (`kurmanci-android`) — Milestone 5D

`kurmanci-android` is an idiomatic, zero-Rust-dependency Android SDK built on top of the stable Kurmancî C ABI (`kurmanci-ffi`) via a JNI bridge (`libkurmanci_jni.so`).

- **Architecture**: Kotlin SDK → JNI Bridge → Stable C ABI → Rust Engine.
- **Native ABI Support**: `arm64-v8a`, `armeabi-v7a`, `x86_64` (compiled with NDK `r26b`, `minSdk = 23`).
- **Zero Rust Setup**: Consuming Android apps do **not** require Rust, Cargo, or NDK build scripts.

---

## 1. Gradle Installation (Local Maven Repository)

During local development and CI testing, `scripts/android/build-aar.sh` publishes `org.kurmanci:kurmanci-android:0.1.0` directly to `dist/android/maven/`.

Add the local Maven repository and dependency to your consumer app's `settings.gradle.kts` / `build.gradle.kts`:

```kotlin
dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
        maven {
            name = "localDist"
            url = uri(rootDir.resolve("../../../dist/android/maven"))
        }
    }
}

dependencies {
    implementation("org.kurmanci:kurmanci-android:0.1.0")
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

## 3. Building from Source

To build the native shared libraries, assemble the `.aar`, and publish to `dist/android/maven`:

```bash
# 1. Build seed data pack
cargo run -p kurmanci-data-builder -- build-pack seed

# 2. Build AAR and local Maven publication
./scripts/android/build-aar.sh

# 3. Test clean consumer application
./scripts/android/test-consumers.sh
```
