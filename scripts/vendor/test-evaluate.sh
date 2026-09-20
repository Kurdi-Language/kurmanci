#!/usr/bin/env bash
# Shell-level check of scripts/vendor/evaluate.sh. Against a fake release served from file://
# URLs, `fetch` must accept a consistent release and fail closed (non-zero, naming the problem,
# no completion line) when: the bundle tarball does not match its published hash; a listed
# bundle file does not match SHA256SUMS; an unlisted regular file is present; the bundle's
# VERSION differs from the requested version; provenance.json's release_version differs; the
# Maven Central AAR differs from the hash provenance.json recorded for it; provenance.json
# records no attached XCFramework at the expected platform/path. `ios` must run only when the
# remote consumer project requires exactly the requested SDK version and its Package.resolved
# pins it, refusing (before any xcodebuild call) when either differs; a fake xcodebuild on
# PATH records whether it was invoked. Needs bash, curl, tar, python3 and shasum or
# sha256sum; no network, no Rust, no Xcode.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DRIVER="$SCRIPT_DIR/evaluate.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
V="9.9.9"
NAME="kurmanci-ku-Latn-$V"

sha() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi; }

write_sums() {
  ( cd "$1" && find . -type f ! -name SHA256SUMS | sed 's|^\./||' | LC_ALL=C sort | while read -r f; do printf '%s  %s\n' "$(sha "$f")" "$f"; done > SHA256SUMS )
}

publish_tarball() {
  local root="$1"
  tar -czf "$root/releases/v$V/$NAME.tar.gz" -C "$root/src" "$NAME"
  printf '%s  %s.tar.gz\n' "$(sha "$root/releases/v$V/$NAME.tar.gz")" "$NAME" > "$root/releases/v$V/$NAME.tar.gz.sha256"
}

write_provenance() {
  local b="$1" release_version="$2"
  printf '{"release_version":"%s","release_kind":"production","source":{"repository":"r","commit":"abc","data_tree":"def","worktree_dirty":false},"packs":[{"pack_id":"reviewed","entry_count":3,"model_profile":"prediction"}],"platform_artifacts":[{"platform":"android","path":"android/kurmanci-android-%s.aar","sha256":"%s"},{"platform":"apple","path":"apple/KurmanciFFI-v%s.xcframework.zip","sha256":"%s"}]}\n' \
    "$release_version" "$V" "$(sha "$b/android/kurmanci-android-$V.aar")" "$V" "$(sha "$b/apple/KurmanciFFI-v$V.xcframework.zip")" > "$b/provenance.json"
}

# A fake but structurally faithful release: bundle directory with SHA256SUMS and provenance
# recording the attached AAR and XCFramework by platform and path, its tarball and .sha256,
# the AAR on a fake Maven layout, the XCFramework on the fake swift release.
make_release() {
  local root="$1"
  rm -rf "$root"
  local maven="$root/maven/io/github/ferhatguneri/kurmanci-android/$V"
  local b="$root/src/$NAME"
  mkdir -p "$root/releases/v$V" "$root/releases/swift-v$V" "$maven" "$b/packs/reviewed" "$b/android" "$b/apple"
  printf 'aar bytes\n' > "$maven/kurmanci-android-$V.aar"
  printf '<project/>\n' > "$maven/kurmanci-android-$V.pom"
  printf 'xcframework bytes\n' > "$root/releases/swift-v$V/KurmanciFFI-v$V.xcframework.zip"
  cp "$maven/kurmanci-android-$V.aar" "$b/android/"
  cp "$root/releases/swift-v$V/KurmanciFFI-v$V.xcframework.zip" "$b/apple/"
  printf '%s\n' "$V" > "$b/VERSION"
  printf 'pack bytes\n' > "$b/packs/reviewed/lexicon.bin"
  printf '{"schema_version":"x","engine_version":"%s","c_abi_version":{"major":1,"minor":1},"pack_magic":"KMRP","pack_schema_version":4,"supported_pack_schemas":[4],"language_model_schema_version":1,"supported_language_model_schemas":[1],"language_tag":"ku-Latn"}\n' "$V" > "$b/compatibility.json"
  write_provenance "$b" "$V"
  write_sums "$b"
  publish_tarball "$root"
}

