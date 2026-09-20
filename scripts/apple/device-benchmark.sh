#!/usr/bin/env bash
# Runs the internal device benchmark harness (DeviceBenchmarkTests) of the iOS consumer test
# host on a simulator or a real iPhone and collects its JSON report.
#
# Usage: scripts/apple/device-benchmark.sh [--pack PATH] [--destination DEST] [--out DIR] [--project local|remote]
#   --pack PATH          pack to measure (default: the committed placeholder benchmark_pack.bin,
#                        a tiny fixture). The file is swapped into integration/apple/fixtures
#                        for the run and the placeholder is restored afterwards.
#   --project local|remote
#                        which consumer test host runs the benchmark (default: local).
#                        "local" is integration/apple/ios-consumer on the locally built Swift
#                        package (needs the Rust toolchain and scripts/apple/build-xcframework.sh
#                        first); "remote" is integration/apple/ios-remote-consumer on the
#                        published Kurdi-Language/kurmanci-swift package at the version the
#                        project pins, so no Rust toolchain is needed (vendor evaluation kit).
#   --destination DEST   xcodebuild destination (default: the first available iPhone simulator).
#                        "platform=iOS Simulator,id=<UDID>" runs unsigned; "platform=iOS,id=<UDID>"
#                        (a real device) runs with automatic Apple Development signing, the team
#                        coming from XCODEBUILD_EXTRA_ARGS, e.g.
#                        XCODEBUILD_EXTRA_ARGS="DEVELOPMENT_TEAM=ABCDE12345 -allowProvisioningUpdates"
#   --out DIR            where to write the report (default: dist/device-benchmarks)
# No timing gate; the numbers are recorded for the report.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIXTURE="$REPO_ROOT/integration/apple/fixtures/benchmark_pack.bin"
PROJECT_KIND="local"
PACK=""
DEST=""
OUT_DIR="$REPO_ROOT/dist/device-benchmarks"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --pack) PACK="$2"; shift 2 ;;
    --destination) DEST="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --project) PROJECT_KIND="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done
case "$PROJECT_KIND" in
  local) PROJECT="$REPO_ROOT/integration/apple/ios-consumer/KurmanciConsumer.xcodeproj" ;;
  remote) PROJECT="$REPO_ROOT/integration/apple/ios-remote-consumer/KurmanciConsumer.xcodeproj" ;;
  *) echo "❌ unsupported --project '$PROJECT_KIND': expected 'local' or 'remote'" >&2; exit 1 ;;
esac

command -v xcodebuild >/dev/null 2>&1 || { echo "❌ xcodebuild not found" >&2; exit 1; }
if [[ -z "$DEST" ]]; then
  SIM_UDID="$(xcrun simctl list devices available --json | python3 -c '
import json, sys
data = json.load(sys.stdin)
for runtime, devices in data.get("devices", {}).items():
    if "iOS" in runtime:
        for d in devices:
            if d.get("isAvailable") and "iPhone" in d.get("name", ""):
                print(d["udid"]); sys.exit(0)
')"
  [[ -n "$SIM_UDID" ]] || { echo "❌ no available iPhone simulator; pass --destination" >&2; exit 1; }
  DEST="platform=iOS Simulator,id=$SIM_UDID"
fi

# Signing follows the destination type, not whether it was supplied: a simulator runs the
# host unsigned (the project's own setting); a real device needs a signed test host, so
# automatic Apple Development signing is enabled and the team comes from XCODEBUILD_EXTRA_ARGS
# (DEVELOPMENT_TEAM=... -allowProvisioningUpdates), which Xcode uses to create the development
# certificate and provisioning profile on first use. Anything else is refused.
destination_kind() {
  if [[ "$1" =~ (^|,)platform=iOS\ Simulator(,|$) ]]; then echo simulator
  elif [[ "$1" =~ (^|,)platform=iOS(,|$) ]]; then echo device
  else echo unsupported
  fi
}
case "$(destination_kind "$DEST")" in
  simulator) SIGNING=(CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY="") ;;
  device) SIGNING=(CODE_SIGNING_ALLOWED=YES CODE_SIGNING_REQUIRED=YES CODE_SIGN_STYLE=Automatic CODE_SIGN_IDENTITY="Apple Development") ;;
  *) echo "❌ unsupported destination '$DEST': expected 'platform=iOS Simulator,...' or 'platform=iOS,...'" >&2; exit 1 ;;
