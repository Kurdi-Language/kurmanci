import Foundation
import Kurmanci

guard CommandLine.arguments.count >= 2 else {
    print("Usage: KurmanciExample <path_to_lexicon.bin>")
    exit(1)
}

let packPath = CommandLine.arguments[1]
let packURL = URL(fileURLWithPath: packPath)

do {
    print("Loading Kurmancî engine from \(packPath)...")
    let engine = try KurmanciEngine(packURL: packURL)

    let info = engine.packInfo
    print("✅ Loaded pack (tag: \(info.languageTag), format v\(info.formatVersion), entries: \(info.entryCount))")

    let isKnown = try engine.isKnownWord("welat")
    print("✅ Is 'welat' known? \(isKnown ? "yes" : "no")")

    let suggestions = try engine.suggest("spaz", limit: 5)
    print("✅ Suggestions for 'spaz': \(suggestions.count) candidates")
    for (i, sug) in suggestions.enumerated() {
        print("  \(i + 1). \(sug.text) (editCost: \(sug.editCost), kind: \(sug.kind))")
    }

    let predictions = try engine.predictNext(context: ["ez", "diçim"], limit: 5)
    print("✅ Predictions following ['ez', 'diçim']: \(predictions.count) candidates")
    for (i, pred) in predictions.enumerated() {
        print("  \(i + 1). \(pred.text) (source: \(pred.source), count: \(pred.count))")
    }

    print("⚡ Kurmancî Swift SDK Example completed successfully!")
} catch {
    fputs("Error: \(error)\n", stderr)
    exit(1)
}
