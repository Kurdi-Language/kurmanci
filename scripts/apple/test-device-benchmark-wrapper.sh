#!/usr/bin/env bash
# Shell-level check of scripts/apple/device-benchmark.sh: the wrapper must never report a
# measurement when xcodebuild/XCTest fails, even though the XCTest prints its JSON report
# before its final stability assertion. A fake xcodebuild on PATH prints a report line and
# then exits with the given status; the wrapper must propagate a failure and write no report,
# and must write the report on success. The fake also records the arguments it receives, so
# the test proves that signing follows the destination type: an explicit simulator
# destination runs unsigned, a physical iOS destination runs with automatic Apple Development
# signing and needs no DEVELOPMENT_TEAM, and any other destination is refused before
# xcodebuild runs; and that --project selects the local or the remote consumer project and
# refuses anything else. Needs only bash (no Xcode).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WRAPPER="$SCRIPT_DIR/device-benchmark.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/out-fail" "$TMP/out-ok" "$TMP/out-device" "$TMP/out-unsupported" "$TMP/out-remote" "$TMP/out-project"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ARGS="$TMP/xcodebuild-args"

REPORT='{"schema_version":"device-benchmark-v1","platform":"ios","device_model":"fake","os_version":"0","simulator":true,"pack_file":"benchmark_pack.bin","pack_bytes":1,"pack_sha256":"0000000000000000000000000000000000000000000000000000000000000000","entry_count":1,"pack_format_version":4,"load_ms_median":0.1,"load_ms_min":0.1,"load_ms_max":0.1,"rss_after_load_bytes":1,"rss_after_queries_bytes":1,"operations":[{"name":"known_hit","input":"welat","iterations":1,"p50_us":1,"p95_us":1,"max_us":1,"result_count":1}],"stability_rounds":1,"stable":false}'

make_fake_xcodebuild() {
  local status="$1"
  cat > "$TMP/bin/xcodebuild" <<EOF
#!/usr/bin/env bash
printf '%s\\n' "\$@" > "$ARGS"
echo "Test Case '-[KurmanciConsumerTests.DeviceBenchmarkTests testDeviceBenchmarkReport]' started."
echo 'KURMANCI_DEVICE_BENCHMARK $REPORT'
if [[ $status -ne 0 ]]; then
  echo "error: -[KurmanciConsumerTests.DeviceBenchmarkTests testDeviceBenchmarkReport] : XCTAssertTrue failed - repeated queries returned different results"
  echo "** TEST FAILED **"
fi
exit $status
EOF
  chmod +x "$TMP/bin/xcodebuild"
}

# 1. XCTest fails after printing its report: the wrapper must fail and write nothing.
make_fake_xcodebuild 65
set +e
PATH="$TMP/bin:$PATH" "$WRAPPER" --destination "platform=iOS Simulator,id=FAKE" --out "$TMP/out-fail" > "$TMP/fail.log" 2>&1
STATUS=$?
set -e
if [[ $STATUS -eq 0 ]]; then
  echo "❌ wrapper reported success although xcodebuild exited 65" >&2
  cat "$TMP/fail.log" >&2
  exit 1
fi
if ls "$TMP/out-fail"/ios-*.json >/dev/null 2>&1; then
  echo "❌ wrapper wrote a benchmark report for a failed run" >&2
  exit 1
fi
grep -q "xcodebuild test failed" "$TMP/fail.log" || { echo "❌ failure not reported" >&2; cat "$TMP/fail.log" >&2; exit 1; }
echo "✅ failed XCTest run propagates exit status $STATUS and writes no report"

# The arguments the fake xcodebuild received, one per line, must contain every expected
# setting and none of the forbidden ones.
expect_args() {
  local label="$1"; shift
  local expected=() forbidden=() arg
  local mode=expected
  for arg in "$@"; do
    if [[ "$arg" == "--not" ]]; then mode=forbidden; continue; fi
    if [[ "$mode" == expected ]]; then expected+=("$arg"); else forbidden+=("$arg"); fi
  done
  for arg in "${expected[@]}"; do
    grep -qxF -- "$arg" "$ARGS" || { echo "❌ $label: xcodebuild did not receive '$arg'" >&2; cat "$ARGS" >&2; exit 1; }
  done
  for arg in "${forbidden[@]}"; do
    if grep -qxF -- "$arg" "$ARGS"; then echo "❌ $label: xcodebuild received '$arg'" >&2; cat "$ARGS" >&2; exit 1; fi
  done
}
expect_args "explicit simulator destination" \
  "-destination" "platform=iOS Simulator,id=FAKE" \
  "CODE_SIGNING_ALLOWED=NO" "CODE_SIGNING_REQUIRED=NO" "CODE_SIGN_IDENTITY=" \
  --not "CODE_SIGNING_ALLOWED=YES" "CODE_SIGNING_REQUIRED=YES" "CODE_SIGN_STYLE=Automatic" "CODE_SIGN_IDENTITY=Apple Development"
echo "✅ explicit simulator destination runs unsigned"

# 2. Success: the wrapper writes the report and prints the pack identity.
make_fake_xcodebuild 0
PATH="$TMP/bin:$PATH" "$WRAPPER" --destination "platform=iOS Simulator,id=FAKE" --out "$TMP/out-ok" > "$TMP/ok.log" 2>&1
ls "$TMP/out-ok"/ios-*.json >/dev/null 2>&1 || { echo "❌ no report written on success" >&2; cat "$TMP/ok.log" >&2; exit 1; }
grep -q "pack sha256 0000000000000000" "$TMP/ok.log" || { echo "❌ summary does not print the pack identity" >&2; cat "$TMP/ok.log" >&2; exit 1; }
echo "✅ successful run writes the report and prints the pack identity"
expect_args "simulator success run" "CODE_SIGNING_ALLOWED=NO" "CODE_SIGNING_REQUIRED=NO" "CODE_SIGN_IDENTITY=" --not "CODE_SIGNING_ALLOWED=YES"

