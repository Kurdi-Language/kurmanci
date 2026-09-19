#!/usr/bin/env bash
# Vendor evaluation kit driver: fetch and verify a published Kurmancî release, then run the
# device benchmark harnesses against the published SDK artifacts, without a Rust toolchain.
#
# Usage:
#   scripts/vendor/evaluate.sh fetch   --version V [--dir DIR]
#       Downloads the release bundle kurmanci-ku-Latn-V.tar.gz from the GitHub release vV,
#       checks it against the published .sha256, unpacks it, checks the exact file set against
#       the bundle's SHA256SUMS (every listed file present and matching, no unlisted regular
#       file), checks that the bundle's VERSION and provenance release_version are V, then
#       downloads the Android AAR from Maven Central and the Apple XCFramework from the GitHub
#       release swift-vV and checks both against the platform-artifact records of the bundle's
#       provenance.json (platform and path, not just file name).
#   scripts/vendor/evaluate.sh android --version V [--dir DIR] [--pack reviewed|experimental-full]
#       Runs the Android device benchmark (integration/android/android-consumer,
#       DeviceBenchmarkTest) on the device or emulator visible to adb, resolving the SDK from
#       Maven Central (CONSUMER_MODE=public) at version V, with the chosen pack from the bundle.
#   scripts/vendor/evaluate.sh ios     --version V [--dir DIR] [--pack reviewed|experimental-full] [--destination DEST]
#       Runs the iOS device benchmark (integration/apple/ios-remote-consumer,
#       DeviceBenchmarkTests) on a simulator (default) or a real iPhone, resolving the SDK from
#       the published Kurdi-Language/kurmanci-swift package. The remote consumer project must
#       require exactly version V and its Package.resolved must pin V, otherwise the run is
#       refused before xcodebuild starts: V's pack is never measured through another SDK
#       version. For a real iPhone pass --destination "platform=iOS,id=<UDID>" and set
#       XCODEBUILD_EXTRA_ARGS="DEVELOPMENT_TEAM=<team id> -allowProvisioningUpdates".
#
# DIR defaults to dist/vendor-evaluation. Reports land in DIR/reports. Needs curl, tar,
# python3 and shasum or sha256sum; Android needs JDK 17, the Android SDK and adb; iOS needs
# Xcode. Nothing here changes the repository's data or decisions. For testing, the download
# locations can be overridden with KURMANCI_RELEASE_BASE_URL (default
# https://github.com/Kurdi-Language/kurmanci/releases/download) and KURMANCI_MAVEN_BASE_URL
# (default https://repo1.maven.org/maven2); file:// URLs are accepted; and
# KURMANCI_REMOTE_CONSUMER_DIR points the iOS version check at a copy of the remote consumer.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RELEASE_BASE_URL="${KURMANCI_RELEASE_BASE_URL:-https://github.com/Kurdi-Language/kurmanci/releases/download}"
MAVEN_BASE_URL="${KURMANCI_MAVEN_BASE_URL:-https://repo1.maven.org/maven2}"
MAVEN_GROUP_PATH="io/github/ferhatguneri"
MAVEN_ARTIFACT="kurmanci-android"
REMOTE_CONSUMER_DIR="${KURMANCI_REMOTE_CONSUMER_DIR:-$REPO_ROOT/integration/apple/ios-remote-consumer}"

COMMAND="${1:-}"
[[ -n "$COMMAND" ]] && shift || true
VERSION=""
DIR="$REPO_ROOT/dist/vendor-evaluation"
PACK="reviewed"
DEST=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dir) DIR="$2"; shift 2 ;;
    --pack) PACK="$2"; shift 2 ;;
    --destination) DEST="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

usage() { sed -n '2,35p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

case "$COMMAND" in
  fetch|android|ios) ;;
  ""|-h|--help) usage; exit 0 ;;
  *) echo "❌ unknown command '$COMMAND'" >&2; usage >&2; exit 1 ;;
esac
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "❌ --version X.Y.Z is required" >&2; exit 1; }
case "$PACK" in reviewed|experimental-full|seed) ;; *) echo "❌ --pack must be reviewed, experimental-full or seed" >&2; exit 1 ;; esac

