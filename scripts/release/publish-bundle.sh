#!/usr/bin/env bash
# Builds the release bundle of a tagged release from a clean clone and publishes it as the
# GitHub release v<V>, with the platform artifacts the SDK workflows already published
# attached and hashed as received. This is the last step of a release, after the tags
# android-v<V> and swift-v<V> have been pushed, both SDK workflows have succeeded, and the
# vendor-kit-<V> tag exists.
#
# Usage: scripts/release/publish-bundle.sh --version V [--skip-publish] [--work DIR]
#                                          [--accept-apple-manifest-abi MAJOR.MINOR]
#   --version V        the release (X.Y.Z)
#   --skip-publish     do everything except creating the tag and the release: the bundle,
#                      the tarball, its .sha256, SHA256SUMS, provenance.json and the notes
#                      stay under the work directory for inspection
#   --work DIR         work directory (default: dist/release-publish/V under the repository)
#   --accept-apple-manifest-abi 1.0
#                      acknowledge the one known historical defect: release 0.1.1's manifest
#                      records C ABI 1.0 while the header at its commit says 1.1 (the manifest
#                      generator of that release did not derive the ABI from the header). The
#                      exception is mechanical: VERSION 0.1.1, recorded 1.0, header 1.1, and
#                      the flag; any other ABI disagreement is fatal, flag or not.
#
# Every step fails closed:
#   1. the remote tags android-vV and swift-vV (GitHub, not local refs) resolve to one commit,
#      COMMIT, which is on origin/main; local copies of those tags, if any, must agree;
#   2. the exact SDK publication workflow runs (release-android-sdk.yml on android-vV and
#      release-apple-sdk.yml on swift-vV, event push, head COMMIT) completed successfully;
#   3. the remote tag vendor-kit-V exists (every production release has one; it may point at
#      the release commit when no separate kit change was needed), its commit is on
#      origin/main, the tree there carries the kit (docs/vendor-evaluation-kit.md,
#      scripts/vendor/evaluate.sh, the remote iOS consumer), the kit's documented release
#      identity (vendor-kit-V, --version V) is V, and the remote consumer's package
#      requirement and Package.resolved pin the Swift package at exactly V;
#   4. release vV does not exist; the tag vV is absent or already at COMMIT;
#   5. the published AAR (Maven Central) and XCFramework (release swift-vV) are downloaded;
#      swift-vV/release-manifest.json must be the apple-sdk-release-v1 manifest for V,
#      COMMIT and this XCFramework, and its C ABI must be the header's at COMMIT;
#   6. the Swift distribution tag V in Kurdi-Language/kurmanci-swift must carry a
#      swift-package-sources-v1 source-manifest.json for V and COMMIT with this XCFramework's
#      checksum, and a Package.swift whose binary target has that checksum and an immutable
#      URL whose bytes equal the release asset;
#   7. a clean clone at COMMIT runs the derivation steps exactly as
#      scripts/release/verify-clean-checkout-determinism.sh does and must stay clean;
#   8. build-release-bundle with --apple and --android; verify-release-bundle; the bundle must
#      be a production release from a clean tree with release_version V,
#      provenance.source.commit COMMIT, and exactly the two expected platform-artifact
#      records with the downloaded hashes;
#   9. tarball, <tarball>.sha256, SHA256SUMS, provenance.json and the notes rendered from
#      scripts/release/release-notes.template.md (every placeholder must resolve);
#  10. the tag vV is created at COMMIT (if absent) and pushed, and the release is created on
#      that verified tag with the four assets.
# Needs: git, gh (authenticated), curl, tar, shasum or sha256sum, python3, and the Rust
# toolchain (the bundle builder is built in the clone). For the shell test only, the
# environment can substitute the repository (KURMANCI_PUBLISH_REPO_ROOT), the builder
# (KURMANCI_DATA_BUILDER_BIN), the download roots (KURMANCI_RELEASE_BASE_URL,
# KURMANCI_MAVEN_BASE_URL, file:// accepted), the notes template
# (KURMANCI_RELEASE_NOTES_TEMPLATE), allow a non-GitHub binary URL in Package.swift
# (KURMANCI_PUBLISH_ALLOW_LOCAL_BINARY_URL=1) and put a fake gh on PATH.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${KURMANCI_PUBLISH_REPO_ROOT:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
GH_REPO="Kurdi-Language/kurmanci"
SWIFT_DIST_REPO="Kurdi-Language/kurmanci-swift"
MAVEN_GROUP_PATH="io/github/ferhatguneri"
MAVEN_ARTIFACT="kurmanci-android"
RELEASE_BASE_URL="${KURMANCI_RELEASE_BASE_URL:-https://github.com/$GH_REPO/releases/download}"
MAVEN_BASE_URL="${KURMANCI_MAVEN_BASE_URL:-https://repo1.maven.org/maven2}"
TEMPLATE="${KURMANCI_RELEASE_NOTES_TEMPLATE:-$SCRIPT_DIR/release-notes.template.md}"
# shellcheck source=scripts/apple/c-abi-version.sh
source "$SCRIPT_DIR/../apple/c-abi-version.sh"

