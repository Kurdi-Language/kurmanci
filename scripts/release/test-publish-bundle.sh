#!/usr/bin/env bash
# Shell-level check of scripts/release/publish-bundle.sh against a fully stubbed world: a
# temporary source repository with a bare origin, a fake `gh` answering from fixture files, a
# fake data-builder that writes a fixture bundle, and file:// download roots. It proves that
# a consistent fixture reaches the pre-publish success point, that --skip-publish never calls
# release creation, that a full publish creates the tag at COMMIT and calls release creation
# on the verified tag, and that every integrity check fails closed before anything is
# published. Needs bash, git, curl, tar, python3 and shasum or sha256sum; no network, no Rust.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PUBLISHER="$SCRIPT_DIR/publish-bundle.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
V="9.9.9"
NAME="kurmanci-ku-Latn-$V"
bash -n "$PUBLISHER" && echo "✅ publish-bundle.sh passes bash -n"

sha() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi; }

# --- a source repository and its bare origin ------------------------------------------
# Commits on main: STALE (kit written for 0.1.0), COMMIT (the release commit, kit for V),
# LATER, NOFILE (kit without the driver), WRONGPIN (kit documented for V but pinning 0.1.0),
# K011 (kit for 0.1.1, for the historical-ABI scenario); OFFMAIN is a branch off COMMIT.
SRC="$TMP/src"; ORIGIN="$TMP/origin.git"
git init -q -b main "$SRC"
git -C "$SRC" config user.name t; git -C "$SRC" config user.email t@example.invalid
mkdir -p "$SRC/ffi/include"
printf '#define KMR_ABI_VERSION_MAJOR 1U\n#define KMR_ABI_VERSION_MINOR 1U\n' > "$SRC/ffi/include/kurmanci.h"
printf 'channel = "1.85.0"\n' > "$SRC/rust-toolchain.toml"
printf 'dist/\n' > "$SRC/.gitignore"
# write_kit <doc version> <swift pin>: the files the publisher requires of a vendor-kit tree
write_kit() {
  local docv="$1" pin="$2"
  mkdir -p "$SRC/docs" "$SRC/scripts/vendor" "$SRC/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj/project.xcworkspace/xcshareddata/swiftpm"
  printf '# Vendor evaluation kit\n\nClone the tag vendor-kit-%s and run evaluate.sh fetch --version %s; a later fix would be vendor-kit-%s-r2.\n' "$docv" "$docv" "$docv" > "$SRC/docs/vendor-evaluation-kit.md"
  printf '#!/usr/bin/env bash\necho kit\n' > "$SRC/scripts/vendor/evaluate.sh"
  printf '\t\tX /* XCRemoteSwiftPackageReference "kurmanci-swift" */ = {\n\t\t\tisa = XCRemoteSwiftPackageReference;\n\t\t\trepositoryURL = "https://github.com/Kurdi-Language/kurmanci-swift.git";\n\t\t\trequirement = {\n\t\t\t\tkind = exactVersion;\n\t\t\t\tversion = %s;\n\t\t\t};\n\t\t};\n' "$pin" > "$SRC/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj/project.pbxproj"
  printf '{"pins":[{"identity":"kurmanci-swift","kind":"remoteSourceControl","state":{"revision":"%s","version":"%s"}}],"version":3}\n' "$(printf '7%.0s' {1..40})" "$pin" > "$SRC/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"
}
write_kit "0.1.0" "0.1.0"
git -C "$SRC" add -A && git -C "$SRC" commit -q -m "stale kit"
STALE="$(git -C "$SRC" rev-parse HEAD)"
write_kit "$V" "$V"
git -C "$SRC" add -A && git -C "$SRC" commit -q -m "release commit"
COMMIT="$(git -C "$SRC" rev-parse HEAD)"
printf 'later\n' > "$SRC/later.txt" && git -C "$SRC" add -A && git -C "$SRC" commit -q -m "later commit"
LATER="$(git -C "$SRC" rev-parse HEAD)"
git -C "$SRC" rm -q scripts/vendor/evaluate.sh && git -C "$SRC" commit -q -m "kit without driver"
NOFILE="$(git -C "$SRC" rev-parse HEAD)"
write_kit "$V" "0.1.0"
git -C "$SRC" add -A && git -C "$SRC" commit -q -m "kit with wrong pin"
WRONGPIN="$(git -C "$SRC" rev-parse HEAD)"
write_kit "0.1.1" "0.1.1"
git -C "$SRC" add -A && git -C "$SRC" commit -q -m "kit for 0.1.1"
K011="$(git -C "$SRC" rev-parse HEAD)"
git -C "$SRC" checkout -q -b unrelated "$COMMIT" && printf 'x\n' > "$SRC/x.txt" && git -C "$SRC" add -A && git -C "$SRC" commit -q -m "not on main"
OFFMAIN="$(git -C "$SRC" rev-parse HEAD)"
git -C "$SRC" checkout -q main
git init -q --bare "$ORIGIN"
git -C "$SRC" remote add origin "$ORIGIN"
git -C "$SRC" push -q origin main