BUNDLE_NAME="kurmanci-ku-Latn-$VERSION"
BUNDLE_DIR="$DIR/$BUNDLE_NAME"
REPORTS="$DIR/reports"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else echo "❌ neither shasum nor sha256sum found" >&2; exit 1; fi
}

download() {
  local url="$1" out="$2"
  echo "  ← $url"
  curl -fsSL --retry 3 --retry-delay 2 -o "$out" "$url" || { echo "❌ download failed: $url" >&2; exit 1; }
}

# Verifies the extracted bundle fail-closed: the set of regular files must be exactly the
# paths listed in SHA256SUMS plus SHA256SUMS itself, every listed file must hash as listed,
# and the release identity (requested version, VERSION file, provenance release_version when
# present) must agree. Prints the bundle summary on success.
verify_bundle() {
  python3 - "$BUNDLE_DIR" "$VERSION" <<'EOF'
import hashlib, json, os, sys
root, requested = sys.argv[1], sys.argv[2]
name = os.path.basename(root)
def fail(msg):
    print(f"❌ {name}: {msg}", file=sys.stderr); sys.exit(1)
sums_path = os.path.join(root, "SHA256SUMS")
if not os.path.isfile(sums_path):
    fail("SHA256SUMS missing after extraction")
listed = {}
for line in open(sums_path, encoding="utf-8"):
    line = line.rstrip("\n")
    if not line:
        continue
    if "  " not in line:
        fail(f"malformed SHA256SUMS line: {line!r}")
    digest, path = line.split("  ", 1)
    if path in listed:
        fail(f"SHA256SUMS lists {path} twice")
    if path == "SHA256SUMS" or path.startswith("/") or ".." in path.split("/"):
        fail(f"SHA256SUMS lists an invalid path: {path}")
    listed[path] = digest
present = set()
for dirpath, dirnames, filenames in os.walk(root):
    for d in dirnames:
        if os.path.islink(os.path.join(dirpath, d)):
            fail(f"symbolic link in bundle: {os.path.relpath(os.path.join(dirpath, d), root)}")
    for f in filenames:
        full = os.path.join(dirpath, f)
        rel = os.path.relpath(full, root)
        if os.path.islink(full) or not os.path.isfile(full):
            fail(f"not a regular file: {rel}")
        present.add(rel)
present.discard("SHA256SUMS")
missing = sorted(set(listed) - present)
unlisted = sorted(present - set(listed))
if missing:
    fail(f"listed in SHA256SUMS but absent: {', '.join(missing)}")
if unlisted:
    fail(f"regular file(s) not listed in SHA256SUMS: {', '.join(unlisted)}")
for path, digest in sorted(listed.items()):
    h = hashlib.sha256()
    with open(os.path.join(root, path), "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    if h.hexdigest() != digest:
        fail(f"{path} does not match its SHA256SUMS entry")
version_file = open(os.path.join(root, "VERSION"), encoding="utf-8").read().strip()
if version_file != requested:
    fail(f"VERSION is {version_file!r} but {requested!r} was requested")
prov = json.load(open(os.path.join(root, "provenance.json"), encoding="utf-8"))
if "release_version" in prov and prov["release_version"] != requested:
    fail(f"provenance.json release_version is {prov['release_version']!r} but {requested!r} was requested")
compat = json.load(open(os.path.join(root, "compatibility.json"), encoding="utf-8"))
identity = hashlib.sha256(open(sums_path, "rb").read()).hexdigest()
print(f"✅ exact file set verified: {len(listed)} files listed in SHA256SUMS, all present and matching, no unlisted file; SHA256SUMS sha256 {identity} is the identity of this bundle")
print(f"✅ release identity {requested}: VERSION and provenance agree")
abi = compat["c_abi_version"]
print(f"   release {version_file} ({prov['release_kind']}), source commit {prov['source']['commit']}, data tree {prov['source']['data_tree']}")
print(f"   engine {compat['engine_version']}, C ABI {abi['major']}.{abi['minor']}, pack schema {compat['pack_schema_version']}, language-model schema {compat['language_model_schema_version']}, language {compat['language_tag']}")
for pack in prov["packs"]:
    print(f"   pack {pack['pack_id']}: {pack['entry_count']} entries, model profile {pack['model_profile']}")
if prov.get("evaluation_notice"):
    print(f"   notice: {prov['evaluation_notice']}")
EOF
}

# Compares a downloaded platform artifact with the provenance record of the artifact the
# bundle attached under exactly this platform and path. Fails closed when the bundle recorded
# no such artifact, or more than one: the published file then cannot be tied to the release.
check_against_provenance() {
  local file="$1" platform="$2" expected_path="$3" label="$4"
  local actual; actual="$(sha256_of "$file")"
  local recorded
  recorded="$(python3 - "$BUNDLE_DIR/provenance.json" "$platform" "$expected_path" <<'EOF'
import json, sys
prov = json.load(open(sys.argv[1]))
platform, path = sys.argv[2], sys.argv[3]
matches = [a for a in prov.get("platform_artifacts", []) if a.get("platform") == platform and a.get("path") == path]
if len(matches) == 1:
    print(matches[0]["sha256"])
elif len(matches) > 1:
    print("AMBIGUOUS")
EOF
)"
  if [[ -z "$recorded" ]]; then
    echo "❌ $label: provenance.json of $BUNDLE_NAME does not record an attached $platform artifact at $expected_path" >&2
    exit 1
  fi
  if [[ "$recorded" == "AMBIGUOUS" ]]; then
    echo "❌ $label: provenance.json of $BUNDLE_NAME records more than one $platform artifact at $expected_path" >&2
    exit 1
  fi
  if [[ "$actual" != "$recorded" ]]; then
    echo "❌ $label: sha256 $actual differs from the hash provenance.json recorded for $expected_path ($recorded)" >&2
    exit 1
  fi
  echo "✅ $label matches provenance.json ($expected_path): $actual"
}