VERSION=""
SKIP_PUBLISH=0
WORK=""
ACCEPT_MANIFEST_ABI=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --skip-publish) SKIP_PUBLISH=1; shift ;;
    --work) WORK="$2"; shift 2 ;;
    --accept-apple-manifest-abi) ACCEPT_MANIFEST_ABI="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "❌ --version X.Y.Z is required" >&2; exit 1; }
if [[ -n "$ACCEPT_MANIFEST_ABI" ]]; then
  [[ "$ACCEPT_MANIFEST_ABI" == "1.0" && "$VERSION" == "0.1.1" ]] || { echo "❌ --accept-apple-manifest-abi acknowledges only the historical defect of release 0.1.1 (recorded C ABI 1.0); it does not apply to release $VERSION" >&2; exit 1; }
fi
[[ -f "$TEMPLATE" ]] || { echo "❌ release notes template missing: $TEMPLATE" >&2; exit 1; }
WORK="${WORK:-$REPO_ROOT/dist/release-publish/$VERSION}"
BUNDLE_NAME="kurmanci-ku-Latn-$VERSION"
mkdir -p "$WORK"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
  else sha256sum "$1" | awk '{print $1}'; fi
}
download() {
  echo "  ← $1"
  curl -fsSL --retry 5 --retry-delay 10 -o "$2" "$1" || { echo "❌ download failed: $1" >&2; exit 1; }
}
fail() { echo "❌ $*" >&2; exit 1; }

# Resolves a tag of a GitHub repository to the commit it points at (annotated tags are
# peeled). Prints nothing when the tag does not exist.
remote_tag_commit() {
  local repo="$1" tag="$2" json
  json="$(gh api "repos/$repo/git/ref/tags/$tag" 2>/dev/null)" || return 0
  python3 - "$repo" "$json" <<'EOF'
import json, subprocess, sys
repo, obj = sys.argv[1], json.loads(sys.argv[2])["object"]
seen = 0
while obj["type"] == "tag" and seen < 5:
    obj = json.loads(subprocess.check_output(["gh", "api", f"repos/{repo}/git/tags/{obj['sha']}"]))["object"]
    seen += 1
print(obj["sha"] if obj["type"] == "commit" else "")
EOF
}
gh_contents() {  # repo path ref -> decoded file on stdout
  gh api "repos/$1/contents/$2?ref=$3" 2>/dev/null | python3 -c 'import base64, json, sys; print(base64.b64decode(json.load(sys.stdin)["content"]).decode("utf-8"), end="")'
}

echo "=== 1. release $VERSION: remote SDK tags"
ANDROID_COMMIT="$(remote_tag_commit "$GH_REPO" "android-v$VERSION")"
SWIFT_COMMIT="$(remote_tag_commit "$GH_REPO" "swift-v$VERSION")"
[[ -n "$ANDROID_COMMIT" ]] || fail "remote tag android-v$VERSION does not exist in $GH_REPO"
[[ -n "$SWIFT_COMMIT" ]] || fail "remote tag swift-v$VERSION does not exist in $GH_REPO"
[[ "$ANDROID_COMMIT" == "$SWIFT_COMMIT" ]] || fail "remote tags android-v$VERSION ($ANDROID_COMMIT) and swift-v$VERSION ($SWIFT_COMMIT) resolve to different commits"
COMMIT="$ANDROID_COMMIT"
for tag in "android-v$VERSION" "swift-v$VERSION"; do
  if local_sha="$(git -C "$REPO_ROOT" rev-parse --verify --quiet "refs/tags/$tag^{commit}")"; then
    [[ "$local_sha" == "$COMMIT" ]] || fail "local tag $tag ($local_sha) differs from the remote tag ($COMMIT); the remote is authoritative"
  fi