# --- fixture world: downloads, fake gh, fake builder --------------------------------------
WORLD="$TMP/world"
AAR_BYTES="aar bytes"; XC_BYTES="xcframework bytes"

# make_world <dir> [key=value ...]: builds the whole fixture with optional deviations.
make_world() {
  local dir="$1"; shift
  local android_tag="$COMMIT" swift_tag="$COMMIT" android_run="success" apple_run="success"
  local manifest_commit="$COMMIT" manifest_sha="" manifest_abi_minor="1" sm_commit="$COMMIT" pkg_checksum="" pkg_bytes="$XC_BYTES"
  local kit_tag="$COMMIT" v_tag="" bundle_commit="$COMMIT" template=""
  local kv
  for kv in "$@"; do  # commit=X moves every commit-bound default to X
    if [[ "${kv%%=*}" == "commit" ]]; then local c="${kv#*=}"; android_tag="$c"; swift_tag="$c"; manifest_commit="$c"; sm_commit="$c"; kit_tag="$c"; bundle_commit="$c"; fi
  done
  for kv in "$@"; do [[ "${kv%%=*}" == "commit" ]] || local "${kv%%=*}"="${kv#*=}"; done
  rm -rf "$dir"
  local rel="$dir/releases" maven="$dir/maven/io/github/ferhatguneri/kurmanci-android/$V" fix="$dir/fixtures"
  mkdir -p "$rel/swift-v$V" "$maven" "$fix/api" "$fix/runs" "$dir/dist/Frameworks"
  printf '%s\n' "$AAR_BYTES" > "$maven/kurmanci-android-$V.aar"
  printf '<project/>\n' > "$maven/kurmanci-android-$V.pom"
  printf '%s\n' "$XC_BYTES" > "$rel/swift-v$V/KurmanciFFI-v$V.xcframework.zip"
  printf '%s\n' "$pkg_bytes" > "$dir/dist/Frameworks/KurmanciFFI-v$V.xcframework.zip"
  local xc_sha; xc_sha="$(sha "$rel/swift-v$V/KurmanciFFI-v$V.xcframework.zip")"
  [[ -n "$manifest_sha" ]] || manifest_sha="$xc_sha"
  [[ -n "$pkg_checksum" ]] || pkg_checksum="$xc_sha"
  printf '{"schema_version":"apple-sdk-release-v1","sdk_version":"%s","source_repository":"Kurdi-Language/kurmanci","source_tag":"swift-v%s","source_commit":"%s","distribution_repository":"Kurdi-Language/kurmanci-swift","distribution_tag":"%s","c_abi_major":1,"c_abi_minor":%s,"artifact_sha256":"%s","swiftpm_checksum":"%s"}\n' \
    "$V" "$V" "$manifest_commit" "$V" "$manifest_abi_minor" "$manifest_sha" "$manifest_sha" > "$rel/swift-v$V/release-manifest.json"
  python3 - "$fix" "$V" "$android_tag" "$android_tag" "$swift_tag" "$android_run" "$apple_run" "$sm_commit" "$xc_sha" "$pkg_checksum" "$dir/dist/Frameworks/KurmanciFFI-v$V.xcframework.zip" "$kit_tag" "$v_tag" <<'PY'
import base64, json, os, sys
gh, v, commit, android_tag, swift_tag, android_run, apple_run, sm_commit, xc_sha, pkg_checksum, pkg_path, kit_tag, v_tag = sys.argv[1:14]
def api(path, obj):
    name = path.replace("/", "__").replace("?", "__Q__")
    json.dump(obj, open(os.path.join(gh, "api", name + ".json"), "w"))
def ref(repo, tag, sha):
    if sha:
        api(f"repos/{repo}/git/ref/tags/{tag}", {"ref": f"refs/tags/{tag}", "object": {"type": "commit", "sha": sha}})
ref("Kurdi-Language/kurmanci", f"android-v{v}", android_tag)
ref("Kurdi-Language/kurmanci", f"swift-v{v}", swift_tag)
ref("Kurdi-Language/kurmanci", f"vendor-kit-{v}", kit_tag)
ref("Kurdi-Language/kurmanci", f"v{v}", v_tag)
ref("Kurdi-Language/kurmanci-swift", v, "1" * 40)
def contents(repo, path, text):
    api(f"repos/{repo}/contents/{path}?ref={v}", {"content": base64.b64encode(text.encode()).decode()})
sm = {"schema_version": "swift-package-sources-v1", "version": v, "source_commit": sm_commit, "binary_target_checksum": xc_sha, "files": {}}
contents("Kurdi-Language/kurmanci-swift", "source-manifest.json", json.dumps(sm))
pkg = f'let package = Package(targets: [ .binaryTarget(name: "KurmanciFFI", url: "file://{pkg_path}", checksum: "{pkg_checksum}") ])\n'
contents("Kurdi-Language/kurmanci-swift", "Package.swift", pkg)
def runs(workflow, branch, conclusion):
    status = "completed" if conclusion in ("success", "failure") else "in_progress"
    json.dump([{"headSha": commit, "status": status, "conclusion": conclusion if status == "completed" else ""}], open(os.path.join(gh, "runs", f"{workflow}__{branch}.json"), "w"))
runs("release-android-sdk.yml", f"android-v{v}", android_run)
runs("release-apple-sdk.yml", f"swift-v{v}", apple_run)
PY
  # fake gh: answers `api` from fixtures (404 when absent), `run list` from fixtures, logs `release create`
  cat > "$dir/gh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
FIX="$fix"
case "\${1:-}" in
  api)
    path="\$2"; name="\${path//\//__}"; name="\${name//\?/__Q__}"
    if [[ -f "\$FIX/api/\$name.json" ]]; then cat "\$FIX/api/\$name.json"; else echo "gh: Not Found (HTTP 404)" >&2; exit 1; fi ;;
  run)
    wf=""; br=""; shift 2
    while [[ \$# -gt 0 ]]; do case "\$1" in --workflow) wf="\$2"; shift 2;; --branch) br="\$2"; shift 2;; *) shift;; esac; done
    if [[ -f "\$FIX/runs/\${wf}__\${br}.json" ]]; then cat "\$FIX/runs/\${wf}__\${br}.json"; else echo "[]"; fi ;;
  release)
    printf '%s\n' "\$@" >> "\$FIX/release-create.log" ;;
  *) echo "fake gh: unsupported: \$*" >&2; exit 2 ;;
