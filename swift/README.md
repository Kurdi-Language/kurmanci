# Kurmancî Swift SDK Foundation (Milestone 5C.1)

`Kurmanci` is an idiomatic Swift wrapper package built on top of the stable Kurmancî C ABI (`kurmanci-ffi`).

> **Scope Note**: Milestone **5C.1** provides the repository-local Swift wrapper foundation validated on macOS and Linux against a locally compiled `kurmanci-ffi` library. Precompiled XCFramework packaging and iOS device/simulator integration are scheduled for Milestone **5C.2**.

---

## 1. Supported Platforms & Prerequisites

- **Supported Platforms**: macOS 10.15+, Linux (Ubuntu 20.04+).
- **Prerequisite**: The native `kurmanci_ffi` library must be built first using Cargo:
  ```bash
  cargo build -p kurmanci-ffi
  ```
  This creates `libkurmanci_ffi.dylib` (macOS) or `libkurmanci_ffi.so` (Linux) under `target/debug` (or `target/release`).

---

## 2. Local Build, Test, & Run Commands

From the repository root:

### Debug Test & Run
```bash
# Build seed pack and native FFI library
cargo run -p kurmanci-data-builder -- build-pack seed
cargo build -p kurmanci-ffi

# Run Swift Package test suite
LD_LIBRARY_PATH="$PWD/target/debug" DYLD_LIBRARY_PATH="$PWD/target/debug" \
swift test --package-path swift -Xlinker -L -Xlinker "$PWD/target/debug"

# Run Swift Command-Line Example
LD_LIBRARY_PATH="$PWD/target/debug" DYLD_LIBRARY_PATH="$PWD/target/debug" \
swift run --package-path swift -Xlinker -L -Xlinker "$PWD/target/debug" \
KurmanciExample data/build/packs/seed/lexicon.bin
```

### Release Compilation
```bash
cargo build -p kurmanci-ffi --release
LD_LIBRARY_PATH="$PWD/target/release" DYLD_LIBRARY_PATH="$PWD/target/release" \
swift build --package-path swift -c release -Xlinker -L -Xlinker "$PWD/target/release"
```

---

## 3. Usage Examples

### Loading a Language Pack
```swift
import Foundation
import Kurmanci

// 1. Load from file URL
let packURL = URL(fileURLWithPath: "path/to/lexicon.bin")
let fileEngine = try KurmanciEngine(packURL: packURL)

// 2. Load from in-memory Data
let packData = try Data(contentsOf: packURL)
let dataEngine = try KurmanciEngine(packData: packData)

print("Loaded pack: \(fileEngine.packInfo.languageTag), entries: \(fileEngine.packInfo.entryCount)")
```

### Querying Lexicon & Suggestions
```swift
// Check known word
let isKnown = try engine.isKnownWord("welat")

// Spelling corrections
let corrections = try engine.correct("spaz", limit: 5)

// Prefix completions
let completions = try engine.complete("roj", limit: 5)

// Combined suggestions
let suggestions = try engine.suggest("şeq", limit: 5)
```

### Next-Word Prediction
```swift
// Predict next word following 1-word or multi-word context
let predictions = try engine.predictNext(context: ["ez", "ji"], limit: 5)
for p in predictions {
    print("Candidate: \(p.text), source: \(p.source), probability: \(p.probabilityMillionths)")
}
```

---

## 4. Architecture & Safety Guarantees

- **ABI Compatibility (v1.0)**: Initializers verify `kmr_abi_version_major() == 1` and `kmr_abi_version_minor() >= 0` before handle allocation.
- **Embedded NUL & URL Validation**: Embedded NUL (`\0`) bytes in inputs and non-file URLs are rejected at the Swift boundary with `KurmanciError.invalidArgument`.
- **Context Slicing**: `predictNext(context:)` slices arrays to the final two context words (`Array(context.suffix(2))`), preventing unbounded closure recursion or stack exhaustion.
- **Memory Ownership & Lifetime**: All native result handles use `defer { kmr_..._destroy(...) }`. Text strings are copied immediately into Swift-owned `String` values.
- **Concurrency & Thread Safety**: `KurmanciEngine` is marked `@unchecked Sendable`. Immutable native engine handles support concurrent read-only queries across multiple threads without locking.

---

## 5. Milestone 5C.2 Roadmap

Distributable Apple SDK distribution (bundled XCFramework with iOS device/simulator targets) is planned for **Milestone 5C.2**.