done
git -C "$REPO_ROOT" fetch --quiet origin main
git -C "$REPO_ROOT" cat-file -e "$COMMIT^{commit}" 2>/dev/null || fail "commit $COMMIT is not available locally; fetch first"
git -C "$REPO_ROOT" merge-base --is-ancestor "$COMMIT" origin/main || fail "$COMMIT is not on origin/main"
echo "  COMMIT $COMMIT (android-v$VERSION = swift-v$VERSION, on origin/main)"

echo "=== 2. SDK publication workflow runs"
require_workflow_success() {
  local workflow="$1" branch="$2" runs
  runs="$(gh run list --repo "$GH_REPO" --workflow "$workflow" --event push --branch "$branch" --limit 50 --json headSha,status,conclusion 2>/dev/null || echo '[]')"
  python3 - "$workflow" "$branch" "$COMMIT" "$runs" <<'EOF'
import json, sys
workflow, branch, commit, runs = sys.argv[1], sys.argv[2], sys.argv[3], json.loads(sys.argv[4] or "[]")
ok = [r for r in runs if r.get("headSha") == commit and r.get("status") == "completed" and r.get("conclusion") == "success"]
if not ok:
    seen = ", ".join(f"{r.get('headSha','?')[:7]}:{r.get('status')}/{r.get('conclusion')}" for r in runs) or "none"
    print(f"❌ no completed successful run of {workflow} for {branch} at {commit[:7]} (runs seen: {seen})", file=sys.stderr); sys.exit(1)
print(f"✅ {workflow} on {branch} at {commit[:7]}: completed, success")
EOF
}
require_workflow_success "release-android-sdk.yml" "android-v$VERSION"
require_workflow_success "release-apple-sdk.yml" "swift-v$VERSION"

echo "=== 3. vendor kit tag"
KIT_COMMIT="$(remote_tag_commit "$GH_REPO" "vendor-kit-$VERSION")"
[[ -n "$KIT_COMMIT" ]] || fail "tag vendor-kit-$VERSION does not exist in $GH_REPO; every production release has one (it may point at the release commit) and the notes reference it"
git -C "$REPO_ROOT" cat-file -e "$KIT_COMMIT^{commit}" 2>/dev/null || fail "vendor-kit-$VERSION commit $KIT_COMMIT is not available locally; fetch first"
git -C "$REPO_ROOT" merge-base --is-ancestor "$KIT_COMMIT" origin/main || fail "vendor-kit-$VERSION ($KIT_COMMIT) is not on origin/main"
KIT_DIR="$WORK/vendor-kit"; rm -rf "$KIT_DIR"; mkdir -p "$KIT_DIR"
for f in docs/vendor-evaluation-kit.md scripts/vendor/evaluate.sh \
         integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj/project.pbxproj \
         integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved; do
  mkdir -p "$KIT_DIR/$(dirname "$f")"
  git -C "$REPO_ROOT" show "$KIT_COMMIT:$f" > "$KIT_DIR/$f" 2>/dev/null || fail "vendor-kit-$VERSION ($KIT_COMMIT) does not contain $f; it is not an evaluation kit tree"
done
python3 - "$KIT_DIR" "$VERSION" "$KIT_COMMIT" <<'EOF'
import json, re, sys
root, v, kit = sys.argv[1], sys.argv[2], sys.argv[3]
def fail(msg):
    print(f"❌ vendor-kit-{v} ({kit[:7]}): {msg}", file=sys.stderr); sys.exit(1)
doc = open(f"{root}/docs/vendor-evaluation-kit.md", encoding="utf-8").read()
kit_versions = set(re.findall(r"vendor-kit-(\d+\.\d+\.\d+)", doc))
cli_versions = set(re.findall(r"--version (\d+\.\d+\.\d+)", doc))
if not kit_versions or not cli_versions:
    fail("docs/vendor-evaluation-kit.md names no vendor-kit tag or no --version; the kit's release identity is undocumented")
if kit_versions != {v} or cli_versions != {v}:
    fail(f"docs/vendor-evaluation-kit.md is written for vendor-kit {sorted(kit_versions)} / --version {sorted(cli_versions)}, not for release {v}: a stale kit tree")
