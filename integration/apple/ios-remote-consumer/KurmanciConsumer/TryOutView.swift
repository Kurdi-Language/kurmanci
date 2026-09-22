import SwiftUI
import Kurmanci

/// Internal try-out screen of the remote consumer host (see `docs/ios-tryout.md`): a text
/// field, the engine's answers for the word being typed, the next-word predictions for the
/// words before it, and the cost of every call. Tapping an answer inserts it. This is an
/// integration harness for a person holding the phone, not a keyboard.
struct TryOutView: View {
    @StateObject private var model = TryOutModel()

    var body: some View {
        NavigationView {
            List {
                packSection
                inputSection
                wordSection
                predictionSection
                provenanceSection
            }
            .listStyle(.insetGrouped)
            .navigationTitle("Kurmancî try-out")
        }
        .navigationViewStyle(.stack)
    }

    // MARK: Sections

    private var packSection: some View {
        Section(header: Text("Pack")) {
            if model.staging.packs.count > 1 {
                Picker("Pack", selection: $model.selectedPackID) {
                    ForEach(model.staging.packs) { pack in
                        Text(pack.id).tag(pack.id)
                    }
                }
                .pickerStyle(.segmented)
            }
            if model.isLoading {
                HStack(spacing: 8) {
                    ProgressView()
                    Text("Loading \(model.selectedPackID)…").foregroundColor(.secondary)
                }
            } else if let error = model.loadError {
                Text(error).foregroundColor(.red).font(.footnote)
            } else if let loaded = model.loaded {
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(loaded.info.entryCount.formatted()) entries · format \(loaded.info.formatVersion) · \(loaded.info.languageTag)")
                    Text("\(Self.megabytes(loaded.bytes)) MB · loaded in \(loaded.loadMilliseconds, specifier: "%.1f") ms")
                        .foregroundColor(.secondary)
                    Text("sha256 \(loaded.sha256.prefix(16))…")
                        .font(.footnote.monospaced())
                        .foregroundColor(.secondary)
                }
                .font(.footnote)
            }
        }
    }

    private var inputSection: some View {
        Section(header: Text("Type"), footer: Text("The system keyboard's own correction is off so only the engine's answers show. Tap an answer to insert it.")) {
            TextField("Type Kurmancî…", text: $model.text)
                .textInputAutocapitalization(.never)
                .disableAutocorrection(true)
                .font(.title3)
            if !model.text.isEmpty {
                Button("Clear", role: .destructive) { model.text = "" }
                    .font(.footnote)
            }
            if let error = model.queryError {
                Text(error).foregroundColor(.red).font(.footnote)
            }
        }
    }

    @ViewBuilder
    private var wordSection: some View {
        if let result = model.wordResult {
            Section(header: Text("Word being typed: \(result.word)")) {
                HStack {
                    Text("Known")
                    Spacer()
                    Text(result.known.value ? "yes" : "no")
                        .foregroundColor(result.known.value ? .green : .secondary)
                    latency(result.known.microseconds)
                }
                suggestionRow("Suggest", result.suggestions)
                suggestionRow("Complete", result.completions)
                suggestionRow("Correct", result.corrections)
            }
        }
    }

    @ViewBuilder
    private var predictionSection: some View {
        if let result = model.predictionResult {
            Section(header: Text("Next word after: \(result.context.joined(separator: " "))")) {
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text("Predict")
                        Spacer()
                        Text("\(result.predictions.value.count) result\(result.predictions.value.count == 1 ? "" : "s")")
                            .foregroundColor(.secondary)
                        latency(result.predictions.microseconds)
                    }
                    if result.predictions.value.isEmpty {
                        Text("no prediction").foregroundColor(.secondary).font(.footnote)
                    } else {
                        chips(result.predictions.value.map { prediction in
                            Chip(text: prediction.text,
                                 detail: "\(Self.label(prediction.source)) · \(prediction.count) · \(Self.percent(prediction.probabilityMillionths))")
                        }) { model.acceptPrediction($0) }
                    }
                }
            }
        }
    }

    private var provenanceSection: some View {
        Section(footer: Text("Internal integration harness of the Kurmancî project, not a keyboard. Packs staged by scripts/apple/tryout.sh from a published release bundle; the SDK is the published kurmanci-swift package this project pins.")) {
            if let version = model.staging.releaseVersion {
                row("Release", version)
            }
            if let sdk = model.staging.sdkVersion {
                row("kurmanci-swift", sdk)
            }
            if let identity = model.staging.bundleIdentity {
                row("Bundle identity", String(identity.prefix(16)) + "…", monospaced: true)
            }
        }
    }

    // MARK: Pieces

    private struct Chip: Identifiable {
        let text: String
        let detail: String
        var id: String { text + "|" + detail }
    }

    private func suggestionRow(_ title: String, _ timed: TryOutModel.Timed<[Suggestion]>) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(title)
                Spacer()
                Text("\(timed.value.count) result\(timed.value.count == 1 ? "" : "s")")
                    .foregroundColor(.secondary)
                latency(timed.microseconds)
            }
            if timed.value.isEmpty {
                Text("none").foregroundColor(.secondary).font(.footnote)
            } else {
                chips(timed.value.map { suggestion in
                    Chip(text: suggestion.text, detail: "\(Self.label(suggestion.kind)) · cost \(suggestion.editCost)")
                }) { model.acceptForCurrentWord($0) }
            }
        }
    }

    private func chips(_ items: [Chip], onTap: @escaping (String) -> Void) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(items) { item in
                    Button { onTap(item.text) } label: {
                        VStack(spacing: 2) {
                            Text(item.text).font(.body.weight(.medium))
                            Text(item.detail).font(.caption2).foregroundColor(.secondary)
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(Color.accentColor.opacity(0.12))
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.vertical, 2)
        }
    }

    private func latency(_ microseconds: Double) -> some View {
        Text("\(microseconds, specifier: "%.0f") µs")
            .font(.footnote.monospacedDigit())
            .foregroundColor(.secondary)
            .frame(minWidth: 64, alignment: .trailing)
    }

    private func row(_ title: String, _ value: String, monospaced: Bool = false) -> some View {
        HStack {
            Text(title)
            Spacer()
            Text(value)
                .font(monospaced ? .footnote.monospaced() : .footnote)
                .foregroundColor(.secondary)
        }
    }

    // MARK: Labels

    private static func label(_ kind: SuggestionKind) -> String {
        switch kind {
        case .exact: return "exact"
        case .completion: return "completion"
        case .correction: return "correction"
        case .diacriticCorrection: return "diacritic"
        case .nextWord: return "next word"
        case .unknown(let raw): return "kind \(raw)"
        }
    }

    private static func label(_ source: PredictionSource) -> String {
        switch source {
        case .trigram: return "trigram"
        case .bigramBackoff: return "backoff"
        case .bigram: return "bigram"
        case .none: return "none"
        case .unknown(let raw): return "source \(raw)"
        }
    }

    private static func percent(_ millionths: UInt32) -> String {
        String(format: "%.1f%%", Double(millionths) / 10_000)
    }

    private static func megabytes(_ bytes: Int) -> String {
        String(format: "%.1f", Double(bytes) / 1_048_576)
    }
}
