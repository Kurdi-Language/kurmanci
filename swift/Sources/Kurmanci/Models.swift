import Foundation

public enum SuggestionKind: Sendable, Equatable, Hashable {
    case exact
    case completion
    case correction
    case diacriticCorrection
    case nextWord
    case unknown(UInt32)

    public init(cValue: UInt32) {
        switch cValue {
        case KMR_SUGGESTION_EXACT:
            self = .exact
        case KMR_SUGGESTION_COMPLETION:
            self = .completion
        case KMR_SUGGESTION_CORRECTION:
            self = .correction
        case KMR_SUGGESTION_DIACRITIC_CORRECTION:
            self = .diacriticCorrection
        case KMR_SUGGESTION_NEXT_WORD:
            self = .nextWord
        default:
            self = .unknown(cValue)
        }
    }
}

public enum PredictionSource: Sendable, Equatable, Hashable {
    case trigram
    case bigramBackoff
    case bigram
    case none
    case unknown(UInt32)

    public init(cValue: UInt32) {
        switch cValue {
        case KMR_PREDICTION_TRIGRAM:
            self = .trigram
        case KMR_PREDICTION_BIGRAM_BACKOFF:
            self = .bigramBackoff
        case KMR_PREDICTION_BIGRAM:
            self = .bigram
        case KMR_PREDICTION_NONE:
            self = .none
        default:
            self = .unknown(cValue)
        }
    }
}

public struct PackInfo: Sendable, Equatable {
    public let languageTag: String
    public let formatVersion: UInt32
    public let entryCount: Int

    public init(languageTag: String, formatVersion: UInt32, entryCount: Int) {
        self.languageTag = languageTag
        self.formatVersion = formatVersion
        self.entryCount = entryCount
    }
}

public struct Suggestion: Sendable, Equatable {
    public let text: String
    public let kind: SuggestionKind
    public let editCost: UInt32

    public init(text: String, kind: SuggestionKind, editCost: UInt32) {
        self.text = text
        self.kind = kind
        self.editCost = editCost
    }
}

public struct Prediction: Sendable, Equatable {
    public let text: String
    public let count: UInt64
    public let probabilityMillionths: UInt32
    public let source: PredictionSource

    public init(text: String, count: UInt64, probabilityMillionths: UInt32, source: PredictionSource) {
        self.text = text
        self.count = count
        self.probabilityMillionths = probabilityMillionths
        self.source = source
    }
}