pbx = open(f"{root}/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj/project.pbxproj", encoding="utf-8").read()
block = re.search(r'XCRemoteSwiftPackageReference "kurmanci-swift".*?requirement = \{(.*?)\};', pbx, re.S)
req = re.search(r'version = "?(\d+\.\d+\.\d+)"?;', block.group(1)) if block else None
kind = re.search(r'kind = (\w+);', block.group(1)) if block else None
if not req or not kind or kind.group(1) != "exactVersion":
    fail("the remote iOS consumer has no exact kurmanci-swift version requirement")
if req.group(1) != v:
    fail(f"the remote iOS consumer requires kurmanci-swift {req.group(1)}, not {v}")
pins = [p for p in json.load(open(f"{root}/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved")).get("pins", []) if p.get("identity") == "kurmanci-swift"]
if len(pins) != 1 or pins[0].get("state", {}).get("version") != v:
    fail(f"the remote iOS consumer's Package.resolved pins kurmanci-swift {pins[0]['state'].get('version') if pins else None}, not {v}")
print(f"✅ vendor-kit-{v} → {kit}: on origin/main, carries the kit, documented for {v}, remote consumer requires and resolves kurmanci-swift {v}")
EOF

echo "=== 4. release target v$VERSION"
if gh api "repos/$GH_REPO/releases/tags/v$VERSION" >/dev/null 2>&1; then
  if [[ $SKIP_PUBLISH -eq 1 ]]; then echo "  release v$VERSION already exists (reconstruction only; nothing will be published)"
  else fail "release v$VERSION already exists; releases are never rewritten"; fi
fi
EXISTING_TAG="$(remote_tag_commit "$GH_REPO" "v$VERSION")"
if [[ -n "$EXISTING_TAG" && "$EXISTING_TAG" != "$COMMIT" ]]; then fail "tag v$VERSION already exists at $EXISTING_TAG, not at $COMMIT"; fi
if local_v="$(git -C "$REPO_ROOT" rev-parse --verify --quiet "refs/tags/v$VERSION^{commit}")"; then
  [[ "$local_v" == "$COMMIT" ]] || fail "local tag v$VERSION ($local_v) is not at $COMMIT"
fi
if [[ -n "$EXISTING_TAG" ]]; then echo "  tag v$VERSION exists at $COMMIT"; else echo "  tag v$VERSION absent; will be created at $COMMIT"; fi

echo "=== 5. published platform artifacts"
IN="$WORK/inputs"; rm -rf "$IN"; mkdir -p "$IN/android" "$IN/apple"
MAVEN="$MAVEN_BASE_URL/$MAVEN_GROUP_PATH/$MAVEN_ARTIFACT/$VERSION"
download "$MAVEN/$MAVEN_ARTIFACT-$VERSION.aar" "$IN/android/$MAVEN_ARTIFACT-$VERSION.aar"
download "$MAVEN/$MAVEN_ARTIFACT-$VERSION.pom" "$IN/android/$MAVEN_ARTIFACT-$VERSION.pom"
download "$RELEASE_BASE_URL/swift-v$VERSION/KurmanciFFI-v$VERSION.xcframework.zip" "$IN/apple/KurmanciFFI-v$VERSION.xcframework.zip"
download "$RELEASE_BASE_URL/swift-v$VERSION/release-manifest.json" "$IN/apple/release-manifest.json"
AAR_SHA="$(sha256_of "$IN/android/$MAVEN_ARTIFACT-$VERSION.aar")"
XC_SHA="$(sha256_of "$IN/apple/KurmanciFFI-v$VERSION.xcframework.zip")"
echo "  AAR sha256 $AAR_SHA"
echo "  XCFramework sha256 $XC_SHA"
git -C "$REPO_ROOT" show "$COMMIT:ffi/include/kurmanci.h" > "$WORK/kurmanci.h.at-commit" 2>/dev/null || fail "ffi/include/kurmanci.h not found at $COMMIT"
read -r ABI_MAJOR ABI_MINOR < <(read_c_abi_version_from_header "$WORK/kurmanci.h.at-commit")
python3 - "$IN/apple/release-manifest.json" "$VERSION" "$COMMIT" "$XC_SHA" "$ABI_MAJOR" "$ABI_MINOR" "$ACCEPT_MANIFEST_ABI" <<'EOF'
import json, sys
path, v, commit, xc, major, minor, accept = sys.argv[1:8]
m = json.load(open(path))
expected = {
    "schema_version": "apple-sdk-release-v1", "sdk_version": v, "source_repository": "Kurdi-Language/kurmanci",
    "source_tag": f"swift-v{v}", "source_commit": commit, "distribution_repository": "Kurdi-Language/kurmanci-swift",
    "distribution_tag": v,
}
for k, want in expected.items():
    if m.get(k) != want:
        print(f"❌ release-manifest.json {k} is {m.get(k)!r}, expected {want!r}" + (" (differs from COMMIT)" if k == "source_commit" else ""), file=sys.stderr); sys.exit(1)
