#!/usr/bin/env bash
# Shell-level check of scripts/apple/tryout.sh against a fake release bundle and a copy of the
# remote consumer (KURMANCI_REMOTE_CONSUMER_DIR): staging must copy exactly the two packs the
# bundle's SHA256SUMS lists and record their hashes, the bundle identity and the versions in
# packs.json; a pack whose bytes differ from SHA256SUMS, a bundle whose VERSION differs, and a
# consumer that requires or pins another kurmanci-swift version must each be refused before
# anything is staged; and the device build must go through xcodebuild with automatic Apple
# Development signing for the requested team and through devicectl for install and launch
# (fake xcodebuild and xcrun on PATH record their arguments). Needs bash and python3, no Xcode.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WRAPPER="$SCRIPT_DIR/tryout.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
V="9.9.9"

sha256_of() { shasum -a 256 "$1" | awk '{print $1}'; }

# A fake bundle: two packs plus one other listed file, SHA256SUMS over all of them.
make_bundle() {
  local dir="$1/kurmanci-ku-Latn-$V"
  mkdir -p "$dir/packs/reviewed" "$dir/packs/experimental-full"
  printf 'reviewed pack bytes' > "$dir/packs/reviewed/lexicon.bin"
  printf 'experimental pack bytes' > "$dir/packs/experimental-full/lexicon.bin"
  printf '%s\n' "$V" > "$dir/VERSION"
  printf '{}\n' > "$dir/provenance.json"
  (cd "$dir" && for f in packs/reviewed/lexicon.bin packs/experimental-full/lexicon.bin VERSION provenance.json; do
    printf '%s  %s\n' "$(sha256_of "$f")" "$f"; done > SHA256SUMS)
}

# A copy of the remote consumer with the package requirement and pin at the given versions.
make_consumer() {
  local dir="$1" req="$2" pin="$3"
  rm -rf "$dir"
  mkdir -p "$dir"
  cp -R "$REPO_ROOT/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj" "$dir/"
  mkdir -p "$dir/Packs"
  python3 - "$dir/KurmanciConsumer.xcodeproj" "$req" "$pin" <<'EOF'
import json, re, sys
project, req, pin = sys.argv[1], sys.argv[2], sys.argv[3]
p = f"{project}/project.pbxproj"
s = open(p).read()
s2, n = re.subn(r'(XCRemoteSwiftPackageReference "kurmanci-swift".*?requirement = \{\s*kind = exactVersion;\s*version = )"?[0-9.]+"?;', rf'\g<1>{req};', s, count=1, flags=re.S)
assert n == 1
open(p, "w").write(s2)
r = f"{project}/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"
doc = json.load(open(r))
for pin_entry in doc["pins"]:
    if pin_entry["identity"] == "kurmanci-swift":
        pin_entry["state"]["version"] = pin
json.dump(doc, open(r, "w"), indent=2)
EOF
}

fail() { echo "❌ $1" >&2; exit 1; }

