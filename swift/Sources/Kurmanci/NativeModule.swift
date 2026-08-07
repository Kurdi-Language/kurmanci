import Foundation

#if canImport(KurmanciFFI)
@_exported import KurmanciFFI
#elseif canImport(CKurmanci)
@_exported import CKurmanci
#else
#error("No Kurmancî C ABI module is available")
#endif