if m.get("artifact_sha256") != xc:
    print(f"❌ release-manifest.json artifact_sha256 {m.get('artifact_sha256')} differs from the downloaded XCFramework {xc}", file=sys.stderr); sys.exit(1)
recorded = f"{m.get('c_abi_major')}.{m.get('c_abi_minor')}"
header = f"{major}.{minor}"
if recorded != header:
    historical = (v == "0.1.1" and recorded == "1.0" and header == "1.1")
    if historical and accept == "1.0":
        print(f"⚠️  release-manifest.json records C ABI 1.0 while the header at the release commit says 1.1: the known historical defect of release 0.1.1 (its manifest generator did not derive the ABI from the header), acknowledged with --accept-apple-manifest-abi 1.0")
    elif historical:
        print(f"❌ release-manifest.json records C ABI 1.0 but ffi/include/kurmanci.h at the release commit says 1.1; this is the known historical defect of release 0.1.1 and must be acknowledged with --accept-apple-manifest-abi 1.0", file=sys.stderr); sys.exit(1)
    else:
        print(f"❌ release-manifest.json records C ABI {recorded} but ffi/include/kurmanci.h at the release commit says {header}; no exception applies to release {v}", file=sys.stderr); sys.exit(1)
else:
    print(f"✅ release-manifest.json: apple-sdk-release-v1 for {v} at {commit[:7]}, artifact {xc[:12]}…, C ABI {recorded} as the header")
EOF

echo "=== 6. Swift distribution tag $VERSION in $SWIFT_DIST_REPO"
DIST_COMMIT="$(remote_tag_commit "$SWIFT_DIST_REPO" "$VERSION")"
[[ -n "$DIST_COMMIT" ]] || fail "tag $VERSION does not exist in $SWIFT_DIST_REPO"
gh_contents "$SWIFT_DIST_REPO" "source-manifest.json" "$VERSION" > "$IN/apple/source-manifest.json" || fail "cannot read source-manifest.json of $SWIFT_DIST_REPO at tag $VERSION"
gh_contents "$SWIFT_DIST_REPO" "Package.swift" "$VERSION" > "$IN/apple/Package.swift" || fail "cannot read Package.swift of $SWIFT_DIST_REPO at tag $VERSION"
DIST_URL="$(python3 - "$IN/apple/source-manifest.json" "$IN/apple/Package.swift" "$VERSION" "$COMMIT" "$XC_SHA" "${KURMANCI_PUBLISH_ALLOW_LOCAL_BINARY_URL:-0}" <<'EOF'
import json, re, sys
sm_path, pkg_path, v, commit, xc, allow_local = sys.argv[1:7]
sm = json.load(open(sm_path))
for k, want in {"schema_version": "swift-package-sources-v1", "version": v, "source_commit": commit, "binary_target_checksum": xc}.items():
    if sm.get(k) != want:
        print(f"❌ source-manifest.json {k} is {sm.get(k)!r}, expected {want!r}" + (" (differs from COMMIT)" if k == "source_commit" else ""), file=sys.stderr); sys.exit(1)
pkg = open(pkg_path, encoding="utf-8").read()
url = re.search(r'url:\s*"([^"]+)"', pkg)
checksum = re.search(r'checksum:\s*"([0-9a-f]{64})"', pkg)
if not url or not checksum:
    print("❌ Package.swift has no binary target url/checksum", file=sys.stderr); sys.exit(1)
if checksum.group(1) != xc:
    print(f"❌ Package.swift binary target checksum {checksum.group(1)} differs from the release XCFramework {xc}", file=sys.stderr); sys.exit(1)
u = url.group(1)
expected_suffix = f"/Frameworks/KurmanciFFI-v{v}.xcframework.zip"
immutable = re.fullmatch(r"https://raw\.githubusercontent\.com/Kurdi-Language/kurmanci-swift/[0-9a-f]{40}" + re.escape(expected_suffix), u)
if not immutable and not (allow_local == "1" and u.endswith(expected_suffix)):
    print(f"❌ Package.swift binary URL is not the immutable distribution URL: {u}", file=sys.stderr); sys.exit(1)
