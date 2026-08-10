#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

VERSION="0.1.0"
CHECKSUM=""
COMMIT=""
URL=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="$2"
      shift 2
      ;;
    --checksum)
      CHECKSUM="$2"
      shift 2
      ;;
    --commit)
      COMMIT="$2"
      shift 2
      ;;
    --url)
      URL="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "❌ Error: Invalid Apple SDK version: '$VERSION' (must match X.Y.Z semver format)" >&2
  exit 1
fi

if [[ -z "$COMMIT" ]]; then
  COMMIT="$(cd "$REPO_ROOT" && git rev-parse HEAD)"
fi

if [[ -z "$CHECKSUM" && -f "$REPO_ROOT/dist/release-manifest.json" ]]; then
  CHECKSUM=$(grep -o '"swiftpm_checksum": "[^"]*"' "$REPO_ROOT/dist/release-manifest.json" | cut -d'"' -f4 || echo "")
fi

if [[ ! "$CHECKSUM" =~ ^[a-f0-9]{64}$ ]]; then
  echo "❌ Error: A valid 64-character lowercase hex SwiftPM checksum is required to generate Package.swift (got '$CHECKSUM')." >&2
  exit 1
fi

if [[ -z "$URL" ]]; then
  URL="https://raw.githubusercontent.com/Kurdi-Language/kurmanci-swift/${VERSION}/Frameworks/KurmanciFFI-v${VERSION}.xcframework.zip"
fi

DIST_SWIFT="$REPO_ROOT/dist/swift-package"
DIST_SWIFT_LOCAL="$REPO_ROOT/dist/swift-package-local"

echo "=== Generating Release Package in dist/swift-package (v${VERSION}) ==="

rm -rf "$DIST_SWIFT" "$DIST_SWIFT_LOCAL"
mkdir -p "$DIST_SWIFT/Sources" "$DIST_SWIFT/Frameworks" "$DIST_SWIFT_LOCAL/Sources"

# Copy Swift wrapper sources
cp -R "$REPO_ROOT/swift/Sources/Kurmanci" "$DIST_SWIFT/Sources/"
cp -R "$REPO_ROOT/swift/Sources/Kurmanci" "$DIST_SWIFT_LOCAL/Sources/"

# Copy framework zip if present
if [[ -f "$REPO_ROOT/dist/KurmanciFFI-v${VERSION}.xcframework.zip" ]]; then
  cp "$REPO_ROOT/dist/KurmanciFFI-v${VERSION}.xcframework.zip" "$DIST_SWIFT/Frameworks/KurmanciFFI-v${VERSION}.xcframework.zip"
fi

# Copy documentation & licenses
cp "$REPO_ROOT/swift/README.md" "$DIST_SWIFT/README.md"
cp "$REPO_ROOT/LICENSE" "$DIST_SWIFT/LICENSE"
cp "$REPO_ROOT/NOTICE" "$DIST_SWIFT/NOTICE"

cp "$REPO_ROOT/swift/README.md" "$DIST_SWIFT_LOCAL/README.md"
cp "$REPO_ROOT/LICENSE" "$DIST_SWIFT_LOCAL/LICENSE"
cp "$REPO_ROOT/NOTICE" "$DIST_SWIFT_LOCAL/NOTICE"

# Generate Remote Package.swift
cat <<EOF > "$DIST_SWIFT/Package.swift"
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Kurmanci",
    platforms: [
        .macOS(.v11),
        .iOS(.v14)
    ],
    products: [
        .library(name: "Kurmanci", targets: ["Kurmanci"])
    ],
    targets: [
        .binaryTarget(
            name: "KurmanciFFI",
            url: "${URL}",
            checksum: "${CHECKSUM}"
        ),
        .target(
            name: "Kurmanci",
            dependencies: ["KurmanciFFI"]
        )
    ]
)
EOF

# Generate Local Package.swift
cat <<EOF > "$DIST_SWIFT_LOCAL/Package.swift"
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Kurmanci",
    platforms: [
        .macOS(.v11),
        .iOS(.v14)
    ],
    products: [
        .library(name: "Kurmanci", targets: ["Kurmanci"])
    ],
    targets: [
        .binaryTarget(
            name: "KurmanciFFI",
            path: "../artifacts/KurmanciFFI.xcframework"
        ),
        .target(
            name: "Kurmanci",
            dependencies: ["KurmanciFFI"]
        )
    ]
)
EOF

# Generate source-manifest.json
python3 -c '
import os, json, hashlib

repo_root = "'"$REPO_ROOT"'"
dist_swift = "'"$DIST_SWIFT"'"
version = "'"$VERSION"'"
commit = "'"$COMMIT"'"
checksum = "'"$CHECKSUM"'"

files = {}
for root, _, filenames in os.walk(dist_swift):
    for f in filenames:
        p = os.path.join(root, f)
        rel = os.path.relpath(p, dist_swift)
        with open(p, "rb") as fp:
            files[rel] = hashlib.sha256(fp.read()).hexdigest()

manifest = {
    "schema_version": "swift-package-sources-v1",
    "version": version,
    "source_commit": commit,
    "binary_target_checksum": checksum,
    "files": files
}

with open(os.path.join(dist_swift, "source-manifest.json"), "w") as fp:
    json.dump(manifest, fp, indent=2)
'

echo "✅ Distribution package generated under dist/swift-package and dist/swift-package-local"
