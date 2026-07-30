# Integration Guide

## Embedding the Core Engine

The Kurmancî core engine is compiled into native libraries for target platforms:

- **iOS / macOS**: Static C library (`libkurmanci_engine.a`) wrapped by Swift package `KurmanciEngine`.
- **Android**: Shared C library (`libkurmanci_engine.so`) loaded via JNI inside Kotlin library.
- **Web Browsers**: Compiled to WebAssembly (`kurmanci_engine_bg.wasm`) with TypeScript bindings.

## Basic Swift Integration (iOS)

```swift
import KurmanciEngine

let engine = KurmanciEngine()
let suggestions = engine.suggest("rojb", limit: 3)

for suggestion in suggestions {
    print("Suggested: \(suggestion.text) [\(suggestion.kind)]")
}
```

## Basic Kotlin Integration (Android)

```kotlin
import com.kurmanci.engine.KurmanciEngine

val engine = KurmanciEngine.load()
val suggestions = engine.suggest("rojb", limit = 3)

suggestions.forEach {
    println("Suggested: ${it.text}")
}
```