print(u)
EOF
)" || exit 1
download "$DIST_URL" "$IN/apple/from-swift-package.zip"
[[ "$(sha256_of "$IN/apple/from-swift-package.zip")" == "$XC_SHA" ]] || fail "$SWIFT_DIST_REPO tag $VERSION wraps a different XCFramework than release swift-v$VERSION"
echo "✅ $SWIFT_DIST_REPO tag $VERSION ($DIST_COMMIT): source-manifest for $VERSION at ${COMMIT:0:7}, binary target checksum and bytes equal the release XCFramework"

echo "=== 7. clean clone at $COMMIT"
CLONE="$WORK/clone"; rm -rf "$CLONE"
git clone --quiet --no-hardlinks "$REPO_ROOT" "$CLONE"
git -C "$CLONE" checkout --quiet --detach "$COMMIT"
[[ -z "$(git -C "$CLONE" status --porcelain)" ]] || fail "clone is not clean"
if [[ -n "${KURMANCI_DATA_BUILDER_BIN:-}" ]]; then
  B="$KURMANCI_DATA_BUILDER_BIN"
else
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$WORK/target}"
  ( cd "$CLONE" && cargo build --quiet --release -p kurmanci-data-builder )
  B="$CARGO_TARGET_DIR/release/data-builder"
fi
(
  cd "$CLONE"
  step() { "$B" "$@" >> "$WORK/derivation.log" 2>&1 || { echo "❌ step failed: $*" >&2; tail -n 40 "$WORK/derivation.log" >&2; exit 1; }; }
  : > "$WORK/derivation.log"
  step import-hunspell kurdish-hunspell-kmr
  step audit-lexicon kurdish-hunspell-kmr
  step generate-review-queues kurdish-hunspell-kmr
  step validate-review-decisions kurdish-hunspell-kmr
  for pack in seed reviewed experimental-full; do step build-pack "$pack"; done
  [[ -z "$(git status --porcelain)" ]] || { echo "❌ derivation left the clone dirty:" >&2; git status --short >&2; exit 1; }
  mkdir -p dist/release
  "$B" build-release-bundle --out dist/release \
    --apple "$IN/apple/KurmanciFFI-v$VERSION.xcframework.zip" \
    --android "$IN/android/$MAVEN_ARTIFACT-$VERSION.aar" --json > "$WORK/bundle.json"
  "$B" verify-release-bundle "dist/release/$BUNDLE_NAME" | tail -n 1
)

echo "=== 8. bundle identity"
python3 - "$WORK/bundle.json" "$VERSION" "$COMMIT" "$XC_SHA" "$AAR_SHA" <<'EOF'
import json, sys
o = json.load(open(sys.argv[1])); p = o["provenance"]; v, commit, xc, aar = sys.argv[2:6]
problems = []
if o["release_kind"] != "production": problems.append(f"release_kind is {o['release_kind']}")
if p["source"]["worktree_dirty"]: problems.append("worktree_dirty is true")
if p["release_version"] != v: problems.append(f"release_version is {p['release_version']}")
if p["source"]["commit"] != commit: problems.append(f"provenance.source.commit {p['source']['commit']} differs from COMMIT {commit}")
want = {("apple", f"apple/KurmanciFFI-v{v}.xcframework.zip"): xc, ("android", f"android/kurmanci-android-{v}.aar"): aar}
got = {(a.get("platform"), a.get("path")): a.get("sha256") for a in p.get("platform_artifacts", [])}
if got != want: problems.append(f"platform artifacts {got} differ from the expected records {want}")
if problems:
    print("❌ " + "; ".join(problems), file=sys.stderr); sys.exit(1)
print(f"✅ production bundle {v} from {commit[:7]}, clean tree, SHA256SUMS sha256 {o['sha256sums_sha256']}, both platform artifacts recorded with the downloaded hashes")
EOF

echo "=== 9. archive and notes"
OUT="$WORK/out"; rm -rf "$OUT"; mkdir -p "$OUT"
BD="$CLONE/dist/release/$BUNDLE_NAME"
if tar --version 2>/dev/null | grep -q GNU; then
  tar --owner=0 --group=0 --numeric-owner --mtime="@0" --sort=name -czf "$OUT/$BUNDLE_NAME.tar.gz" -C "$CLONE/dist/release" "$BUNDLE_NAME"
