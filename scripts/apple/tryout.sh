#!/usr/bin/env bash
# Puts the iOS try-out screen (integration/apple/ios-remote-consumer, docs/ios-tryout.md) on an
# iPhone or a simulator with the packs of a published release, against the published Swift
# package the project pins. Nothing is built from Rust.
#
# Usage:
#   scripts/apple/tryout.sh --version V [--dir DIR] [--device UDID | --simulator] [--team TEAMID] [--stage-only]
#
#   --version V    The published release whose packs are staged. The bundle
#                  kurmanci-ku-Latn-V is taken from DIR (default dist/vendor-evaluation) and
#                  fetched and verified through scripts/vendor/evaluate.sh when absent.
#   --device UDID  Install on this physical iPhone (xcrun devicectl). Default: the one physical
#                  device devicectl reports as connected; refused when there is none or several.
#   --simulator    Build unsigned for the first available iPhone simulator, install and launch it.
#   --team TEAMID  Apple Development team for automatic signing on a physical device (also
#                  DEVELOPMENT_TEAM in the environment). Required for --device.
#   --stage-only   Stage the packs and stop before xcodebuild.
#
# Steps, each fail-closed:
#   1. The bundle's VERSION is V; packs/reviewed/lexicon.bin and
#      packs/experimental-full/lexicon.bin hash exactly as the bundle's SHA256SUMS lists them.
#   2. The remote consumer project requires kurmanci-swift at exactly V and its
#      Package.resolved pins V: V's packs are never tried through another SDK version.
#   3. The packs are copied to integration/apple/ios-remote-consumer/Packs/<pack_id>.bin and
#      Packs/packs.json records the release, the bundle identity (sha256 of SHA256SUMS), the
#      SDK version and each pack's sha256; the screen re-hashes every pack it loads and refuses
#      one that differs. Packs/ is a folder reference copied into the app bundle and is
#      gitignored (only .gitkeep is tracked).
#   4. xcodebuild builds the app with its own DerivedData and package cache under DIR, then
#      devicectl (or simctl) installs and launches it.
# For testing: KURMANCI_REMOTE_CONSUMER_DIR points at a copy of the remote consumer.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONSUMER_DIR="${KURMANCI_REMOTE_CONSUMER_DIR:-$REPO_ROOT/integration/apple/ios-remote-consumer}"
PROJECT="$CONSUMER_DIR/KurmanciConsumer.xcodeproj"
BUNDLE_ID="org.kurmanci.consumer"
PACK_IDS=(reviewed experimental-full)

VERSION=""
DIR="$REPO_ROOT/dist/vendor-evaluation"
DEVICE=""
SIMULATOR=0
TEAM="${DEVELOPMENT_TEAM:-}"
STAGE_ONLY=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dir) DIR="$2"; shift 2 ;;
    --device) DEVICE="$2"; shift 2 ;;
    --simulator) SIMULATOR=1; shift ;;
    --team) TEAM="$2"; shift 2 ;;
    --stage-only) STAGE_ONLY=1; shift ;;
    -h|--help) sed -n '2,32p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "❌ unknown argument: $1" >&2; exit 1 ;;
  esac
done
[[ -n "$VERSION" ]] || { echo "❌ --version is required" >&2; exit 1; }
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "❌ --version must be X.Y.Z, got '$VERSION'" >&2; exit 1; }
[[ -n "$DEVICE" && $SIMULATOR -eq 1 ]] && { echo "❌ --device and --simulator exclude each other" >&2; exit 1; }
[[ -d "$CONSUMER_DIR" ]] || { echo "❌ remote consumer not found at $CONSUMER_DIR" >&2; exit 1; }

BUNDLE_NAME="kurmanci-ku-Latn-$VERSION"
BUNDLE_DIR="$DIR/$BUNDLE_NAME"
PACKS_DIR="$CONSUMER_DIR/Packs"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi
}

# 1. Bundle: fetch when absent, then check exactly what is staged against SHA256SUMS.
if [[ ! -d "$BUNDLE_DIR" ]]; then
  echo "== $BUNDLE_NAME not in $DIR; fetching and verifying it through scripts/vendor/evaluate.sh"
  "$REPO_ROOT/scripts/vendor/evaluate.sh" fetch --version "$VERSION" --dir "$DIR"