run_fetch() {
  local root="$1" out="$2"
  KURMANCI_RELEASE_BASE_URL="file://$root/releases" KURMANCI_MAVEN_BASE_URL="file://$root/maven" \
    "$DRIVER" fetch --version "$V" --dir "$out"
}

# 1. Consistent release: accepted, identity and summary printed, layout complete.
make_release "$TMP/good"
run_fetch "$TMP/good" "$TMP/out-good" > "$TMP/good.log" 2>&1 || { echo "❌ consistent release rejected" >&2; cat "$TMP/good.log" >&2; exit 1; }
grep -q "fetch complete" "$TMP/good.log" || { echo "❌ no completion line" >&2; cat "$TMP/good.log" >&2; exit 1; }
grep -q "is the identity of this bundle" "$TMP/good.log" || { echo "❌ bundle identity not printed" >&2; exit 1; }
grep -q "release identity $V: VERSION and provenance agree" "$TMP/good.log" || { echo "❌ release identity not confirmed" >&2; cat "$TMP/good.log" >&2; exit 1; }
grep -q "pack reviewed: 3 entries" "$TMP/good.log" || { echo "❌ pack summary not printed" >&2; cat "$TMP/good.log" >&2; exit 1; }
[[ -f "$TMP/out-good/$NAME/packs/reviewed/lexicon.bin" && -f "$TMP/out-good/sdk/android/kurmanci-android-$V.aar" && -f "$TMP/out-good/sdk/apple/KurmanciFFI-v$V.xcframework.zip" ]] || { echo "❌ fetched layout incomplete" >&2; exit 1; }
echo "✅ consistent release is accepted and its identity printed"

expect_refusal() {
  local label="$1" root="$2" needle="$3"
  set +e
  run_fetch "$root" "$TMP/out-$label" > "$TMP/$label.log" 2>&1
  local status=$?
  set -e
  [[ $status -ne 0 ]] || { echo "❌ $label: accepted" >&2; cat "$TMP/$label.log" >&2; exit 1; }
  grep -q "$needle" "$TMP/$label.log" || { echo "❌ $label: refusal does not name the problem ('$needle')" >&2; cat "$TMP/$label.log" >&2; exit 1; }
  if grep -q "fetch complete" "$TMP/$label.log"; then echo "❌ $label: completion reported after a failure" >&2; exit 1; fi
  echo "✅ $label is refused: $(grep -m1 "$needle" "$TMP/$label.log" | cut -c1-110)"
}

# 2. Tarball differs from its published hash.
make_release "$TMP/tar"
printf 'x' >> "$TMP/tar/releases/v$V/$NAME.tar.gz"
expect_refusal "tampered-tarball" "$TMP/tar" "bundle tarball sha256"

# 3. A listed bundle file differs from SHA256SUMS (the tarball hash is republished to match,
#    so only the inner check can catch it).
make_release "$TMP/inner"
printf 'changed\n' > "$TMP/inner/src/$NAME/packs/reviewed/lexicon.bin"
publish_tarball "$TMP/inner"
expect_refusal "tampered-listed-file" "$TMP/inner" "does not match its SHA256SUMS entry"

# 4. An extra regular file that SHA256SUMS does not list.
make_release "$TMP/extra"
printf 'smuggled\n' > "$TMP/extra/src/$NAME/packs/reviewed/extra.bin"
publish_tarball "$TMP/extra"
expect_refusal "unlisted-file" "$TMP/extra" "not listed in SHA256SUMS: packs/reviewed/extra.bin"