else
  COPYFILE_DISABLE=1 tar --no-xattrs -czf "$OUT/$BUNDLE_NAME.tar.gz" -C "$CLONE/dist/release" "$BUNDLE_NAME" 2>/dev/null \
    || tar -czf "$OUT/$BUNDLE_NAME.tar.gz" -C "$CLONE/dist/release" "$BUNDLE_NAME"
fi
( cd "$OUT" && sha256_of "$BUNDLE_NAME.tar.gz" | awk -v n="$BUNDLE_NAME.tar.gz" '{print $1 "  " n}' > "$BUNDLE_NAME.tar.gz.sha256" && cat "$BUNDLE_NAME.tar.gz.sha256" )
cp "$BD/SHA256SUMS" "$OUT/SHA256SUMS"; cp "$BD/provenance.json" "$OUT/provenance.json"
python3 - "$WORK/bundle.json" "$TEMPLATE" "$OUT/release-notes.md" "$VERSION" "$KIT_COMMIT" <<'EOF'
import json, re, sys
o = json.load(open(sys.argv[1])); p = o["provenance"]; v = sys.argv[4]; kit = sys.argv[5]
arts = {a["path"].split("/")[0]: a["sha256"] for a in p["platform_artifacts"]}
packs = {x["pack_id"]: x["entry_count"] for x in p["packs"]}
abi = p["c_abi_version"]
values = {
    "__VERSION__": v, "__COMMIT__": p["source"]["commit"], "__SUMS__": o["sha256sums_sha256"], "__KIT_COMMIT__": kit,
    "__AAR__": arts["android"], "__XC__": arts["apple"], "__ENGINE__": p["engine_version"],
    "__ABI__": f"{abi['major']}.{abi['minor']}", "__PACK_SCHEMA__": str(p["pack_schema_version"]),
    "__LM_SCHEMA__": str(p["language_model_schema_version"]),
    "__SEED__": f"{packs['seed']:,}", "__REVIEWED__": f"{packs['reviewed']:,}", "__EXPERIMENTAL__": f"{packs['experimental-full']:,}",
}
t = open(sys.argv[2], encoding="utf-8").read()
for k, val in values.items():
    t = t.replace(k, val)
left = sorted(set(re.findall(r"__[A-Z_]+__", t)))
if left:
    print(f"❌ unresolved placeholder(s) in the release notes: {', '.join(left)}", file=sys.stderr); sys.exit(1)
open(sys.argv[3], "w", encoding="utf-8").write(t)
EOF
echo "  notes: $OUT/release-notes.md"

if [[ $SKIP_PUBLISH -eq 1 ]]; then
  echo "✅ --skip-publish: all checks passed; bundle, archive, .sha256, SHA256SUMS, provenance.json and notes are under $OUT (nothing published, no tag created)"
  exit 0
fi
echo "=== 10. tag and release v$VERSION"
if [[ -z "$EXISTING_TAG" ]]; then
  git -C "$REPO_ROOT" rev-parse --verify --quiet "refs/tags/v$VERSION" >/dev/null || git -C "$REPO_ROOT" tag "v$VERSION" "$COMMIT"
  git -C "$REPO_ROOT" push --quiet origin "refs/tags/v$VERSION"
  # Verify on the remote the tag was pushed to (the peeled commit for an annotated tag).
  PUSHED="$(git -C "$REPO_ROOT" ls-remote origin "refs/tags/v$VERSION" "refs/tags/v$VERSION^{}" | awk '{sha=$1} END {print sha}')"
  [[ "$PUSHED" == "$COMMIT" ]] || fail "tag v$VERSION resolves to '$PUSHED' on origin after the push, not to $COMMIT"
  echo "  pushed tag v$VERSION at $COMMIT"
fi
gh release create "v$VERSION" --repo "$GH_REPO" --verify-tag --title "Kurmancî ku-Latn $VERSION" \
  --notes-file "$OUT/release-notes.md" \
  "$OUT/$BUNDLE_NAME.tar.gz" "$OUT/$BUNDLE_NAME.tar.gz.sha256" "$OUT/SHA256SUMS" "$OUT/provenance.json"
echo "✅ published https://github.com/$GH_REPO/releases/tag/v$VERSION"