fi
[[ -f "$BUNDLE_DIR/SHA256SUMS" ]] || { echo "❌ $BUNDLE_DIR/SHA256SUMS missing" >&2; exit 1; }
[[ -f "$BUNDLE_DIR/VERSION" ]] || { echo "❌ $BUNDLE_DIR/VERSION missing" >&2; exit 1; }
BUNDLE_VERSION="$(tr -d '[:space:]' < "$BUNDLE_DIR/VERSION")"
[[ "$BUNDLE_VERSION" == "$VERSION" ]] || { echo "❌ $BUNDLE_NAME/VERSION is '$BUNDLE_VERSION', not $VERSION" >&2; exit 1; }
BUNDLE_IDENTITY="$(sha256_of "$BUNDLE_DIR/SHA256SUMS")"
declare -a PACK_SHAS=()
for pack in "${PACK_IDS[@]}"; do
  rel="packs/$pack/lexicon.bin"
  [[ -f "$BUNDLE_DIR/$rel" ]] || { echo "❌ $BUNDLE_NAME/$rel missing" >&2; exit 1; }
  listed="$(awk -v p="$rel" '$2 == p {print $1}' "$BUNDLE_DIR/SHA256SUMS")"
  [[ -n "$listed" ]] || { echo "❌ $rel is not listed in $BUNDLE_NAME/SHA256SUMS" >&2; exit 1; }
  [[ "$(printf '%s\n' "$listed" | wc -l | tr -d ' ')" == "1" ]] || { echo "❌ $rel is listed more than once in SHA256SUMS" >&2; exit 1; }
  actual="$(sha256_of "$BUNDLE_DIR/$rel")"
  [[ "$actual" == "$listed" ]] || { echo "❌ $rel hashes $actual, SHA256SUMS lists $listed" >&2; exit 1; }
  PACK_SHAS+=("$actual")
  echo "✅ $rel matches SHA256SUMS ($actual)"
done
echo "✅ bundle $BUNDLE_NAME, identity $BUNDLE_IDENTITY"

# 2. The consumer must require and resolve the Swift package at exactly V.
python3 - "$PROJECT" "$VERSION" <<'EOF'
import json, re, sys
project, version = sys.argv[1], sys.argv[2]
pbx = open(f"{project}/project.pbxproj").read()
m = re.search(r'XCRemoteSwiftPackageReference "kurmanci-swift".*?requirement = \{\s*kind = exactVersion;\s*version = "?([0-9.]+)"?;', pbx, re.S)
if not m:
    print("❌ the remote consumer does not require kurmanci-swift by exact version", file=sys.stderr); sys.exit(1)
if m.group(1) != version:
    print(f"❌ the remote consumer requires kurmanci-swift {m.group(1)}, not {version}", file=sys.stderr); sys.exit(1)
res = json.load(open(f"{project}/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"))
pins = [p for p in res.get("pins", []) if p.get("identity") == "kurmanci-swift"]
if len(pins) != 1 or pins[0].get("state", {}).get("version") != version:
    print(f"❌ Package.resolved does not pin kurmanci-swift {version}: {pins}", file=sys.stderr); sys.exit(1)
print(f"✅ remote consumer requires and pins kurmanci-swift {version} (revision {pins[0]['state'].get('revision')})")
EOF