esac
EOF
  chmod +x "$dir/gh"
  # fake data-builder: derivation steps are no-ops; build-release-bundle writes a fixture bundle
  cat > "$dir/data-builder" <<EOF
#!/usr/bin/env bash
set -euo pipefail
case "\$1" in
  build-release-bundle)
    out=""; apple=""; android=""; shift
    while [[ \$# -gt 0 ]]; do case "\$1" in --out) out="\$2"; shift 2;; --apple) apple="\$2"; shift 2;; --android) android="\$2"; shift 2;; *) shift;; esac; done
    python3 - "\$out/$NAME" "$V" "$bundle_commit" "\$apple" "\$android" <<'PY'
import hashlib, json, os, sys
root, v, commit, apple, android = sys.argv[1:6]
def h(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
os.makedirs(os.path.join(root, "packs", "reviewed"), exist_ok=True)
open(os.path.join(root, "VERSION"), "w").write(v + "\n")
open(os.path.join(root, "packs", "reviewed", "lexicon.bin"), "wb").write(b"pack")
compat = {"engine_version": v, "c_abi_version": {"major": 1, "minor": 1}, "pack_schema_version": 4, "language_model_schema_version": 1, "language_tag": "ku-Latn"}
json.dump(compat, open(os.path.join(root, "compatibility.json"), "w"))
prov = {"release_version": v, "release_kind": "production", "source": {"repository": "r", "commit": commit, "data_tree": "d", "worktree_dirty": False},
        "engine_version": v, "c_abi_version": {"major": 1, "minor": 1}, "pack_schema_version": 4, "language_model_schema_version": 1,
        "packs": [{"pack_id": "seed", "entry_count": 33}, {"pack_id": "reviewed", "entry_count": 2143}, {"pack_id": "experimental-full", "entry_count": 42248}],
        "platform_artifacts": [{"platform": "apple", "path": f"apple/{os.path.basename(apple)}", "sha256": h(apple)}, {"platform": "android", "path": f"android/{os.path.basename(android)}", "sha256": h(android)}]}
json.dump(prov, open(os.path.join(root, "provenance.json"), "w"))
lines = []
for d, _, fs in os.walk(root):
    for f in fs:
        p = os.path.join(d, f); rel = os.path.relpath(p, root)
        if rel != "SHA256SUMS": lines.append(f"{h(p)}  {rel}")
open(os.path.join(root, "SHA256SUMS"), "w").write("\n".join(sorted(lines)) + "\n")
print(json.dumps({"bundle_dir": root, "release_kind": "production", "sha256sums_sha256": h(os.path.join(root, "SHA256SUMS")), "provenance": prov}))
PY
    ;;
  verify-release-bundle) echo "release bundle OK: \$2" ;;
  *) : ;;
esac
EOF
  chmod +x "$dir/data-builder"
  if [[ -n "$template" ]]; then printf '%s\n' "$template" > "$dir/template.md"; else cp "$SCRIPT_DIR/release-notes.template.md" "$dir/template.md"; fi
}