# 3. Physical iOS destination: automatic Apple Development signing, no DEVELOPMENT_TEAM needed
#    from the wrapper itself (the team is the caller's XCODEBUILD_EXTRA_ARGS), report written.
make_fake_xcodebuild 0
env -u XCODEBUILD_EXTRA_ARGS PATH="$TMP/bin:$PATH" "$WRAPPER" --destination "platform=iOS,id=FAKE" --out "$TMP/out-device" > "$TMP/device.log" 2>&1
ls "$TMP/out-device"/ios-*.json >/dev/null 2>&1 || { echo "❌ no report written for the device run" >&2; cat "$TMP/device.log" >&2; exit 1; }
expect_args "physical iOS destination" \
  "-destination" "platform=iOS,id=FAKE" \
  "CODE_SIGNING_ALLOWED=YES" "CODE_SIGNING_REQUIRED=YES" "CODE_SIGN_STYLE=Automatic" "CODE_SIGN_IDENTITY=Apple Development" \
  --not "CODE_SIGNING_ALLOWED=NO" "CODE_SIGNING_REQUIRED=NO" "CODE_SIGN_IDENTITY="
if grep -q "DEVELOPMENT_TEAM" "$ARGS"; then echo "❌ wrapper injected a DEVELOPMENT_TEAM of its own" >&2; exit 1; fi
echo "✅ physical iOS destination runs with automatic Apple Development signing"

# 4. The caller's XCODEBUILD_EXTRA_ARGS reach xcodebuild unchanged (the documented device call).
make_fake_xcodebuild 0
XCODEBUILD_EXTRA_ARGS="DEVELOPMENT_TEAM=FAKETEAM00 -allowProvisioningUpdates" PATH="$TMP/bin:$PATH" "$WRAPPER" --destination "platform=iOS,id=FAKE" --out "$TMP/out-device" > "$TMP/device2.log" 2>&1
expect_args "device run with extra args" "DEVELOPMENT_TEAM=FAKETEAM00" "-allowProvisioningUpdates" "CODE_SIGN_STYLE=Automatic"
echo "✅ XCODEBUILD_EXTRA_ARGS are passed through to xcodebuild"

# 5. Any other destination is refused before xcodebuild runs and writes no report.
rm -f "$ARGS"
make_fake_xcodebuild 0
set +e
PATH="$TMP/bin:$PATH" "$WRAPPER" --destination "platform=macOS" --out "$TMP/out-unsupported" > "$TMP/unsupported.log" 2>&1
STATUS=$?
set -e
[[ $STATUS -ne 0 ]] || { echo "❌ wrapper accepted an unsupported destination" >&2; exit 1; }
[[ ! -f "$ARGS" ]] || { echo "❌ xcodebuild ran for an unsupported destination" >&2; exit 1; }
if ls "$TMP/out-unsupported"/ios-*.json >/dev/null 2>&1; then echo "❌ report written for an unsupported destination" >&2; exit 1; fi
grep -q "unsupported destination" "$TMP/unsupported.log" || { echo "❌ refusal not reported" >&2; cat "$TMP/unsupported.log" >&2; exit 1; }
echo "✅ unsupported destination is refused before xcodebuild runs"

# 6. --project selects the consumer test host: local (default) or remote; anything else is
#    refused before xcodebuild runs.
make_fake_xcodebuild 0
PATH="$TMP/bin:$PATH" "$WRAPPER" --destination "platform=iOS Simulator,id=FAKE" --out "$TMP/out-ok" > "$TMP/default.log" 2>&1
expect_args "default project" "-project" "$REPO_ROOT/integration/apple/ios-consumer/KurmanciConsumer.xcodeproj" \
  --not "$REPO_ROOT/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj"
make_fake_xcodebuild 0
PATH="$TMP/bin:$PATH" "$WRAPPER" --project remote --destination "platform=iOS Simulator,id=FAKE" --out "$TMP/out-remote" > "$TMP/remote.log" 2>&1
ls "$TMP/out-remote"/ios-*.json >/dev/null 2>&1 || { echo "❌ no report written for the remote-project run" >&2; cat "$TMP/remote.log" >&2; exit 1; }
expect_args "remote project" "-project" "$REPO_ROOT/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj" \
  --not "$REPO_ROOT/integration/apple/ios-consumer/KurmanciConsumer.xcodeproj"
echo "✅ --project remote runs the remote consumer test host"
rm -f "$ARGS"
set +e
PATH="$TMP/bin:$PATH" "$WRAPPER" --project other --destination "platform=iOS Simulator,id=FAKE" --out "$TMP/out-project" > "$TMP/project.log" 2>&1
STATUS=$?
set -e
[[ $STATUS -ne 0 ]] || { echo "❌ wrapper accepted an unsupported --project" >&2; exit 1; }
[[ ! -f "$ARGS" ]] || { echo "❌ xcodebuild ran for an unsupported --project" >&2; exit 1; }
grep -q "unsupported --project" "$TMP/project.log" || { echo "❌ --project refusal not reported" >&2; cat "$TMP/project.log" >&2; exit 1; }
echo "✅ unsupported --project is refused before xcodebuild runs"