# 3. Stage.
mkdir -p "$PACKS_DIR"
rm -f "$PACKS_DIR"/*.bin "$PACKS_DIR/packs.json"
for i in "${!PACK_IDS[@]}"; do
  pack="${PACK_IDS[$i]}"
  cp "$BUNDLE_DIR/packs/$pack/lexicon.bin" "$PACKS_DIR/$pack.bin"
done
python3 - "$PACKS_DIR/packs.json" "$VERSION" "$BUNDLE_IDENTITY" "${PACK_IDS[@]}" -- "${PACK_SHAS[@]}" <<'EOF'
import json, sys
out, version, identity = sys.argv[1], sys.argv[2], sys.argv[3]
rest = sys.argv[4:]
split = rest.index("--")
ids, shas = rest[:split], rest[split + 1:]
doc = {
    "schema_version": "ios-tryout-staging-v1",
    "release_version": version,
    "bundle_identity": identity,
    "sdk_version": version,
    "packs": [{"pack_id": i, "file": f"{i}.bin", "sha256": s} for i, s in zip(ids, shas)],
}
with open(out, "w") as f:
    json.dump(doc, f, indent=2, sort_keys=True)
    f.write("\n")
EOF
echo "✅ staged ${PACK_IDS[*]} into $PACKS_DIR (packs.json written)"
[[ $STAGE_ONLY -eq 1 ]] && { echo "== --stage-only: not building"; exit 0; }

# 4. Build, install, launch.
command -v xcodebuild >/dev/null 2>&1 || { echo "❌ xcodebuild not found" >&2; exit 1; }
DERIVED="$DIR/tryout-derived"
PACKAGES="$DIR/tryout-packages"
COMMON=(-project "$PROJECT" -scheme KurmanciConsumer -configuration Debug -derivedDataPath "$DERIVED" -clonedSourcePackagesDirPath "$PACKAGES" ONLY_ACTIVE_ARCH=YES)
LOG="$DIR/tryout-xcodebuild.log"

# The build's exit status is xcodebuild's, never the filter's; the full log stays in $LOG.
run_xcodebuild() {
  set +e
  xcodebuild build "${COMMON[@]}" "$@" 2>&1 | tee "$LOG" | grep -E 'error:|BUILD'
  local status=${PIPESTATUS[0]}
  set -e
  [[ $status -eq 0 ]] || { echo "❌ xcodebuild failed (exit $status); full log: $LOG" >&2; tail -n 40 "$LOG" >&2; exit "$status"; }
}

if [[ $SIMULATOR -eq 1 ]]; then
  SIM_UDID="$(xcrun simctl list devices available --json | python3 -c '
import json, sys
for runtime, devices in json.load(sys.stdin).get("devices", {}).items():
    if "iOS" in runtime:
        for d in devices:
            if d.get("isAvailable") and "iPhone" in d.get("name", ""):
                print(d["udid"]); sys.exit(0)
')"
  [[ -n "$SIM_UDID" ]] || { echo "❌ no available iPhone simulator" >&2; exit 1; }
  echo "== building for simulator $SIM_UDID"
  run_xcodebuild -sdk iphonesimulator -destination "platform=iOS Simulator,id=$SIM_UDID" \
    CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY=""
  APP="$DERIVED/Build/Products/Debug-iphonesimulator/KurmanciConsumer.app"
  [[ -d "$APP" ]] || { echo "❌ build produced no $APP" >&2; exit 1; }
  xcrun simctl boot "$SIM_UDID" 2>/dev/null || true
  xcrun simctl bootstatus "$SIM_UDID" -b >/dev/null 2>&1 || true
  xcrun simctl install "$SIM_UDID" "$APP"
  xcrun simctl launch "$SIM_UDID" "$BUNDLE_ID" >/dev/null
  open -a Simulator 2>/dev/null || true
  echo "✅ launched $BUNDLE_ID on simulator $SIM_UDID"
  exit 0
fi

if [[ -z "$DEVICE" ]]; then
  LIST="$(mktemp)"
  xcrun devicectl list devices --json-output "$LIST" >/dev/null 2>&1 || { echo "❌ xcrun devicectl could not list devices" >&2; rm -f "$LIST"; exit 1; }
  DEVICE="$(python3 - "$LIST" <<'EOF'
import json, sys
doc = json.load(open(sys.argv[1]))
found = []
for d in doc.get("result", {}).get("devices", []):
    hw = d.get("hardwareProperties", {})
    conn = d.get("connectionProperties", {})
    if hw.get("reality") == "physical" and conn.get("tunnelState") == "connected":
        found.append(hw.get("udid", ""))
if len(found) == 1:
    print(found[0])
elif len(found) > 1:
    print("❌ several connected iPhones; pass --device UDID: " + ", ".join(found), file=sys.stderr); sys.exit(1)
else:
    print("❌ no connected physical iPhone (plug it in, unlock it, trust this Mac) or pass --device UDID / --simulator", file=sys.stderr); sys.exit(1)
EOF
)"
  rm -f "$LIST"
fi
[[ -n "$TEAM" ]] || { echo "❌ a physical device needs --team TEAMID (or DEVELOPMENT_TEAM) for automatic signing" >&2; exit 1; }
echo "== building for device $DEVICE (team $TEAM, automatic Apple Development signing)"
run_xcodebuild -destination "platform=iOS,id=$DEVICE" -allowProvisioningUpdates \
  CODE_SIGNING_ALLOWED=YES CODE_SIGNING_REQUIRED=YES CODE_SIGN_STYLE=Automatic CODE_SIGN_IDENTITY="Apple Development" DEVELOPMENT_TEAM="$TEAM"
APP="$DERIVED/Build/Products/Debug-iphoneos/KurmanciConsumer.app"
[[ -d "$APP" ]] || { echo "❌ build produced no $APP" >&2; exit 1; }
xcrun devicectl device install app --device "$DEVICE" "$APP"
xcrun devicectl device process launch --device "$DEVICE" "$BUNDLE_ID"
echo "✅ installed and launched $BUNDLE_ID on $DEVICE"