run_publisher() {  # <world> <work> [extra args...]
  local world="$1" work="$2"; shift 2
  mkdir -p "$world/bin"; ln -sf "$world/gh" "$world/bin/gh"
  KURMANCI_PUBLISH_REPO_ROOT="$SRC" KURMANCI_DATA_BUILDER_BIN="$world/data-builder" \
  KURMANCI_RELEASE_BASE_URL="file://$world/releases" KURMANCI_MAVEN_BASE_URL="file://$world/maven" \
  KURMANCI_RELEASE_NOTES_TEMPLATE="$world/template.md" KURMANCI_PUBLISH_ALLOW_LOCAL_BINARY_URL=1 \
  PATH="$world/bin:$PATH" "$PUBLISHER" --version "$V" --work "$work" "$@"
}

# 1. Consistent fixture reaches the pre-publish success point; --skip-publish never creates a release.
make_world "$WORLD"
run_publisher "$WORLD" "$TMP/work-ok" --skip-publish > "$TMP/ok.log" 2>&1 || { echo "❌ consistent fixture refused" >&2; cat "$TMP/ok.log" >&2; exit 1; }
grep -q -- "--skip-publish: all checks passed" "$TMP/ok.log" || { echo "❌ no pre-publish success line" >&2; cat "$TMP/ok.log" >&2; exit 1; }
[[ ! -f "$WORLD/fixtures/release-create.log" ]] || { echo "❌ --skip-publish called release create" >&2; exit 1; }
git -C "$SRC" rev-parse --verify --quiet "refs/tags/v$V" >/dev/null && { echo "❌ --skip-publish created the tag v$V" >&2; exit 1; }
grep -q "vendor-kit-$V" "$TMP/work-ok/out/release-notes.md" && ! grep -q '__[A-Z_]*__' "$TMP/work-ok/out/release-notes.md" || { echo "❌ notes not rendered" >&2; exit 1; }
echo "✅ consistent fixture passes every check; --skip-publish publishes nothing and creates no tag"

