#if canImport(XCTest)
import XCTest
#endif
import Foundation
import CKurmanci
@testable import Kurmanci

final class KurmanciEngineTests: XCTestCase {
    private var seedPackURL: URL {
        let repoDir = URL(fileURLWithPath: #file)
            .deletingLastPathComponent() // KurmanciTests/
            .deletingLastPathComponent() // Tests/
            .deletingLastPathComponent() // swift/
            .deletingLastPathComponent() // repo root
        return repoDir.appendingPathComponent("data/build/packs/seed/lexicon.bin")
    }

    private var fixturePackURL: URL {
        #if SWIFT_PACKAGE
        if let url = Bundle.module.url(forResource: "prediction_test", withExtension: "bin", subdirectory: "Fixtures") {
            return url
        }
        if let url = Bundle.module.url(forResource: "prediction_test", withExtension: "bin") {
            return url
        }
        #endif
        let testDir = URL(fileURLWithPath: #file).deletingLastPathComponent()
        return testDir.appendingPathComponent("Fixtures/prediction_test.bin")
    }

    func testABICompatibilityCheck() {
        XCTAssertEqual(kmr_abi_version_major(), 1)
        XCTAssertEqual(kmr_abi_version_minor(), 0)
    }

    func testInitWithPackURLAndData() throws {
        let fileEngine = try KurmanciEngine(packURL: seedPackURL)
        XCTAssertEqual(fileEngine.packInfo.languageTag, "ku-Latn")
        XCTAssertEqual(fileEngine.packInfo.formatVersion, 4)
        XCTAssertGreaterThan(fileEngine.packInfo.entryCount, 0)

        let data = try Data(contentsOf: seedPackURL)
        let dataEngine = try KurmanciEngine(packData: data)
        XCTAssertEqual(fileEngine.packInfo, dataEngine.packInfo)
    }

    func testIsKnownWord() throws {
        let engine = try KurmanciEngine(packURL: seedPackURL)
        XCTAssertTrue(try engine.isKnownWord("welat"))
        XCTAssertFalse(try engine.isKnownWord("nonexistent123"))
    }

    func testCorrect() throws {
        let engine = try KurmanciEngine(packURL: seedPackURL)
        let corrections = try engine.correct("spaz", limit: 5)
        XCTAssertGreaterThan(corrections.count, 0)
        XCTAssertEqual(corrections[0].text, "spas")
        XCTAssertEqual(corrections[0].kind, SuggestionKind.correction)
    }

    func testComplete() throws {
        let engine = try KurmanciEngine(packURL: seedPackURL)
        let completions = try engine.complete("roj", limit: 5)
        XCTAssertGreaterThan(completions.count, 0)
    }

    func testSuggest() throws {
        let engine = try KurmanciEngine(packURL: seedPackURL)
        let suggestions = try engine.suggest("şeq", limit: 5)
        XCTAssertGreaterThan(suggestions.count, 0)
    }

    func testPredictNext() throws {
        let engine = try KurmanciEngine(packURL: seedPackURL)

        // Empty context
        let emptyPreds = try engine.predictNext(context: [], limit: 5)
        XCTAssertEqual(emptyPreds.count, 0)

        // 1-word context
        let onePreds = try engine.predictNext(context: ["ez"], limit: 5)
        XCTAssertEqual(onePreds.count, 0)

        // 2-word context
        let twoPreds = try engine.predictNext(context: ["ez", "diçim"], limit: 5)
        XCTAssertEqual(twoPreds.count, 0)

        // Longer (4-word) context
        let longer = try engine.predictNext(context: ["ev", "gotin", "ez", "diçim"], limit: 5)
        XCTAssertEqual(longer, twoPreds)

        // Non-ASCII Kurmancî context
        let kurdPreds = try engine.predictNext(context: ["şev", "baş"], limit: 5)
        XCTAssertEqual(kurdPreds.count, 0)
    }

    func testPredictNextUnboundedContextRecursionProtection() throws {
        let engine = try KurmanciEngine(packURL: fixturePackURL)

        let short = try engine.predictNext(context: ["ez", "ji"], limit: 5)
        let long = try engine.predictNext(
            context: Array(repeating: "ignored", count: 1_000) + ["ez", "ji"],
            limit: 5
        )

        XCTAssertEqual(long, short)
        XCTAssertGreaterThan(short.count, 0)
    }

    func testNonEmptyPredictionsWithFixture() throws {
        let engine = try KurmanciEngine(packURL: fixturePackURL)

        // 1. One-word bigram prediction ("ez" -> "ji")
        let bigramPreds = try engine.predictNext(context: ["ez"], limit: 5)
        XCTAssertGreaterThan(bigramPreds.count, 0)
        XCTAssertEqual(bigramPreds[0].text, "ji")
        XCTAssertEqual(bigramPreds[0].source, PredictionSource.bigram)
        XCTAssertGreaterThan(bigramPreds[0].count, 0)
        XCTAssertGreaterThan(bigramPreds[0].probabilityMillionths, 0)

        // 2. Two-word trigram prediction ("ez", "ji" -> "bo")
        let trigramPreds = try engine.predictNext(context: ["ez", "ji"], limit: 5)
        XCTAssertGreaterThan(trigramPreds.count, 0)
        XCTAssertEqual(trigramPreds[0].text, "bo")
        XCTAssertEqual(trigramPreds[0].source, PredictionSource.trigram)
        XCTAssertGreaterThan(trigramPreds[0].count, 0)
        XCTAssertGreaterThan(trigramPreds[0].probabilityMillionths, 0)

        // 3. Trigram miss followed by bigram backoff ("rojb", "ji" -> "bo")
        let backoffPreds = try engine.predictNext(context: ["rojb", "ji"], limit: 5)
        XCTAssertGreaterThan(backoffPreds.count, 0)
        XCTAssertEqual(backoffPreds[0].text, "bo")
        XCTAssertEqual(backoffPreds[0].source, PredictionSource.bigramBackoff)

        // 4. Context longer than two words matching final two words
        let longerPreds = try engine.predictNext(
            context: ["ev", "gotin", "ez", "ji"],
            limit: 5
        )
        XCTAssertEqual(longerPreds, trigramPreds)
    }

    func testEmbeddedNULAndNonFileURLValidation() throws {
        let engine = try KurmanciEngine(packURL: seedPackURL)

        // Embedded NUL character rejection across all inputs
        XCTAssertThrowsError(try engine.isKnownWord("welat\0ignored")) { error in
            guard case KurmanciError.invalidArgument = error else {
                XCTFail("Expected invalidArgument error, got \(error)")
                return
            }
        }

        XCTAssertThrowsError(try engine.correct("spaz\0ignored", limit: 5)) { error in
            guard case KurmanciError.invalidArgument = error else {
                XCTFail("Expected invalidArgument error, got \(error)")
                return
            }
        }

        XCTAssertThrowsError(try engine.complete("roj\0ignored", limit: 5)) { error in
            guard case KurmanciError.invalidArgument = error else {
                XCTFail("Expected invalidArgument error, got \(error)")
                return
            }
        }

        XCTAssertThrowsError(try engine.suggest("şeq\0ignored", limit: 5)) { error in
            guard case KurmanciError.invalidArgument = error else {
                XCTFail("Expected invalidArgument error, got \(error)")
                return
            }
        }

        XCTAssertThrowsError(try engine.predictNext(context: ["ez\0ignored", "diçim"], limit: 5)) { error in
            guard case KurmanciError.invalidArgument = error else {
                XCTFail("Expected invalidArgument error, got \(error)")
                return
            }
        }

        // Non-file URL rejection
        let httpURL = URL(string: "https://example.com/lexicon.bin")!
        XCTAssertThrowsError(try KurmanciEngine(packURL: httpURL)) { error in
            guard case KurmanciError.invalidArgument = error else {
                XCTFail("Expected invalidArgument error, got \(error)")
                return
            }
        }
    }

    func testConcurrentQueries() throws {
        let engine = try KurmanciEngine(packURL: fixturePackURL)
        let group = DispatchGroup()
        let queue = DispatchQueue(label: "org.kurmanci.concurrentTest", attributes: .concurrent)

        let lock = NSLock()
        var failures: [String] = []

        func recordFailure(_ message: String) {
            lock.lock()
            failures.append(message)
            lock.unlock()
        }

        for _ in 0..<20 {
            group.enter()

            queue.async {
                defer { group.leave() }

                do {
                    let known = try engine.isKnownWord("welat")
                    let suggestions = try engine.suggest("spaz", limit: 5)
                    let predictions = try engine.predictNext(context: ["ez", "ji"], limit: 5)

                    if !known || suggestions.isEmpty || predictions.isEmpty {
                        recordFailure("Unexpected empty or false query result")
                    }
                } catch {
                    recordFailure(String(describing: error))
                }
            }
        }

        group.wait()
        XCTAssertTrue(failures.isEmpty, failures.joined(separator: "\n"))
    }

    func testLimits() throws {
        let engine = try KurmanciEngine(packURL: seedPackURL)

        // Limit 0 returns empty array
        let zeroCorr = try engine.correct("spaz", limit: 0)
        XCTAssertEqual(zeroCorr.count, 0)

        // Negative limit throws invalidArgument
        XCTAssertThrowsError(try engine.correct("spaz", limit: -1)) { error in
            guard case KurmanciError.invalidArgument = error else {
                XCTFail("Expected invalidArgument error, got \(error)")
                return
            }
        }

        // Limit > 50 succeeds and is clamped to max 50
        let largeCorr = try engine.correct("spaz", limit: 100)
        XCTAssertGreaterThan(largeCorr.count, 0)
        XCTAssertLessThanOrEqual(largeCorr.count, 50)
    }

    func testErrorHandling() throws {
        // Missing file throws ioError
        let missingURL = URL(fileURLWithPath: "nonexistent_pack_123.bin")
        XCTAssertThrowsError(try KurmanciEngine(packURL: missingURL)) { error in
            guard case KurmanciError.ioError = error else {
                XCTFail("Expected ioError, got \(error)")
                return
            }
        }

        // Empty Data throws invalidArgument
        XCTAssertThrowsError(try KurmanciEngine(packData: Data())) { error in
            guard case KurmanciError.invalidArgument = error else {
                XCTFail("Expected invalidArgument, got \(error)")
                return
            }
        }

        // Corrupted pack Data throws checksumMismatch or invalidPack
        var corruptData = try Data(contentsOf: seedPackURL)
        if !corruptData.isEmpty {
            corruptData[corruptData.count - 1] ^= 0xFF
        }
        XCTAssertThrowsError(try KurmanciEngine(packData: corruptData)) { error in
            switch error {
            case KurmanciError.checksumMismatch, KurmanciError.invalidPack:
                break
            default:
                XCTFail("Expected checksumMismatch or invalidPack, got \(error)")
            }
        }
    }

    func testUnicodeInput() throws {
        let engine = try KurmanciEngine(packURL: seedPackURL)
        XCTAssertTrue(try engine.isKnownWord("bijî"))
        XCTAssertTrue(try engine.isKnownWord("çawa"))

        let corrections = try engine.correct("biji", limit: 5)
        XCTAssertGreaterThan(corrections.count, 0)
    }

    func testEnumAndStatusConversionFallbacks() {
        XCTAssertEqual(SuggestionKind(cValue: 999), .unknown(999))
        XCTAssertEqual(PredictionSource(cValue: 999), .unknown(999))

        XCTAssertThrowsError(try KurmanciError.check(999)) { error in
            guard case KurmanciError.unknownStatus(let code, _) = error, code == 999 else {
                XCTFail("Expected unknownStatus(code: 999), got \(error)")
                return
            }
        }
    }

    func testRepeatedCreationAndDestruction() throws {
        let data = try Data(contentsOf: seedPackURL)
        for _ in 0..<50 {
            let engine = try KurmanciEngine(packData: data)
            XCTAssertTrue(try engine.isKnownWord("welat"))
            let suggestions = try engine.suggest("roj", limit: 5)
            XCTAssertGreaterThan(suggestions.count, 0)
        }
    }
}

#if !canImport(XCTest)
@main
struct TestRunner {
    static func main() throws {
        print("Running KurmanciEngineTests (standalone test runner)...")
        let tests = KurmanciEngineTests()
        tests.testABICompatibilityCheck()
        try tests.testInitWithPackURLAndData()
        try tests.testIsKnownWord()
        try tests.testCorrect()
        try tests.testComplete()
        try tests.testSuggest()
        try tests.testPredictNext()
        try tests.testPredictNextUnboundedContextRecursionProtection()
        try tests.testNonEmptyPredictionsWithFixture()
        try tests.testEmbeddedNULAndNonFileURLValidation()
        try tests.testConcurrentQueries()
        try tests.testLimits()
        try tests.testErrorHandling()
        try tests.testUnicodeInput()
        tests.testEnumAndStatusConversionFallbacks()
        try tests.testRepeatedCreationAndDestruction()
        print("✅ All KurmanciEngineTests passed successfully!")
    }
}
#endif