esac

BACKUP="$(mktemp)"
cp "$FIXTURE" "$BACKUP"
restore() { cp "$BACKUP" "$FIXTURE"; rm -f "$BACKUP"; }
trap restore EXIT
if [[ -n "$PACK" ]]; then
  [[ -f "$PACK" ]] || { echo "❌ pack not found: $PACK" >&2; exit 1; }
  cp "$PACK" "$FIXTURE"
  echo "measuring pack: $PACK ($(wc -c < "$PACK" | tr -d ' ') bytes)"
else
  echo "measuring the committed placeholder pack (pass --pack for a real one)"
fi

LOG="$(mktemp)"
export REPO_ROOT
mkdir -p "$OUT_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
# The XCTest prints its JSON report before its final stability assertion, so the outcome of
# the run is xcodebuild's exit status, never the filter's. Capture it from PIPESTATUS.
set +e
# shellcheck disable=SC2086
xcodebuild test \
  -project "$PROJECT" \
  -scheme KurmanciConsumer \
  -destination "$DEST" \
  -only-testing:KurmanciConsumerTests/DeviceBenchmarkTests \
  "${SIGNING[@]}" ${XCODEBUILD_EXTRA_ARGS:-} 2>&1 | tee "$LOG" | grep -E 'Test Suite|Test Case|error:|BUILD|TEST'
XCODE_STATUS=${PIPESTATUS[0]}
set -e
if [[ $XCODE_STATUS -ne 0 ]]; then
  FAILED_LOG="$OUT_DIR/failed-xcodebuild-$STAMP.log"
  cp "$LOG" "$FAILED_LOG"
  rm -f "$LOG"
  echo "❌ xcodebuild test failed (exit $XCODE_STATUS); this run is not a measurement. Full log: $FAILED_LOG" >&2
  tail -n 40 "$FAILED_LOG" >&2
  exit "$XCODE_STATUS"
fi

JSON="$(grep -o 'KURMANCI_DEVICE_BENCHMARK .*' "$LOG" | tail -n1 | sed 's/^KURMANCI_DEVICE_BENCHMARK //')"
rm -f "$LOG"
if [[ -z "$JSON" ]]; then
  echo "❌ no benchmark report found in the xcodebuild output" >&2
  exit 1
fi
MODEL="$(printf '%s' "$JSON" | python3 -c 'import json,sys,re; print(re.sub(r"[^A-Za-z0-9]+","-",json.load(sys.stdin)["device_model"]).strip("-"))' 2>/dev/null || echo device)"
REPORT="$OUT_DIR/ios-$MODEL-$STAMP.json"
printf '%s\n' "$JSON" > "$REPORT"
echo "report: $REPORT"
python3 - "$REPORT" <<'EOF'
import json, sys
r = json.load(open(sys.argv[1]))
print(f"{r['device_model']} / {r['os_version']}{' (simulator)' if r.get('simulator') else ''}")
print(f"pack sha256 {r.get('pack_sha256','?')}, {r['pack_bytes']} bytes, {r['entry_count']} entries, format {r['pack_format_version']}; load median {r['load_ms_median']:.2f} ms; RSS after load {r['rss_after_load_bytes']/1e6:.1f} MB, after queries {r['rss_after_queries_bytes']/1e6:.1f} MB; stable over {r['stability_rounds']} rounds: {r['stable']}")
print(f"{'operation':<12}{'input':<10}{'p50 us':>10}{'p95 us':>10}{'max us':>10}  results")
for op in r['operations']:
    print(f"{op['name']:<12}{op['input']:<10}{op['p50_us']:>10.1f}{op['p95_us']:>10.1f}{op['max_us']:>10.1f}  {op['result_count']}")
EOF
