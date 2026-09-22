import Foundation
import Kurmanci
import CryptoKit

/// Internal try-out state for the remote consumer host: loads one of the packs staged under
/// `Packs/` by `scripts/apple/tryout.sh`, checks it against the hashes the script recorded
/// in `Packs/packs.json`, and answers every text change with the engine's known / suggest /
/// complete / correct results for the word being typed and the next-word predictions for the
/// words before it, each with its measured call latency. It is an integration harness for a
/// person holding the phone, not a keyboard: nothing here ranks, filters or rewrites what the
/// published engine returns.
final class TryOutModel: ObservableObject {

    struct StagedPack: Identifiable, Equatable {
        let id: String            // pack_id as recorded by the script (reviewed, experimental-full)
        let url: URL
        let recordedSha256: String?
    }

    struct Staging {
        let releaseVersion: String?
        let bundleIdentity: String?
        let sdkVersion: String?
        let packs: [StagedPack]
    }

    struct LoadedPack {
        let pack: StagedPack
        let engine: KurmanciEngine
        let info: PackInfo
        let bytes: Int
        let sha256: String
        let loadMilliseconds: Double
    }

    struct Timed<T> {
        let value: T
        let microseconds: Double
    }

    struct WordResult {
        let word: String
        let known: Timed<Bool>
        let suggestions: Timed<[Suggestion]>
        let completions: Timed<[Suggestion]>
        let corrections: Timed<[Suggestion]>
    }

    struct PredictionResult {
        let context: [String]
        let predictions: Timed<[Prediction]>
    }

    @Published private(set) var staging: Staging
    @Published var selectedPackID: String {
        didSet { if selectedPackID != oldValue { loadSelectedPack() } }
    }
    @Published var text: String = "" {
        didSet { if text != oldValue { query() } }
    }
    @Published private(set) var loaded: LoadedPack?
    @Published private(set) var isLoading = false
    @Published private(set) var loadError: String?
    @Published private(set) var wordResult: WordResult?
    @Published private(set) var predictionResult: PredictionResult?
    @Published private(set) var queryError: String?

    let resultLimit = 5

    init(bundle: Bundle = .main) {
        let staging = TryOutModel.discoverStaging(in: bundle)
        self.staging = staging
        self.selectedPackID = staging.packs.first?.id ?? ""
        // Initial text for scripted runs (simctl launch passes it as SIMCTL_CHILD_KURMANCI_TRYOUT_TEXT).
        self.text = ProcessInfo.processInfo.environment["KURMANCI_TRYOUT_TEXT"] ?? ""
        loadSelectedPack()
    }

    // MARK: Staging discovery

