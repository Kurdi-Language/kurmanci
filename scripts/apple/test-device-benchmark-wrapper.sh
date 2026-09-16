#!/usr/bin/env bash
# Shell-level check of scripts/apple/device-benchmark.sh: the wrapper must never report a
# measurement when xcodebuild/XCTest fails, even though the XCTest prints its JSON report
# before its final stability assertion. A fake xcodebuild on PATH prints a report line and
# then exits with the given status; the wrapper must propagate a failure and write no report,
# and must write the report on success. Needs only bash (no Xcode).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WRAPPER="$SCRIPT_DIR/device-benchmark.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/out-fail" "$TMP/out-ok"

REPORT='{"schema_version":"device-benchmark-v1","platform":"ios","device_model":"fake","os_version":"0","simulator":true,"pack_file":"benchmark_pack.bin","pack_bytes":1,"pack_sha256":"0000000000000000000000000000000000000000000000000000000000000000","entry_count":1,"pack_format_version":4,"load_ms_median":0.1,"load_ms_min":0.1,"load_ms_max":0.1,"rss_after_load_bytes":1,"rss_after_queries_bytes":1,"operations":[{"name":"known_hit","input":"welat","iterations":1,"p50_us":1,"p95_us":1,"max_us":1,"result_count":1}],"stability_rounds":1,"stable":false}'

make_fake_xcodebuild() {
  local status="$1"
  cat > "$TMP/bin/xcodebuild" <<EOF
#!/usr/bin/env bash
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

# 2. Success: the wrapper writes the report and prints the pack identity.
make_fake_xcodebuild 0
PATH="$TMP/bin:$PATH" "$WRAPPER" --destination "platform=iOS Simulator,id=FAKE" --out "$TMP/out-ok" > "$TMP/ok.log" 2>&1
ls "$TMP/out-ok"/ios-*.json >/dev/null 2>&1 || { echo "❌ no report written on success" >&2; cat "$TMP/ok.log" >&2; exit 1; }
grep -q "pack sha256 0000000000000000" "$TMP/ok.log" || { echo "❌ summary does not print the pack identity" >&2; cat "$TMP/ok.log" >&2; exit 1; }
echo "✅ successful run writes the report and prints the pack identity"