# 5. The bundle's VERSION differs from the requested version (SHA256SUMS consistent).
make_release "$TMP/version"
printf '9.9.8\n' > "$TMP/version/src/$NAME/VERSION"
write_sums "$TMP/version/src/$NAME"; publish_tarball "$TMP/version"
expect_refusal "version-mismatch" "$TMP/version" "VERSION is '9.9.8' but '$V' was requested"

# 6. provenance.json release_version differs from the requested version.
make_release "$TMP/provver"
write_provenance "$TMP/provver/src/$NAME" "9.9.8"
write_sums "$TMP/provver/src/$NAME"; publish_tarball "$TMP/provver"
expect_refusal "provenance-version-mismatch" "$TMP/provver" "release_version is '9.9.8' but '$V' was requested"

# 7. The AAR on Maven Central is not the one the bundle recorded.
make_release "$TMP/aar"
printf 'other aar\n' > "$TMP/aar/maven/io/github/ferhatguneri/kurmanci-android/$V/kurmanci-android-$V.aar"
expect_refusal "different-maven-aar" "$TMP/aar" "differs from the hash provenance.json recorded for android/kurmanci-android-$V.aar"

# 8. provenance.json records no attached XCFramework at the expected platform/path (a file of
#    the same name under another platform does not count): refused, not accepted unverified.
make_release "$TMP/noprov"
python3 - "$TMP/noprov/src/$NAME" "$V" <<'PY'
import json, sys, os
root, v = sys.argv[1], sys.argv[2]
path = os.path.join(root, "provenance.json")
p = json.load(open(path))
p["platform_artifacts"] = [a for a in p["platform_artifacts"] if a["platform"] != "apple"]
p["platform_artifacts"].append({"platform": "android", "path": f"android/KurmanciFFI-v{v}.xcframework.zip", "sha256": "00" * 32})
json.dump(p, open(path, "w"))
PY
write_sums "$TMP/noprov/src/$NAME"; publish_tarball "$TMP/noprov"
expect_refusal "unrecorded-xcframework" "$TMP/noprov" "does not record an attached apple artifact at apple/KurmanciFFI-v$V.xcframework.zip"

# --- ios: the SDK version is bound to --version -----------------------------------------
# A fake xcodebuild records that it ran and prints a report line so the harness succeeds.
mkdir -p "$TMP/bin"
XCARGS="$TMP/xcodebuild-args"
cat > "$TMP/bin/xcodebuild" <<EOF
#!/usr/bin/env bash
printf '%s\\n' "\$@" > "$XCARGS"
echo 'KURMANCI_DEVICE_BENCHMARK {"schema_version":"device-benchmark-v1","platform":"ios","device_model":"fake","os_version":"0","simulator":true,"pack_file":"benchmark_pack.bin","pack_bytes":1,"pack_sha256":"0000000000000000000000000000000000000000000000000000000000000000","entry_count":1,"pack_format_version":4,"load_ms_median":0.1,"load_ms_min":0.1,"load_ms_max":0.1,"rss_after_load_bytes":1,"rss_after_queries_bytes":1,"operations":[{"name":"known_hit","input":"welat","iterations":1,"p50_us":1,"p95_us":1,"max_us":1,"result_count":1}],"stability_rounds":1,"stable":true}'
exit 0
EOF
chmod +x "$TMP/bin/xcodebuild"