# 2. Full publish: tag created at COMMIT and pushed to origin, release created on the verified tag.
make_world "$WORLD"
run_publisher "$WORLD" "$TMP/work-pub" > "$TMP/pub.log" 2>&1 || { echo "❌ full publish failed" >&2; cat "$TMP/pub.log" >&2; exit 1; }
[[ "$(git -C "$ORIGIN" rev-parse "refs/tags/v$V^{commit}")" == "$COMMIT" ]] || { echo "❌ tag v$V not pushed at COMMIT" >&2; exit 1; }
grep -q -- "--verify-tag" "$WORLD/fixtures/release-create.log" && grep -qx "v$V" "$WORLD/fixtures/release-create.log" || { echo "❌ release create not called with --verify-tag v$V" >&2; cat "$WORLD/fixtures/release-create.log" >&2; exit 1; }
grep -q "$NAME.tar.gz.sha256" "$WORLD/fixtures/release-create.log" || { echo "❌ release create lacks the .sha256 asset" >&2; exit 1; }
echo "✅ full publish creates tag v$V at COMMIT and creates the release on the verified tag"
git -C "$SRC" tag -d "v$V" >/dev/null; git -C "$SRC" push -q origin ":refs/tags/v$V"

expect_refusal() {
  local label="$1" needle="$2"; shift 2
  set +e
  run_publisher "$WORLD" "$TMP/work-$label" "$@" > "$TMP/$label.log" 2>&1
  local status=$?
  set -e
  [[ $status -ne 0 ]] || { echo "❌ $label: accepted" >&2; cat "$TMP/$label.log" >&2; exit 1; }
  grep -q -- "$needle" "$TMP/$label.log" || { echo "❌ $label: refusal does not name the problem ('$needle')" >&2; cat "$TMP/$label.log" >&2; exit 1; }
  [[ ! -f "$WORLD/fixtures/release-create.log" ]] || { echo "❌ $label: release create was called" >&2; exit 1; }
  git -C "$ORIGIN" rev-parse --verify --quiet "refs/tags/v$V" >/dev/null && { echo "❌ $label: tag v$V was pushed" >&2; exit 1; }
  echo "✅ $label is refused: $(grep -m1 -- "$needle" "$TMP/$label.log" | cut -c1-120)"
}

