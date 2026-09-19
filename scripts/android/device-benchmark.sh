#!/usr/bin/env bash
# Runs the internal device benchmark harness (DeviceBenchmarkTest) of the standalone Android
# consumer on the connected device or emulator and collects its JSON report.
#
# Usage: scripts/android/device-benchmark.sh [--pack PATH] [--out DIR]
#   --pack PATH   pack to measure (default: the committed placeholder benchmark_pack.bin, a
#                 tiny fixture). The file is swapped into the consumer's test assets for the
#                 run and the placeholder is restored afterwards.
#   --out DIR     where to write the report (default: dist/device-benchmarks)
# Requires: a device/emulator visible to adb, and the packaged AAR in dist/android/maven
# (scripts/android/build-aar.sh). No timing gate; the numbers are recorded for the report.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONSUMER="$REPO_ROOT/integration/android/android-consumer"
ASSET="$CONSUMER/app/src/androidTest/assets/benchmark_pack.bin"
PACK=""
OUT_DIR="$REPO_ROOT/dist/device-benchmarks"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --pack) PACK="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

DEFAULT_GROUP="$(grep '^kurmanciMavenGroup=' "$REPO_ROOT/android/gradle.properties" 2>/dev/null | cut -d'=' -f2 | tr -d ' \r\n' || echo 'io.github.ferhatguneri')"
GROUP_ID="${GROUP_ID:-$DEFAULT_GROUP}"
# shellcheck source=scripts/android/kurmanci-version.sh
source "$SCRIPT_DIR/kurmanci-version.sh"
VERSION="$(resolve_kurmanci_version "$REPO_ROOT")"

command -v adb >/dev/null 2>&1 || { echo "❌ adb not found" >&2; exit 1; }
adb devices | grep -q "device$" || { echo "❌ no Android device/emulator visible to adb" >&2; exit 1; }

BACKUP="$(mktemp)"
cp "$ASSET" "$BACKUP"
restore() { cp "$BACKUP" "$ASSET"; rm -f "$BACKUP"; }
trap restore EXIT
if [[ -n "$PACK" ]]; then
  [[ -f "$PACK" ]] || { echo "❌ pack not found: $PACK" >&2; exit 1; }
  cp "$PACK" "$ASSET"
  echo "measuring pack: $PACK ($(wc -c < "$PACK" | tr -d ' ') bytes)"
else
  echo "measuring the committed placeholder pack (pass --pack for a real one)"
fi

cd "$CONSUMER"
if [[ ! -f "./gradlew" ]]; then
  # Only the wrapper: the glob android/gradle* would also copy android/gradle.properties over
  # the consumer's own tracked gradle.properties.
  cp -r "$REPO_ROOT/android/gradle" .
  cp "$REPO_ROOT/android/gradlew"* .
fi
chmod +x ./gradlew
export CONSUMER_MODE="${CONSUMER_MODE:-local}"
adb logcat -c || true
./gradlew --quiet connectedDebugAndroidTest \
  -Pandroid.testInstrumentationRunnerArguments.class=org.kurmanci.consumer.DeviceBenchmarkTest \
  -PkurmanciMavenGroup="$GROUP_ID" -PkurmanciVersion="$VERSION"

JSON="$(adb logcat -d -s KurmanciDeviceBenchmark:I | grep -o 'KURMANCI_DEVICE_BENCHMARK .*' | tail -n1 | sed 's/^KURMANCI_DEVICE_BENCHMARK //')"
if [[ -z "$JSON" ]]; then
  echo "❌ no benchmark report found in logcat" >&2
  exit 1
fi
mkdir -p "$OUT_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
MODEL="$(printf '%s' "$JSON" | python3 -c 'import json,sys,re; print(re.sub(r"[^A-Za-z0-9]+","-",json.load(sys.stdin)["device_model"]).strip("-"))' 2>/dev/null || echo device)"
REPORT="$OUT_DIR/android-$MODEL-$STAMP.json"
printf '%s\n' "$JSON" > "$REPORT"
echo "report: $REPORT"
python3 - "$REPORT" <<'EOF'
import json, sys
r = json.load(open(sys.argv[1]))
print(f"{r['device_model']} / {r['os_version']} / {r.get('abi','')}{' (emulator)' if r.get('simulator') else ''}")
print(f"pack sha256 {r.get('pack_sha256','?')}, {r['pack_bytes']} bytes, {r['entry_count']} entries, format {r['pack_format_version']}; load median {r['load_ms_median']:.2f} ms; RSS after load {r['rss_after_load_bytes']/1e6:.1f} MB, after queries {r['rss_after_queries_bytes']/1e6:.1f} MB; stable over {r['stability_rounds']} rounds: {r['stable']}")
print(f"{'operation':<12}{'input':<10}{'p50 us':>10}{'p95 us':>10}{'max us':>10}  results")
for op in r['operations']:
    print(f"{op['name']:<12}{op['input']:<10}{op['p50_us']:>10.1f}{op['p95_us']:>10.1f}{op['max_us']:>10.1f}  {op['result_count']}")
EOF