# A copy of the real remote consumer whose requirement and Package.resolved are rewritten to
# the test version; the copies below diverge one field at a time.
REAL="$REPO_ROOT/integration/apple/ios-remote-consumer"
PBX_REL="KurmanciConsumer.xcodeproj/project.pbxproj"
RES_REL="KurmanciConsumer.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"
make_consumer() {
  local dir="$1" project_version="$2" resolved_version="$3"
  rm -rf "$dir"; mkdir -p "$dir"
  cp -R "$REAL/." "$dir/"
  python3 - "$dir/$PBX_REL" "$dir/$RES_REL" "$project_version" "$resolved_version" <<'PY'
import json, re, sys
pbx, res, pv, rv = sys.argv[1:5]
t = open(pbx).read()
t2 = re.sub(r'(XCRemoteSwiftPackageReference "kurmanci-swift".*?requirement = \{.*?version = )"?[0-9.]+"?;', lambda m: m.group(1) + pv + ";", t, count=1, flags=re.S)
assert t2 != t or pv in t
open(pbx, "w").write(t2)
r = json.load(open(res))
for p in r["pins"]:
    if p["identity"] == "kurmanci-swift":
        p["state"]["version"] = rv
json.dump(r, open(res, "w"), indent=2)
PY
}

run_ios() {
  local consumer="$1" out="$2"
  rm -f "$XCARGS"
  KURMANCI_REMOTE_CONSUMER_DIR="$consumer" PATH="$TMP/bin:$PATH" \
    "$DRIVER" ios --version "$V" --dir "$out" --destination "platform=iOS Simulator,id=FAKE"
}

# 9. Requirement and Package.resolved both V: accepted, xcodebuild ran against the remote
#    project with the bundle's pack, report written.
make_consumer "$TMP/consumer-ok" "$V" "$V"
run_ios "$TMP/consumer-ok" "$TMP/out-good" > "$TMP/ios-ok.log" 2>&1 || { echo "❌ ios with matching SDK version refused" >&2; cat "$TMP/ios-ok.log" >&2; exit 1; }
grep -q "requires and resolves kurmanci-swift $V" "$TMP/ios-ok.log" || { echo "❌ ios: version binding not confirmed" >&2; cat "$TMP/ios-ok.log" >&2; exit 1; }
[[ -f "$XCARGS" ]] || { echo "❌ ios: xcodebuild did not run for the matching version" >&2; exit 1; }
grep -qxF -- "$REPO_ROOT/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj" "$XCARGS" || { echo "❌ ios: xcodebuild did not run the remote consumer project" >&2; cat "$XCARGS" >&2; exit 1; }
grep -qF -- "-derivedDataPath" "$XCARGS" || { echo "❌ ios: no dedicated derived data path" >&2; exit 1; }
ls "$TMP/out-good/reports"/ios-*.json >/dev/null 2>&1 || { echo "❌ ios: no report written" >&2; cat "$TMP/ios-ok.log" >&2; exit 1; }
echo "✅ ios runs when the remote consumer requires and resolves kurmanci-swift $V"

expect_ios_refusal() {
  local label="$1" consumer="$2" needle="$3"
  set +e
  run_ios "$consumer" "$TMP/out-good" > "$TMP/$label.log" 2>&1
  local status=$?
  set -e
  [[ $status -ne 0 ]] || { echo "❌ $label: accepted" >&2; cat "$TMP/$label.log" >&2; exit 1; }
  [[ ! -f "$XCARGS" ]] || { echo "❌ $label: xcodebuild ran although the SDK version differs" >&2; exit 1; }
  grep -q "$needle" "$TMP/$label.log" || { echo "❌ $label: refusal does not name the problem ('$needle')" >&2; cat "$TMP/$label.log" >&2; exit 1; }
  echo "✅ $label is refused before xcodebuild: $(grep -m1 "$needle" "$TMP/$label.log" | cut -c1-110)"
}

# 10. Project requirement differs from V (Package.resolved still V).
make_consumer "$TMP/consumer-pbx" "9.9.8" "$V"
expect_ios_refusal "ios-project-requirement-differs" "$TMP/consumer-pbx" "requires kurmanci-swift 9.9.8, but --version $V was requested"

# 11. Package.resolved differs from V (project requirement still V).
make_consumer "$TMP/consumer-res" "$V" "9.9.8"
expect_ios_refusal "ios-package-resolved-differs" "$TMP/consumer-res" "Package.resolved pins kurmanci-swift 9.9.8, but --version $V was requested"
