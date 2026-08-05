import Foundation
import CKurmanci

public final class KurmanciEngine: @unchecked Sendable {
    private let handle: OpaquePointer
    public let packInfo: PackInfo

    private static let requiredABIMajor: UInt32 = 1
    private static let requiredABIMinor: UInt32 = 0

    private static func verifyABI() throws {
        let major = kmr_abi_version_major()
        let minor = kmr_abi_version_minor()
        guard major == requiredABIMajor,
              minor >= requiredABIMinor else {
            throw KurmanciError.incompatibleAbi(major: major, minor: minor)
        }
    }

    private static func validateCString(_ value: String, name: String) throws {
        guard !value.contains("\0") else {
            throw KurmanciError.invalidArgument("\(name) contains an embedded NUL character")
        }
    }

    private static func readPackInfo(_ handle: OpaquePointer) throws -> PackInfo {
        var info = kmr_pack_info(language_tag: nil, format_version: 0, entry_count: 0)
        let status = kmr_engine_get_info(handle, &info)
        try KurmanciError.check(status)

        let langTagStr: String
        if let tagPtr = info.language_tag {
            langTagStr = String(cString: tagPtr)
        } else {
            langTagStr = ""
        }

        return PackInfo(
            languageTag: langTagStr,
            formatVersion: info.format_version,
            entryCount: info.entry_count
        )
    }

    private static func nativeLimit(_ limit: Int) throws -> Int {
        guard limit >= 0 else {
            throw KurmanciError.invalidArgument("result limit cannot be negative")
        }
        return min(limit, 50)
    }

    public init(packURL: URL) throws {
        try Self.verifyABI()
        guard packURL.isFileURL else {
            throw KurmanciError.invalidArgument("packURL must be a file URL")
        }
        try Self.validateCString(packURL.path, name: "packURL path")

        var enginePtr: OpaquePointer?
        let status = packURL.path.withCString { pathPtr in
            kmr_engine_create_from_file(pathPtr, &enginePtr)
        }
        try KurmanciError.check(status)

        guard let validHandle = enginePtr else {
            throw KurmanciError.nullEnginePointer
        }

        do {
            self.packInfo = try Self.readPackInfo(validHandle)
            self.handle = validHandle
        } catch {
            kmr_engine_destroy(validHandle)
            throw error
        }
    }

    public init(packData: Data) throws {
        try Self.verifyABI()

        guard !packData.isEmpty else {
            throw KurmanciError.invalidArgument("pack data cannot be empty")
        }

        var enginePtr: OpaquePointer?
        let status = packData.withUnsafeBytes { rawBuffer in
            let bytePtr = rawBuffer.bindMemory(to: UInt8.self).baseAddress
            return kmr_engine_create_from_bytes(bytePtr, packData.count, &enginePtr)
        }
        try KurmanciError.check(status)

        guard let validHandle = enginePtr else {
            throw KurmanciError.nullEnginePointer
        }

        do {
            self.packInfo = try Self.readPackInfo(validHandle)
            self.handle = validHandle
        } catch {
            kmr_engine_destroy(validHandle)
            throw error
        }
    }

    deinit {
        kmr_engine_destroy(handle)
    }

    public func isKnownWord(_ word: String) throws -> Bool {
        try Self.validateCString(word, name: "word")
        var isKnown = false
        let status = word.withCString { cWord in
            kmr_engine_is_known_word(handle, cWord, &isKnown)
        }
        try KurmanciError.check(status)
        return isKnown
    }

