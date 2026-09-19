import XCTest
import Kurmanci
import CryptoKit
import Darwin

/// Internal device benchmark harness (not a consumer feature): loads the bundled
/// `benchmark_pack.bin`, records load time, resident memory, per-operation latency for
/// known / suggest / correct / complete / predict, and repeated-query stability, and prints
/// one JSON line prefixed `KURMANCI_DEVICE_BENCHMARK` that `scripts/apple/device-benchmark.sh`
/// collects. The committed `benchmark_pack.bin` is a tiny placeholder; the script swaps a
/// real pack in for a measurement run. No timing assertion: numbers are recorded, only
/// stability (identical results across repetitions) is asserted.
final class DeviceBenchmarkTests: XCTestCase {

    private func packURL() -> URL {
        let bundles = [Bundle(for: type(of: self)), Bundle.main]
        for bundle in bundles {
            if let url = bundle.url(forResource: "benchmark_pack", withExtension: "bin") {
                return url
            }
        }
        if let path = ProcessInfo.processInfo.environment["KURMANCI_BENCHMARK_PACK"],
           FileManager.default.fileExists(atPath: path) {
            return URL(fileURLWithPath: path)
        }
        fatalError("benchmark_pack.bin is not in the test bundle and KURMANCI_BENCHMARK_PACK is not set")
    }

    private func residentBytes() -> UInt64 {
        var info = mach_task_basic_info()
        var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? UInt64(info.resident_size) : 0
    }

    private func deviceModel() -> String {
        var system = utsname()
        uname(&system)
        return withUnsafePointer(to: &system.machine) {
            $0.withMemoryRebound(to: CChar.self, capacity: Int(_SYS_NAMELEN)) { String(cString: $0) }
        }
    }

    private func percentile(_ sorted: [Double], _ p: Double) -> Double {
        guard !sorted.isEmpty else { return 0 }
        let rank = min(sorted.count - 1, Int((Double(sorted.count - 1) * p).rounded()))
        return sorted[rank]
    }

    private func microseconds(_ start: DispatchTime, _ end: DispatchTime) -> Double {
        Double(end.uptimeNanoseconds - start.uptimeNanoseconds) / 1000.0
    }

    func testDeviceBenchmarkReport() throws {
        let url = packURL()
        let data = try Data(contentsOf: url)
        let packSha256 = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()

        // Load time: cold loads of the same bytes.
        var loadsMs: [Double] = []
        for _ in 0..<5 {
            let start = DispatchTime.now()
            let engine = try KurmanciEngine(packData: data)
            loadsMs.append(microseconds(start, DispatchTime.now()) / 1000.0)
            _ = engine.packInfo
        }
        loadsMs.sort()

        let engine = try KurmanciEngine(packData: data)
        let info = engine.packInfo
        let rssAfterLoad = residentBytes()

        // Operations, each measured over `iterations` calls.
        let iterations = 200
        struct Op { let name: String; let input: String; let run: () throws -> Int }
        let ops: [Op] = [
            Op(name: "known_hit", input: "welat") { try engine.isKnownWord("welat") ? 1 : 0 },
            Op(name: "known_miss", input: "xyzqwv") { try engine.isKnownWord("xyzqwv") ? 1 : 0 },
            Op(name: "suggest", input: "rojbas") { try engine.suggest("rojbas", limit: 5).count },
            Op(name: "correct", input: "spaz") { try engine.correct("spaz", limit: 5).count },
            Op(name: "complete", input: "ro") { try engine.complete("ro", limit: 5).count },
            Op(name: "predict", input: "ez") { try engine.predictNext(context: ["ez"], limit: 5).count },
        ]
        var opReports: [[String: Any]] = []
        for op in ops {
            var samples: [Double] = []
            var count = 0
            for _ in 0..<iterations {
                let start = DispatchTime.now()
                count = try op.run()
                samples.append(microseconds(start, DispatchTime.now()))
            }
            samples.sort()
            opReports.append([
                "name": op.name,
                "input": op.input,
                "iterations": iterations,
                "p50_us": percentile(samples, 0.5),
                "p95_us": percentile(samples, 0.95),
                "max_us": samples.last ?? 0,
                "result_count": count,
            ])
        }

        // Repeated-query stability: the same inputs must give identical results every time.
        func snapshot() throws -> String {
            var parts: [String] = []
            parts.append(String(try engine.isKnownWord("welat")))
            parts.append(try engine.suggest("rojbas", limit: 5).map { "\($0.text):\($0.editCost)" }.joined(separator: ","))
            parts.append(try engine.correct("spaz", limit: 5).map { "\($0.text):\($0.editCost)" }.joined(separator: ","))
            parts.append(try engine.complete("ro", limit: 5).map { $0.text }.joined(separator: ","))
            parts.append(try engine.predictNext(context: ["ez"], limit: 5).map { "\($0.text):\($0.count)" }.joined(separator: ","))
            return parts.joined(separator: "|")
        }
        let reference = try snapshot()
        let rounds = 300
        var stable = true
        for _ in 0..<rounds where stable {
            stable = try snapshot() == reference
        }
        let rssAfterQueries = residentBytes()

        let report: [String: Any] = [
            "schema_version": "device-benchmark-v1",
            "platform": "ios",
            "device_model": deviceModel(),
            "os_version": ProcessInfo.processInfo.operatingSystemVersionString,
            "simulator": ProcessInfo.processInfo.environment["SIMULATOR_DEVICE_NAME"] != nil,
            "pack_file": url.lastPathComponent,
            "pack_bytes": data.count,
            "pack_sha256": packSha256,
            "entry_count": info.entryCount,
            "pack_format_version": info.formatVersion,
            "load_ms_median": percentile(loadsMs, 0.5),
            "load_ms_min": loadsMs.first ?? 0,
            "load_ms_max": loadsMs.last ?? 0,
            "rss_after_load_bytes": rssAfterLoad,
            "rss_after_queries_bytes": rssAfterQueries,
            "operations": opReports,
            "stability_rounds": rounds,
            "stable": stable,
        ]
        let jsonData = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
        let json = String(decoding: jsonData, as: UTF8.self)
        print("KURMANCI_DEVICE_BENCHMARK \(json)")
        let attachment = XCTAttachment(data: jsonData, uniformTypeIdentifier: "public.json")
        attachment.name = "kurmanci-device-benchmark.json"
        attachment.lifetime = .keepAlways
        add(attachment)
        XCTAssertTrue(stable, "repeated queries returned different results")
    }
}