# 3. Local tag differs from the remote tag.
make_world "$WORLD"; git -C "$SRC" tag "android-v$V" "$LATER"
expect_refusal "local-tag-differs" "local tag android-v$V ($LATER) differs from the remote tag"
git -C "$SRC" tag -d "android-v$V" >/dev/null
# 4. Android remote tag differs from the Swift remote tag / from COMMIT.
make_world "$WORLD" "android_tag=$LATER"
expect_refusal "android-tag-differs" "resolve to different commits"
# 5. Swift remote tag differs.
make_world "$WORLD" "swift_tag=$LATER"
expect_refusal "swift-tag-differs" "resolve to different commits"
# 6. Both tags agree but the commit is not on origin/main.
make_world "$WORLD" "android_tag=$OFFMAIN" "swift_tag=$OFFMAIN"
expect_refusal "commit-off-main" "is not on origin/main"
# 7. Android workflow not successful.
make_world "$WORLD" "android_run=failure"
expect_refusal "android-workflow-failed" "no completed successful run of release-android-sdk.yml"
# 8. Apple workflow still running.
make_world "$WORLD" "apple_run=in_progress"
expect_refusal "apple-workflow-not-finished" "no completed successful run of release-apple-sdk.yml"
# 9. Apple release manifest source commit mismatch.
make_world "$WORLD" "manifest_commit=$LATER"
expect_refusal "manifest-commit-mismatch" "release-manifest.json source_commit"
# 10. Apple artifact hash mismatch.
make_world "$WORLD" "manifest_sha=$(printf '0%.0s' {1..64})"
expect_refusal "manifest-artifact-mismatch" "artifact_sha256 .* differs from the downloaded XCFramework"
# 11. Apple manifest C ABI differs from the header at COMMIT: fatal for this release, with or
#     without the acknowledgement flag (the exception is mechanically limited to 0.1.1).
make_world "$WORLD" "manifest_abi_minor=0"
expect_refusal "manifest-abi-mismatch" "records C ABI 1.0 but ffi/include/kurmanci.h at the release commit says 1.1; no exception applies to release $V"
expect_refusal "manifest-abi-override-attempt" "acknowledges only the historical defect of release 0.1.1" --skip-publish --accept-apple-manifest-abi 1.0
# 11b. The historical case itself: release 0.1.1, recorded 1.0, header 1.1: refused until
#      acknowledged, accepted with the flag; any other recorded value stays fatal.
SAVED_V="$V"; V="0.1.1"; NAME="kurmanci-ku-Latn-$V"
make_world "$WORLD" "commit=$K011" "manifest_abi_minor=0"
expect_refusal "historical-abi-unacknowledged" "known historical defect of release 0.1.1 and must be acknowledged"
run_publisher "$WORLD" "$TMP/work-abi-ok" --skip-publish --accept-apple-manifest-abi 1.0 > "$TMP/abi-ok.log" 2>&1 || { echo "❌ acknowledged historical ABI defect refused" >&2; cat "$TMP/abi-ok.log" >&2; exit 1; }
grep -q "known historical defect of release 0.1.1" "$TMP/abi-ok.log" && grep -q -- "--skip-publish: all checks passed" "$TMP/abi-ok.log" || { echo "❌ acknowledged historical ABI defect not accepted" >&2; cat "$TMP/abi-ok.log" >&2; exit 1; }
echo "✅ the 0.1.1 historical manifest ABI is accepted only with --accept-apple-manifest-abi 1.0"
make_world "$WORLD" "commit=$K011" "manifest_abi_minor=2"
expect_refusal "historical-version-other-abi" "records C ABI 1.2 but ffi/include/kurmanci.h at the release commit says 1.1; no exception applies to release 0.1.1" --skip-publish --accept-apple-manifest-abi 1.0
V="$SAVED_V"; NAME="kurmanci-ku-Latn-$V"
# 12. Swift distribution source-manifest commit mismatch.
make_world "$WORLD" "sm_commit=$LATER"
expect_refusal "swift-source-manifest-commit-mismatch" "source-manifest.json source_commit"
# 13. Swift package binary mismatch (checksum right, bytes differ).
make_world "$WORLD" "pkg_bytes=other bytes"
expect_refusal "swift-package-binary-mismatch" "wraps a different XCFramework"
# 14. Swift package checksum differs.
make_world "$WORLD" "pkg_checksum=$(printf '1%.0s' {1..64})"
expect_refusal "swift-package-checksum-mismatch" "Package.swift binary target checksum"
# 15. Pre-existing wrong vV tag.
make_world "$WORLD" "v_tag=$LATER"
expect_refusal "wrong-existing-release-tag" "tag v$V already exists at $LATER, not at $COMMIT"
# 16. Missing vendor-kit tag.
make_world "$WORLD" "kit_tag="
expect_refusal "missing-vendor-kit-tag" "tag vendor-kit-$V does not exist"
# 16b. vendor-kit tag not on origin/main.
make_world "$WORLD" "kit_tag=$OFFMAIN"
expect_refusal "vendor-kit-off-main" "vendor-kit-$V ($OFFMAIN) is not on origin/main"
# 16c. vendor-kit tag pointing at a stale tree documented for another release.
make_world "$WORLD" "kit_tag=$STALE"
expect_refusal "vendor-kit-stale-tree" "written for vendor-kit \['0.1.0'\] / --version \['0.1.0'\], not for release $V: a stale kit tree"
# 16d. vendor-kit tree without the driver.
make_world "$WORLD" "kit_tag=$NOFILE"
expect_refusal "vendor-kit-missing-driver" "does not contain scripts/vendor/evaluate.sh"
# 16e. vendor-kit tree documented for V but whose remote consumer pins another SDK version.
make_world "$WORLD" "kit_tag=$WRONGPIN"
expect_refusal "vendor-kit-wrong-swift-pin" "requires kurmanci-swift 0.1.0, not $V"
# 17. Bundle provenance source commit mismatch.
make_world "$WORLD" "bundle_commit=$LATER"
expect_refusal "bundle-provenance-commit-mismatch" "provenance.source.commit $LATER differs from COMMIT"
# 18. Unresolved release-note placeholder.
make_world "$WORLD" "template=release __VERSION__ with __UNKNOWN_FIELD__"
expect_refusal "unresolved-placeholder" "unresolved placeholder(s) in the release notes: __UNKNOWN_FIELD__"
