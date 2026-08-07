import Darwin
import KurmanciFFI

print("Testing direct C module import 'import KurmanciFFI'...")
let major = kmr_abi_version_major()
let minor = kmr_abi_version_minor()
guard major == 1, minor >= 0 else {
    print("❌ Incompatible ABI: \(major).\(minor)")
    exit(1)
}
print("✅ Direct C module import verified (ABI v\(major).\(minor))")