    private func querySuggestions(
        operation: (OpaquePointer, UnsafePointer<CChar>, Int, UnsafeMutablePointer<OpaquePointer?>) -> kmr_status,
        input: String,
        name: String,
        limit: Int
    ) throws -> [Suggestion] {
        try Self.validateCString(input, name: name)
        let nLimit = try Self.nativeLimit(limit)
        var resultsPtr: OpaquePointer?

        let status = input.withCString { cInput in
            operation(handle, cInput, nLimit, &resultsPtr)
        }
        try KurmanciError.check(status)

        guard let validResults = resultsPtr else {
            throw KurmanciError.internalError("native API returned no suggestion result handle")
        }
        defer {
            kmr_suggestion_list_destroy(validResults)
        }

        var len: Int = 0
        let lenStatus = kmr_suggestion_list_len(validResults, &len)
        try KurmanciError.check(lenStatus)

        var suggestions = [Suggestion]()
        suggestions.reserveCapacity(len)

        for i in 0..<len {
            var item = kmr_suggestion_item(text: nil, kind: 0, edit_cost: 0)
            let getStatus = kmr_suggestion_list_get(validResults, i, &item)
            try KurmanciError.check(getStatus)

            guard let textPtr = item.text else {
                throw KurmanciError.internalError("suggestion item contains null text pointer")
            }
            let textStr = String(cString: textPtr)
            suggestions.append(Suggestion(
                text: textStr,
                kind: SuggestionKind(cValue: item.kind),
                editCost: item.edit_cost
            ))
        }

        return suggestions
    }

    public func correct(_ input: String, limit: Int = 5) throws -> [Suggestion] {
        return try querySuggestions(operation: kmr_engine_correct, input: input, name: "correction input", limit: limit)
    }

    public func complete(_ prefix: String, limit: Int = 5) throws -> [Suggestion] {
        return try querySuggestions(operation: kmr_engine_complete, input: prefix, name: "completion prefix", limit: limit)
    }

    public func suggest(_ input: String, limit: Int = 5) throws -> [Suggestion] {
        return try querySuggestions(operation: kmr_engine_suggest, input: input, name: "suggestion input", limit: limit)
    }

    public func predictNext(context: [String], limit: Int = 5) throws -> [Prediction] {
        let nLimit = try Self.nativeLimit(limit)
        let effectiveContext = Array(context.suffix(2))
        for word in effectiveContext {
            try Self.validateCString(word, name: "context word")
        }

        var resultsPtr: OpaquePointer?

        let status: kmr_status
        if effectiveContext.isEmpty {
            status = kmr_engine_predict_next(handle, nil, 0, nLimit, &resultsPtr)
        } else {
            status = withCStringArray(effectiveContext) { cPtrs in
                return kmr_engine_predict_next(handle, cPtrs, effectiveContext.count, nLimit, &resultsPtr)
            }
        }
        try KurmanciError.check(status)

        guard let validResults = resultsPtr else {
            throw KurmanciError.internalError("native API returned no prediction result handle")
        }
        defer {
            kmr_prediction_list_destroy(validResults)
        }

        var len: Int = 0
        let lenStatus = kmr_prediction_list_len(validResults, &len)
        try KurmanciError.check(lenStatus)

        var predictions = [Prediction]()
        predictions.reserveCapacity(len)

        for i in 0..<len {
            var item = kmr_prediction_item(text: nil, count: 0, probability_millionths: 0, source: 0)
            let getStatus = kmr_prediction_list_get(validResults, i, &item)
            try KurmanciError.check(getStatus)

            guard let textPtr = item.text else {
                throw KurmanciError.internalError("prediction item contains null text pointer")
            }
            let textStr = String(cString: textPtr)
            predictions.append(Prediction(
                text: textStr,
                count: item.count,
                probabilityMillionths: item.probability_millionths,
                source: PredictionSource(cValue: item.source)
            ))
        }

        return predictions
    }
}

private func withCStringArray<R>(
    _ strings: [String],
    _ body: (UnsafePointer<UnsafePointer<CChar>?>) throws -> R
) rethrows -> R {
    func helper(
        _ index: Int,
        _ cPtrs: inout [UnsafePointer<CChar>?],
        _ body: (UnsafePointer<UnsafePointer<CChar>?>) throws -> R
    ) rethrows -> R {
        if index == strings.count {
            return try cPtrs.withUnsafeBufferPointer { buf in
                return try body(buf.baseAddress!)
            }
        } else {
            return try strings[index].withCString { cStr in
                cPtrs.append(cStr)
                return try helper(index + 1, &cPtrs, body)
            }
        }
    }

    var cPtrs = [UnsafePointer<CChar>?]()
    cPtrs.reserveCapacity(strings.count)
    return try helper(0, &cPtrs, body)
}