# 1. Happy path, --stage-only.
make_bundle "$TMP/eval"
make_consumer "$TMP/consumer" "$V" "$V"
KURMANCI_REMOTE_CONSUMER_DIR="$TMP/consumer" "$WRAPPER" --version "$V" --dir "$TMP/eval" --stage-only > "$TMP/stage.log" 2>&1 || { cat "$TMP/stage.log" >&2; fail "staging failed"; }
python3 - "$TMP/consumer/Packs" "$TMP/eval/kurmanci-ku-Latn-$V" "$V" <<'EOF' || fail "packs.json or staged files are wrong"
import hashlib, json, os, sys
packs, bundle, version = sys.argv[1], sys.argv[2], sys.argv[3]
def sha(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
doc = json.load(open(os.path.join(packs, "packs.json")))
assert doc["schema_version"] == "ios-tryout-staging-v1", doc
assert doc["release_version"] == version and doc["sdk_version"] == version, doc
assert doc["bundle_identity"] == sha(os.path.join(bundle, "SHA256SUMS")), doc
assert [p["pack_id"] for p in doc["packs"]] == ["reviewed", "experimental-full"], doc
for p in doc["packs"]:
    staged = os.path.join(packs, p["file"])
    assert p["file"] == p["pack_id"] + ".bin", p
    assert sha(staged) == p["sha256"] == sha(os.path.join(bundle, "packs", p["pack_id"], "lexicon.bin")), p
present = sorted(f for f in os.listdir(packs) if f != ".gitkeep")
assert present == ["experimental-full.bin", "packs.json", "reviewed.bin"], present
print("packs.json and staged files verified")
EOF
grep -q "not building" "$TMP/stage.log" || fail "--stage-only did not stop before the build"
echo "✅ staging records both packs with their SHA256SUMS hashes, the bundle identity and the versions"

# 2. Refusals, each before anything is staged.
refuse() {
  local label="$1" consumer="$2" eval="$3" pattern="$4"
  rm -f "$consumer/Packs"/*.bin "$consumer/Packs/packs.json"
  set +e
  KURMANCI_REMOTE_CONSUMER_DIR="$consumer" "$WRAPPER" --version "$V" --dir "$eval" --stage-only > "$TMP/refuse.log" 2>&1
  local status=$?
  set -e
  [[ $status -ne 0 ]] || { cat "$TMP/refuse.log" >&2; fail "$label: not refused"; }
  grep -q -- "$pattern" "$TMP/refuse.log" || { cat "$TMP/refuse.log" >&2; fail "$label: refused for another reason"; }
  if ls "$consumer/Packs"/*.bin >/dev/null 2>&1 || [[ -f "$consumer/Packs/packs.json" ]]; then fail "$label: packs were staged although the run was refused"; fi
  echo "✅ refused: $label"
}

make_bundle "$TMP/eval-tampered"
printf 'tampered' >> "$TMP/eval-tampered/kurmanci-ku-Latn-$V/packs/reviewed/lexicon.bin"
refuse "pack bytes differ from SHA256SUMS" "$TMP/consumer" "$TMP/eval-tampered" "SHA256SUMS lists"

make_bundle "$TMP/eval-version"
printf '1.2.3\n' > "$TMP/eval-version/kurmanci-ku-Latn-$V/VERSION"
refuse "bundle VERSION differs" "$TMP/consumer" "$TMP/eval-version" "VERSION is '1.2.3'"

make_bundle "$TMP/eval-unlisted"
sed -i.bak '/experimental-full/d' "$TMP/eval-unlisted/kurmanci-ku-Latn-$V/SHA256SUMS"
refuse "pack not listed in SHA256SUMS" "$TMP/consumer" "$TMP/eval-unlisted" "not listed"

make_consumer "$TMP/consumer-req" "1.0.0" "$V"
refuse "consumer requires another SDK version" "$TMP/consumer-req" "$TMP/eval" "requires kurmanci-swift 1.0.0"

make_consumer "$TMP/consumer-pin" "$V" "1.0.0"
refuse "Package.resolved pins another SDK version" "$TMP/consumer-pin" "$TMP/eval" "does not pin kurmanci-swift $V"

# 3. Device path through fake xcodebuild and xcrun.
mkdir -p "$TMP/bin"
cat > "$TMP/bin/xcodebuild" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$TMP/xcodebuild-args"
# Produce the app the wrapper expects, at the derived data path it passed.
derived=""
while [[ \$# -gt 0 ]]; do case "\$1" in -derivedDataPath) derived="\$2"; shift 2 ;; *) shift ;; esac; done
mkdir -p "\$derived/Build/Products/Debug-iphoneos/KurmanciConsumer.app"
echo "** BUILD SUCCEEDED **"
EOF
cat > "$TMP/bin/xcrun" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" >> "$TMP/xcrun-args"
if [[ "\$1 \$2 \$3" == "devicectl list devices" ]]; then
  out=""; while [[ \$# -gt 0 ]]; do case "\$1" in --json-output) out="\$2"; shift 2 ;; *) shift ;; esac; done
  cat > "\$out" <<'JSON'
{"result":{"devices":[
 {"hardwareProperties":{"udid":"FAKE-PHONE","reality":"physical"},"connectionProperties":{"tunnelState":"connected"}},
 {"hardwareProperties":{"udid":"FAKE-SIM","reality":"simulated"},"connectionProperties":{"tunnelState":"connected"}},
 {"hardwareProperties":{"udid":"FAKE-OFFLINE","reality":"physical"},"connectionProperties":{"tunnelState":"unavailable"}}
]}}
JSON
fi
exit 0
EOF
chmod +x "$TMP/bin/xcodebuild" "$TMP/bin/xcrun"
rm -f "$TMP/xcrun-args"
PATH="$TMP/bin:$PATH" KURMANCI_REMOTE_CONSUMER_DIR="$TMP/consumer" "$WRAPPER" --version "$V" --dir "$TMP/eval" --team ABCDE12345 > "$TMP/device.log" 2>&1 || { cat "$TMP/device.log" >&2; fail "device run failed"; }
expect_arg() { grep -qxF -- "$1" "$2" || { echo "--- $2"; cat "$2"; fail "expected argument '$1' in $2"; }; }
expect_arg "platform=iOS,id=FAKE-PHONE" "$TMP/xcodebuild-args"
expect_arg "DEVELOPMENT_TEAM=ABCDE12345" "$TMP/xcodebuild-args"
expect_arg "CODE_SIGN_STYLE=Automatic" "$TMP/xcodebuild-args"
expect_arg "CODE_SIGN_IDENTITY=Apple Development" "$TMP/xcodebuild-args"
expect_arg "-allowProvisioningUpdates" "$TMP/xcodebuild-args"
expect_arg "-derivedDataPath" "$TMP/xcodebuild-args"
expect_arg "$TMP/eval/tryout-derived" "$TMP/xcodebuild-args"
grep -q "^install$" "$TMP/xcrun-args" && grep -q "^FAKE-PHONE$" "$TMP/xcrun-args" && grep -q "^launch$" "$TMP/xcrun-args" && grep -q "^org.kurmanci.consumer$" "$TMP/xcrun-args" || { cat "$TMP/xcrun-args"; fail "devicectl install/launch not issued for the connected phone"; }
echo "✅ device run: the one connected physical iPhone is chosen, the build is signed for the team, devicectl installs and launches"

# 4. A physical device without a team is refused before xcodebuild.
rm -f "$TMP/xcodebuild-args"
set +e
PATH="$TMP/bin:$PATH" DEVELOPMENT_TEAM="" KURMANCI_REMOTE_CONSUMER_DIR="$TMP/consumer" "$WRAPPER" --version "$V" --dir "$TMP/eval" --device FAKE-PHONE > "$TMP/noteam.log" 2>&1
STATUS=$?
set -e
[[ $STATUS -ne 0 ]] || fail "device run without a team was not refused"
grep -q "needs --team" "$TMP/noteam.log" || { cat "$TMP/noteam.log" >&2; fail "no-team refusal has another reason"; }
[[ ! -f "$TMP/xcodebuild-args" ]] || fail "xcodebuild ran although no team was given"
echo "✅ refused: physical device without a signing team, before xcodebuild"

echo "✅ tryout.sh checks passed"
