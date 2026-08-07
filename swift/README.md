# Kurmancî Swift SDK & Apple XCFramework Distribution (Milestone 5C.1 & 5C.2)

`Kurmanci` is an idiomatic Swift wrapper package built on top of the stable Kurmancî C ABI (`kurmanci-ffi`).

- **Milestone 5C.1**: Repository-local Swift wrapper foundation (`swift/Package.swift`) validated on macOS and Linux against local native builds (`CKurmanci`).
- **Milestone 5C.2**: Zero-Rust-dependency Apple SDK distribution (`dist/swift-package/Package.swift`) precompiled as an XCFramework for macOS (arm64 & x86_64), iOS Device (arm64), and iOS Simulator (arm64 & x86_64) published to `Kurdi-Language/kurmanci-swift`.

---

## 1. Supported Platforms & Deployment Targets

| Target Platform | Architecture Slices | Deployment Minimum | Distribution Method |
| :--- | :--- | :--- | :--- |
| **macOS** | `arm64`, `x86_64` | macOS 11.0+ | `KurmanciFFI.xcframework` / SwiftPM |
| **iOS Device** | `arm64` | iOS 14.0+ | `KurmanciFFI.xcframework` / SwiftPM |
| **iOS Simulator** | `arm64`, `x86_64` | iOS 14.0+ | `KurmanciFFI.xcframework` / SwiftPM |
| **Linux** | Host | Ubuntu 20.04+ | Monorepo source package (`swift/`) |

---

## 2. Consuming the Precompiled Apple SDK (Zero Rust Dependency)

Developers adding Kurmancî to an Xcode project or Swift application do **not** need a local Rust toolchain or linker path overrides.

### Swift Package Manager Dependency

In your `Package.swift`:
```swift
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MyApp",
    dependencies: [
        .package(
            url: "https://github.com/Kurdi-Language/kurmanci-swift",
            exact: "0.1.0"
        )
    ],
    targets: [
        .target(
            name: "MyApp",
            dependencies: [
                .product(name: "Kurmanci", package: "kurmanci-swift")
            ]
        )
    ]
)
```

In your Swift source code:
```swift
import Foundation
import Kurmanci

let engine = try KurmanciEngine(packURL: packURL)
let isKnown = try engine.isKnownWord("welat")
let suggestions = try engine.suggest("spaz", limit: 5)
```

---

## 3. Apple SDK Build & Release Automation Scripts

The monorepo provides one-command automated scripts under `scripts/apple/`:

1. **`build-xcframework.sh`**:
   Cross-compiles `kurmanci-ffi` across Apple targets (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`), creates universal static archives via `lipo`, and packages `dist/artifacts/KurmanciFFI.xcframework`.
   ```bash
   ./scripts/apple/build-xcframework.sh --version 0.1.0
   ```

2. **`verify-xcframework.sh`**:
   Validates `Info.plist`, `lipo -info`, exported `kmr_*` symbols against `ffi/include/required_symbols.txt`, and header/modulemap integrity.
   ```bash
   ./scripts/apple/verify-xcframework.sh
   ```

3. **`create-release-archive.sh`**:
   Creates `dist/KurmanciFFI-v0.1.0.xcframework.zip`, computes SHA-256 / SwiftPM checksums, and outputs `release-manifest.json`.
   ```bash
   ./scripts/apple/create-release-archive.sh --version 0.1.0
   ```

4. **`generate-release-package.sh`**:
   Generates `dist/swift-package/Package.swift` (remote URL distribution) and `dist/swift-package-local/Package.swift` (local binaryTarget path).
   ```bash
   ./scripts/apple/generate-release-package.sh --version 0.1.0
   ```

5. **`test-consumers.sh`**:
   Executes plain C header tests, direct `import KurmanciFFI` tests, macOS consumer tests, dependency/deployment target `otool` inspections, and iOS Simulator tests without Rust invocations.
   ```bash
   ./scripts/apple/test-consumers.sh
   ```

---

## 4. Local Development & Source Library Commands (5C.1)

For monorepo development and Linux/macOS source testing against a local `cargo build -p kurmanci-ffi`:

```bash
# Build seed pack and native FFI library
cargo run -p kurmanci-data-builder -- build-pack seed
cargo build -p kurmanci-ffi

# Test local Swift package
LD_LIBRARY_PATH="$PWD/target/debug" DYLD_LIBRARY_PATH="$PWD/target/debug" \
swift test --package-path swift -Xlinker -L -Xlinker "$PWD/target/debug"

# Run Swift Command-Line Example
LD_LIBRARY_PATH="$PWD/target/debug" DYLD_LIBRARY_PATH="$PWD/target/debug" \
swift run --package-path swift -Xlinker -L -Xlinker "$PWD/target/debug" \
KurmanciExample data/build/packs/seed/lexicon.bin
```
