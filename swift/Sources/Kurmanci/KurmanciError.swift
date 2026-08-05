import Foundation
import CKurmanci

public enum KurmanciError: Error, Equatable, CustomStringConvertible {
    case invalidArgument(String)
    case ioError(String)
    case invalidPack(String)
    case unsupportedPack(String)
    case incompatibleLanguage(String)
    case checksumMismatch(String)
    case internalError(String)
    case unknownStatus(code: UInt32, message: String)
    case incompatibleAbi(major: UInt32, minor: UInt32)
    case nullEnginePointer

    public var description: String {
        switch self {
        case .invalidArgument(let msg):
            return "Invalid argument: \(msg)"
        case .ioError(let msg):
            return "I/O error: \(msg)"
        case .invalidPack(let msg):
            return "Invalid pack: \(msg)"
        case .unsupportedPack(let msg):
            return "Unsupported pack: \(msg)"
        case .incompatibleLanguage(let msg):
            return "Incompatible language: \(msg)"
        case .checksumMismatch(let msg):
            return "Checksum mismatch: \(msg)"
        case .internalError(let msg):
            return "Internal error: \(msg)"
        case .unknownStatus(let code, let msg):
            return "Unknown status error \(code): \(msg)"
        case .incompatibleAbi(let major, let minor):
            return "Incompatible C ABI version v\(major).\(minor) (expected v\(KMR_ABI_VERSION_MAJOR).\(KMR_ABI_VERSION_MINOR))"
        case .nullEnginePointer:
            return "Engine handle pointer was null"
        }
    }

    public static func lastErrorMessage() -> String {
        guard let pointer = kmr_last_error_message() else {
            return ""
        }
        return String(cString: pointer)
    }

    public static func check(_ status: kmr_status) throws {
        guard status != KMR_OK else {
            return
        }

        let msg = lastErrorMessage()
        switch status {
        case KMR_ERROR_INVALID_ARGUMENT:
            throw KurmanciError.invalidArgument(msg)
        case KMR_ERROR_IO:
            throw KurmanciError.ioError(msg)
        case KMR_ERROR_INVALID_PACK:
            throw KurmanciError.invalidPack(msg)
        case KMR_ERROR_UNSUPPORTED_PACK:
            throw KurmanciError.unsupportedPack(msg)
        case KMR_ERROR_INCOMPATIBLE_LANGUAGE:
            throw KurmanciError.incompatibleLanguage(msg)
        case KMR_ERROR_CHECKSUM:
            throw KurmanciError.checksumMismatch(msg)
        case KMR_ERROR_INTERNAL:
            throw KurmanciError.internalError(msg)
        default:
            throw KurmanciError.unknownStatus(code: status, message: msg)
        }
    }
}
