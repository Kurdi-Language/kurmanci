import Foundation
import Kurmanci

func runConsumerTests(packPath: String) throws {
    print("=== macOS Consumer Integration Test ===")

    let packURL = URL(fileURLWithPath: packPath)

    // 1. Pack Info Verification
    let engine = try KurmanciEngine(packURL: packURL)
    let info = engine.packInfo
    print("✅ Loaded pack info: tag=\(info.languageTag), entries=\(info.entryCount)")
    assert(info.formatVersion == 4, "Expected pack format version 4")

    // 2. Core Query APIs
    let isKnown = try engine.isKnownWord("welat")
    assert(isKnown, "Expected 'welat' to be known")

    let corrections = try engine.correct("spaz", limit: 5)
    assert(!corrections.isEmpty, "Expected non-empty corrections for 'spaz'")

    let completions = try engine.complete("roj", limit: 5)
    assert(!completions.isEmpty, "Expected non-empty completions for 'roj'")

    // 3. Prediction Test with Dedicated Model Fixture
    let sourcePath = #file
    let mainDir = URL(fileURLWithPath: sourcePath).deletingLastPathComponent()
    let appleDir = mainDir.deletingLastPathComponent()
    let localPredPath = appleDir.appendingPathComponent("fixtures/prediction_test.bin").path
    let siblingPredPath = mainDir.appendingPathComponent("prediction_test.bin").path
    let predPath: String
    if FileManager.default.fileExists(atPath: localPredPath) {
        predPath = localPredPath
    } else if FileManager.default.fileExists(atPath: siblingPredPath) {
        predPath = siblingPredPath
    } else {
        fatalError("❌ Error: Required prediction fixture 'prediction_test.bin' is missing")
    }

    let predEngine = try KurmanciEngine(packURL: URL(fileURLWithPath: predPath))
    let predictions = try predEngine.predictNext(context: ["ez"], limit: 5)
    assert(!predictions.isEmpty, "Expected non-empty predictions for 'ez'")
    assert(predictions.first?.text == "ji", "Expected prediction text 'ji'")
    assert(predictions.first?.source == .bigram, "Expected prediction source .bigram")
    assert(predictions.first?.count == 3, "Expected prediction count 3")
    print("✅ Prediction test verified: text=\(predictions.first?.text ?? ""), source=\(String(describing: predictions.first?.source)), count=\(predictions.first?.count ?? 0)")

    // 4. Limit Clamping & Invalid Limits
    let zeroLimitCorr = try engine.correct("spaz", limit: 0)
    assert(zeroLimitCorr.isEmpty, "Expected 0 results for limit 0")

    var negLimitCaught = false
    do {
        _ = try engine.correct("spaz", limit: -5)
    } catch KurmanciError.invalidArgument {
        negLimitCaught = true
    }
    assert(negLimitCaught, "Expected invalidArgument error for negative limit")

    // 5. Embedded NUL Byte Rejection
    var nulCaught = false
    do {
        _ = try engine.isKnownWord("wel\0at")
    } catch KurmanciError.invalidArgument {
        nulCaught = true
    }
    assert(nulCaught, "Expected embedded NUL error")

    // 6. Non-File URL Rejection
    var nonFileCaught = false
    do {
        _ = try KurmanciEngine(packURL: URL(string: "https://example.com/pack.bin")!)
    } catch KurmanciError.invalidArgument {
        nonFileCaught = true
    }
    assert(nonFileCaught, "Expected invalid URL error")

    // 7. Malformed Pack Data Rejection
    var malformedCaught = false
    do {
        _ = try KurmanciEngine(packData: Data([0x00, 0x01, 0x02, 0x03]))
    } catch KurmanciError.invalidPack {
        malformedCaught = true
    }
    assert(malformedCaught, "Expected invalid pack error")

    // 8. Repeated Creation & Destruction Lifecycle
    for _ in 0..<50 {
        let tempEngine = try KurmanciEngine(packURL: packURL)
        let tempKnown = try tempEngine.isKnownWord("welat")
        assert(tempKnown)
    }

    // 9. Concurrent Read-Only Queries
    let group = DispatchGroup()
    let queue = DispatchQueue(label: "org.kurmanci.macosConsumerTest", attributes: .concurrent)
    let lock = NSLock()
    var failures: [String] = []

    for i in 0..<20 {
        group.enter()
        queue.async {
            defer { group.leave() }
            do {
                let res = try engine.isKnownWord("welat")
                if !res {
                    lock.lock()
                    failures.append("Task \(i): isKnownWord failed")
                    lock.unlock()
                }
            } catch {
                lock.lock()
                failures.append("Task \(i): exception \(error)")
                lock.unlock()
            }
        }
    }
    group.wait()
    assert(failures.isEmpty, "Concurrent queries produced failures: \(failures)")

    print("✅ macOS Consumer Integration Tests PASSED 100%!")
}

let args = CommandLine.arguments
let packPath: String
if args.count >= 2 {
    packPath = args[1]
} else {
    let sourcePath = #file
    let mainDir = URL(fileURLWithPath: sourcePath).deletingLastPathComponent()
    let appleDir = mainDir.deletingLastPathComponent()
    packPath = appleDir.appendingPathComponent("fixtures/apple_consumer_test.bin").path
}

do {
    try runConsumerTests(packPath: packPath)
} catch {
    print("❌ macOS Consumer test failed: \(error)")
    exit(1)
}