    /// `Packs/` is a folder reference in the project, so whatever the script put there is
    /// copied into the app bundle unchanged. `packs.json` is optional metadata written by the
    /// script; a `.bin` without a record is still offered, without provenance.
    static func discoverStaging(in bundle: Bundle) -> Staging {
        guard let folder = bundle.url(forResource: "Packs", withExtension: nil),
              let entries = try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil) else {
            return Staging(releaseVersion: nil, bundleIdentity: nil, sdkVersion: nil, packs: [])
        }
        var releaseVersion: String?
        var bundleIdentity: String?
        var sdkVersion: String?
        var recorded: [String: (file: String, sha256: String)] = [:]
        var recordedOrder: [String] = []
        if let data = try? Data(contentsOf: folder.appendingPathComponent("packs.json")),
           let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            releaseVersion = json["release_version"] as? String
            bundleIdentity = json["bundle_identity"] as? String
            sdkVersion = json["sdk_version"] as? String
            for record in json["packs"] as? [[String: Any]] ?? [] {
                if let id = record["pack_id"] as? String,
                   let file = record["file"] as? String,
                   let sha = record["sha256"] as? String {
                    recorded[id] = (file, sha)
                    recordedOrder.append(id)
                }
            }
        }
        // Packs in the order packs.json records them (reviewed first), then any unrecorded .bin.
        var packs: [StagedPack] = []
        let bins = entries.filter { $0.pathExtension == "bin" }
        let byID = Dictionary(bins.map { ($0.deletingPathExtension().lastPathComponent, $0) }, uniquingKeysWith: { first, _ in first })
        for id in recordedOrder {
            if let url = byID[id], recorded[id]?.file == url.lastPathComponent {
                packs.append(StagedPack(id: id, url: url, recordedSha256: recorded[id]?.sha256))
            }
        }
        for url in bins.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
            let id = url.deletingPathExtension().lastPathComponent
            if !packs.contains(where: { $0.id == id }) {
                packs.append(StagedPack(id: id, url: url, recordedSha256: nil))
            }
        }
        return Staging(releaseVersion: releaseVersion, bundleIdentity: bundleIdentity, sdkVersion: sdkVersion, packs: packs)
    }

    // MARK: Loading

    func loadSelectedPack() {
        loaded = nil
        loadError = nil
        wordResult = nil
        predictionResult = nil
        guard let pack = staging.packs.first(where: { $0.id == selectedPackID }) else {
            loadError = staging.packs.isEmpty
                ? "No pack staged. Run scripts/apple/tryout.sh, which stages the packs of a published release under Packs/ before building."
                : "Unknown pack \(selectedPackID)."
            return
        }
        isLoading = true
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let outcome: Result<LoadedPack, Error> = Result {
                let data = try Data(contentsOf: pack.url)
                let sha = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
                if let expected = pack.recordedSha256, expected != sha {
                    throw TryOutError.stagedPackMismatch(pack: pack.id, expected: expected, actual: sha)
                }
                let start = DispatchTime.now()
                let engine = try KurmanciEngine(packData: data)
                let ms = Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1_000_000
                return LoadedPack(pack: pack, engine: engine, info: engine.packInfo, bytes: data.count, sha256: sha, loadMilliseconds: ms)
            }
            DispatchQueue.main.async {
                guard let self = self, self.selectedPackID == pack.id else { return }
                self.isLoading = false
                switch outcome {
                case .success(let loadedPack):
                    self.loaded = loadedPack
                    self.query()
                case .failure(let error):
                    self.loadError = "\(error)"
                }
            }
        }
    }

    // MARK: Querying

    /// The word being typed is the trailing token unless the text ends in whitespace; the
    /// prediction context is the last two tokens before it. Tokens are passed to the engine as
    /// typed (the engine applies its own normalization); only leading and trailing punctuation
    /// is stripped from context words, as the evaluation tokenizer does for sentence tokens.
    func query() {
        queryError = nil
        guard let loaded = loaded else {
            wordResult = nil
            predictionResult = nil
            return
        }
        let engine = loaded.engine
        let endsInWhitespace = text.last.map { $0.isWhitespace || $0.isNewline } ?? false
        var tokens = text.split(whereSeparator: { $0.isWhitespace || $0.isNewline }).map(String.init)
        let currentWord: String? = (!endsInWhitespace && !tokens.isEmpty) ? tokens.removeLast() : nil
        let context = tokens.suffix(2)
            .map { $0.trimmingCharacters(in: .punctuationCharacters) }
            .filter { !$0.isEmpty }

        do {
            if let word = currentWord {
                let known = try timed { try engine.isKnownWord(word) }
                let suggestions = try timed { try engine.suggest(word, limit: resultLimit) }
                let completions = try timed { try engine.complete(word, limit: resultLimit) }
                let corrections = try timed { try engine.correct(word, limit: resultLimit) }
                wordResult = WordResult(word: word, known: known, suggestions: suggestions, completions: completions, corrections: corrections)
            } else {
                wordResult = nil
            }
            if context.isEmpty {
                predictionResult = nil
            } else {
                let predictions = try timed { try engine.predictNext(context: context, limit: resultLimit) }
                predictionResult = PredictionResult(context: context, predictions: predictions)
            }
        } catch {
            wordResult = nil
            predictionResult = nil
            queryError = "\(error)"
        }
    }

    /// Replaces the word being typed with `replacement` and closes it with a space.
    func acceptForCurrentWord(_ replacement: String) {
        guard let word = wordResult?.word, text.hasSuffix(word) else { return }
        text = String(text.dropLast(word.count)) + replacement + " "
    }

    /// Appends a predicted next word to the text and closes it with a space.
    func acceptPrediction(_ word: String) {
        let separator = (text.isEmpty || text.last!.isWhitespace) ? "" : " "
        text += separator + word + " "
    }

    private func timed<T>(_ body: () throws -> T) rethrows -> Timed<T> {
        let start = DispatchTime.now()
        let value = try body()
        let us = Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1_000
        return Timed(value: value, microseconds: us)
    }
}

enum TryOutError: Error, CustomStringConvertible {
    case stagedPackMismatch(pack: String, expected: String, actual: String)

    var description: String {
        switch self {
        case .stagedPackMismatch(let pack, let expected, let actual):
            return "Staged pack \(pack).bin does not match packs.json: recorded \(expected.prefix(12))…, file \(actual.prefix(12))…. Re-run scripts/apple/tryout.sh."
        }
    }
}