do_fetch() {
  mkdir -p "$DIR" "$REPORTS"
  local release="$RELEASE_BASE_URL/v$VERSION"
  local tarball="$DIR/$BUNDLE_NAME.tar.gz"
  echo "== release bundle $BUNDLE_NAME"
  download "$release/$BUNDLE_NAME.tar.gz" "$tarball"
  download "$release/$BUNDLE_NAME.tar.gz.sha256" "$tarball.sha256"
  local expected actual
  expected="$(awk '{print $1}' "$tarball.sha256")"
  actual="$(sha256_of "$tarball")"
  [[ "$expected" == "$actual" ]] || { echo "❌ bundle tarball sha256 $actual differs from published $expected" >&2; exit 1; }
  echo "✅ tarball sha256 $actual"
  rm -rf "$BUNDLE_DIR"
  tar -xzf "$tarball" -C "$DIR"
  [[ -d "$BUNDLE_DIR" ]] || { echo "❌ tarball did not contain $BUNDLE_NAME/" >&2; exit 1; }
  verify_bundle

  echo "== Android SDK $MAVEN_ARTIFACT $VERSION from Maven Central"
  mkdir -p "$DIR/sdk/android"
  local maven="$MAVEN_BASE_URL/$MAVEN_GROUP_PATH/$MAVEN_ARTIFACT/$VERSION"
  download "$maven/$MAVEN_ARTIFACT-$VERSION.aar" "$DIR/sdk/android/$MAVEN_ARTIFACT-$VERSION.aar"
  download "$maven/$MAVEN_ARTIFACT-$VERSION.pom" "$DIR/sdk/android/$MAVEN_ARTIFACT-$VERSION.pom"
  check_against_provenance "$DIR/sdk/android/$MAVEN_ARTIFACT-$VERSION.aar" android "android/$MAVEN_ARTIFACT-$VERSION.aar" "Maven Central AAR"

  echo "== Apple SDK KurmanciFFI v$VERSION from the GitHub release swift-v$VERSION"
  mkdir -p "$DIR/sdk/apple"
  download "$RELEASE_BASE_URL/swift-v$VERSION/KurmanciFFI-v$VERSION.xcframework.zip" "$DIR/sdk/apple/KurmanciFFI-v$VERSION.xcframework.zip"
  check_against_provenance "$DIR/sdk/apple/KurmanciFFI-v$VERSION.xcframework.zip" apple "apple/KurmanciFFI-v$VERSION.xcframework.zip" "XCFramework"
  echo "✅ fetch complete: $BUNDLE_DIR, $DIR/sdk"
}

require_bundle() {
  [[ -f "$BUNDLE_DIR/packs/$PACK/lexicon.bin" ]] || { echo "❌ $BUNDLE_DIR/packs/$PACK/lexicon.bin not found; run 'fetch --version $VERSION' first" >&2; exit 1; }
}

