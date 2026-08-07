import XCTest
import Kurmanci

final class KurmanciConsumerTests: XCTestCase {

    private func fixtureURL(named name: String = "apple_consumer_test") -> URL {
        let testBundle = Bundle(for: type(of: self))
        if let url = testBundle.url(forResource: name, withExtension: "bin") ?? testBundle.url(forResource: name, withExtension: nil) {
            return url
        }
        let mainBundle = Bundle.main
        if let url = mainBundle.url(forResource: name, withExtension: "bin") ?? mainBundle.url(forResource: name, withExtension: nil) {
            return url
        }
        if let envRoot = ProcessInfo.processInfo.environment["REPO_ROOT"] ?? ProcessInfo.processInfo.environment["SRCROOT"] {
            let url1 = URL(fileURLWithPath: envRoot).appendingPathComponent("integration/apple/fixtures/\(name).bin")
            if FileManager.default.fileExists(atPath: url1.path) {
                return url1
            }
            let url2 = URL(fileURLWithPath: envRoot).appendingPathComponent("fixtures/\(name).bin")
            if FileManager.default.fileExists(atPath: url2.path) {
                return url2
            }
        }
        let searchDirs = [
            URL(fileURLWithPath: testBundle.bundlePath),
            URL(fileURLWithPath: mainBundle.bundlePath),
            URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
        ]
        for dir in searchDirs {
            var curr = dir
            for _ in 0..<6 {
                let candidate = curr.appendingPathComponent("integration/apple/fixtures/\(name).bin")
                if FileManager.default.fileExists(atPath: candidate.path) {
                    return candidate
                }
                let candidateDirect = curr.appendingPathComponent("fixtures/\(name).bin")
                if FileManager.default.fileExists(atPath: candidateDirect.path) {
                    return candidateDirect
                }
                curr = curr.deletingLastPathComponent()
            }
        }
        let rawFile = #filePath
        let testDir = URL(fileURLWithPath: rawFile).deletingLastPathComponent()
        let iosConsumerDir = testDir.deletingLastPathComponent()
        let appleDir = iosConsumerDir.deletingLastPathComponent()
        let fallbackURL = appleDir.appendingPathComponent("fixtures/\(name).bin")
        if FileManager.default.fileExists(atPath: fallbackURL.path) {
            return fallbackURL
        }
        fatalError("Could not locate fixture pack '\(name).bin' at bundle or path \(fallbackURL.path)")
    }

    func testEngineQueriesInIOSConsumer() throws {
        let packURL = fixtureURL(named: "apple_consumer_test")
        let engine = try KurmanciEngine(packURL: packURL)
        let info = engine.packInfo

        XCTAssertEqual(info.languageTag, "ku-Latn")
        XCTAssertEqual(info.entryCount, 33)
        XCTAssertEqual(info.formatVersion, 4)

        XCTAssertTrue(try engine.isKnownWord("welat"))
        XCTAssertTrue(try engine.isKnownWord("spas"))
        XCTAssertFalse(try engine.isKnownWord("nediyar"))

        let suggestions = try engine.suggest("spaz", limit: 5)
        XCTAssertFalse(suggestions.isEmpty)
        XCTAssertEqual(suggestions.first?.text, "spas")

        let completions = try engine.complete("roj", limit: 5)
        XCTAssertFalse(completions.isEmpty)
        XCTAssertEqual(completions.first?.text, "roja")
    }

    func testPredictionWithModelFixture() throws {
        let predURL = fixtureURL(named: "prediction_test")
        let engine = try KurmanciEngine(packURL: predURL)

        let predictions = try engine.predictNext(context: ["ez"], limit: 5)
        XCTAssertFalse(predictions.isEmpty)
        XCTAssertEqual(predictions.first?.text, "ji")
        XCTAssertEqual(predictions.first?.source, .bigram)
        XCTAssertEqual(predictions.first?.count, 3)
    }

    func testInitializationFromInMemoryData() throws {
        let data = try Data(contentsOf: fixtureURL(named: "apple_consumer_test"))
        let engine = try KurmanciEngine(packData: data)
        XCTAssertTrue(try engine.isKnownWord("welat"))
    }

    func testMalformedPackRejection() throws {
        let invalidData = Data([0x00, 0x01, 0x02, 0x03, 0x04])
        XCTAssertThrowsError(try KurmanciEngine(packData: invalidData)) { error in
            guard let engineErr = error as? KurmanciError else {
                XCTFail("Expected KurmanciError")
                return
            }
            if case .invalidPack = engineErr {
                // Passed
            } else {
                XCTFail("Expected invalidPack error, got \(engineErr)")
            }
        }
    }

    func testEmbeddedNULRejection() throws {
        let engine = try KurmanciEngine(packURL: fixtureURL(named: "apple_consumer_test"))
        XCTAssertThrowsError(try engine.isKnownWord("wel\0at"))
        XCTAssertThrowsError(try engine.suggest("sp\0az", limit: 5))
    }

    func testNonFileURLRejection() throws {
        XCTAssertThrowsError(try KurmanciEngine(packURL: URL(string: "https://example.com/pack.bin")!)) { error in
            guard let engineErr = error as? KurmanciError, case .invalidArgument = engineErr else {
                XCTFail("Expected invalidArgument error")
                return
            }
        }
    }

    func testLimitClampingAndValidation() throws {
        let engine = try KurmanciEngine(packURL: fixtureURL(named: "apple_consumer_test"))
        XCTAssertTrue(try engine.suggest("spaz", limit: 0).isEmpty)

        XCTAssertThrowsError(try engine.suggest("spaz", limit: -5)) { error in
            guard let engineErr = error as? KurmanciError, case .invalidArgument = engineErr else {
                XCTFail("Expected invalidArgument error for negative limit")
                return
            }
        }
    }

    func testRepeatedInitializationAndDestruction() throws {
        let data = try Data(contentsOf: fixtureURL(named: "apple_consumer_test"))
        for _ in 0..<50 {
            let engine = try KurmanciEngine(packData: data)
            XCTAssertTrue(try engine.isKnownWord("welat"))
        }
    }

    func testConcurrentReadOnlyQueries() throws {
        let engine = try KurmanciEngine(packURL: fixtureURL(named: "apple_consumer_test"))
        let group = DispatchGroup()
        let queue = DispatchQueue(label: "org.kurmanci.iosConsumerTest", attributes: .concurrent)
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
        XCTAssertTrue(failures.isEmpty, "Concurrent queries produced failures: \(failures)")
    }
}