do_android() {
  require_bundle
  mkdir -p "$REPORTS"
  echo "== Android device benchmark, pack $PACK, SDK $MAVEN_ARTIFACT:$VERSION from Maven Central"
  CONSUMER_MODE=public VERSION="$VERSION" "$REPO_ROOT/scripts/android/device-benchmark.sh" \
    --pack "$BUNDLE_DIR/packs/$PACK/lexicon.bin" --out "$REPORTS"
}

# The remote consumer's Xcode project pins the published Swift package at one exact version,
# and its Package.resolved records what was resolved. Both must be exactly V before anything
# is built: measuring V's pack through another SDK version would not be an evaluation of V.
require_remote_consumer_version() {
  local pbxproj="$REMOTE_CONSUMER_DIR/KurmanciConsumer.xcodeproj/project.pbxproj"
  local resolved="$REMOTE_CONSUMER_DIR/KurmanciConsumer.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"
  [[ -f "$pbxproj" ]] || { echo "❌ remote consumer project not found: $pbxproj" >&2; exit 1; }
  [[ -f "$resolved" ]] || { echo "❌ remote consumer Package.resolved not found: $resolved" >&2; exit 1; }
  python3 - "$pbxproj" "$resolved" "$VERSION" <<'EOF'
import json, re, sys
pbxproj, resolved, requested = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(pbxproj, encoding="utf-8").read()
block = re.search(r'XCRemoteSwiftPackageReference "kurmanci-swift".*?requirement = \{(.*?)\};', text, re.S)
if not block:
    print("❌ remote consumer: no kurmanci-swift package requirement found in project.pbxproj", file=sys.stderr); sys.exit(1)
kind = re.search(r'kind = (\w+);', block.group(1))
version = re.search(r'version = "?([0-9]+\.[0-9]+\.[0-9]+)"?;', block.group(1))
if not kind or kind.group(1) != "exactVersion" or not version:
    print(f"❌ remote consumer: the kurmanci-swift requirement is not an exact version ({block.group(1).strip()})", file=sys.stderr); sys.exit(1)
if version.group(1) != requested:
    print(f"❌ remote consumer requires kurmanci-swift {version.group(1)}, but --version {requested} was requested; refusing to measure {requested}'s pack through SDK {version.group(1)}", file=sys.stderr); sys.exit(1)
pins = [p for p in json.load(open(resolved, encoding="utf-8")).get("pins", []) if p.get("identity") == "kurmanci-swift"]
if len(pins) != 1:
    print("❌ remote consumer: Package.resolved does not pin kurmanci-swift exactly once", file=sys.stderr); sys.exit(1)
pinned = pins[0].get("state", {}).get("version")
if pinned != requested:
    print(f"❌ remote consumer Package.resolved pins kurmanci-swift {pinned}, but --version {requested} was requested; refusing to measure {requested}'s pack through SDK {pinned}", file=sys.stderr); sys.exit(1)
print(f"✅ remote consumer requires and resolves kurmanci-swift {requested} (revision {pins[0]['state'].get('revision', '?')})")
EOF
}

do_ios() {
  require_bundle
  require_remote_consumer_version
  mkdir -p "$REPORTS"
  echo "== iOS device benchmark, pack $PACK, published Swift package kurmanci-swift $VERSION (remote consumer)"
  local args=(--project remote --pack "$BUNDLE_DIR/packs/$PACK/lexicon.bin" --out "$REPORTS")
  [[ -n "$DEST" ]] && args+=(--destination "$DEST")
  # Own derived data and package cache per evaluation directory: Xcode's shared DerivedData
  # keeps precompiled modules of whichever package version was resolved last, and a stale
  # module for a different release fails the build instead of measuring anything.
  XCODEBUILD_EXTRA_ARGS="${XCODEBUILD_EXTRA_ARGS:-} -derivedDataPath $DIR/xcode-derived-data -clonedSourcePackagesDirPath $DIR/xcode-packages" \
    "$REPO_ROOT/scripts/apple/device-benchmark.sh" "${args[@]}"
}

case "$COMMAND" in
  fetch) do_fetch ;;
  android) do_android ;;
  ios) do_ios ;;
esac
