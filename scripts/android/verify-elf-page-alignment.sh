#!/usr/bin/env bash
# Fail-closed 16 KB page-size check of one Android shared library.
#
# Usage: scripts/android/verify-elf-page-alignment.sh <libkurmanci_jni.so> <label>
#   <label> names the ABI (and, for a packaged artifact, the container) in every message.
#
# Invariants (what the Android loader requires on 16 KB-page devices):
#   1. every LOAD program header has Align >= 0x4000;
#   2. a GNU_RELRO program header exists (RELRO must stay enabled) and
#      (VirtAddr + MemSiz) % 0x4000 == 0.
#
# The program headers are read with `readelf -lW` from, in order: $READELF (explicit override,
# used by the regression test), the NDK's llvm-readelf under $ANDROID_NDK_HOME/$ANDROID_NDK_ROOT,
# llvm-readelf on PATH, readelf on PATH. The parser joins wrapped program-header lines, so GNU
# readelf's two-line 64-bit layout, its wide layout and llvm-readelf's layout all parse the
# same way; scripts/android/test-verify-elf-page-alignment.sh covers all three with fixtures.
set -euo pipefail

SO="${1:-}"
LABEL="${2:-${1:-}}"
[[ -n "$SO" ]] || { echo "usage: $0 <shared-library> [label]" >&2; exit 2; }
[[ -f "$SO" ]] || { echo "❌ $LABEL: file not found: $SO" >&2; exit 2; }
PAGE=16384

READELF="${READELF:-}"
if [[ -z "$READELF" ]]; then
  for candidate in "${ANDROID_NDK_HOME:-/nonexistent}"/toolchains/llvm/prebuilt/*/bin/llvm-readelf "${ANDROID_NDK_ROOT:-/nonexistent}"/toolchains/llvm/prebuilt/*/bin/llvm-readelf; do
    if [[ -x "$candidate" ]]; then READELF="$candidate"; break; fi
  done
fi
if [[ -z "$READELF" ]] && command -v llvm-readelf >/dev/null 2>&1; then READELF="$(command -v llvm-readelf)"; fi
if [[ -z "$READELF" ]] && command -v readelf >/dev/null 2>&1; then READELF="$(command -v readelf)"; fi
[[ -n "$READELF" ]] || { echo "❌ $LABEL: no llvm-readelf/readelf found (set ANDROID_NDK_HOME or READELF)" >&2; exit 2; }

# One record per program header: "<Type> <VirtAddr> <MemSiz> <Align>". Inside the
# "Program Headers:" section a line whose first token is a header type starts a record and a
# line whose first token is a 0x… number continues the previous one (GNU readelf wraps 64-bit
# headers over two lines). Columns are positional: Type Offset VirtAddr PhysAddr FileSiz MemSiz
# Flg… Align, and Align is always the last token.
RECORDS="$("$READELF" -lW "$SO" | awk '
  function flush() { if (n > 0) { print tok[1], tok[3], tok[6], tok[n]; n = 0 } }
  /^Program Headers:/ { inph = 1; next }
  inph && /^ *Section to Segment/ { flush(); inph = 0; next }
  inph && $1 == "Type" { next }
  inph && $1 ~ /^[A-Za-z]/ { flush(); for (i = 1; i <= NF; i++) tok[++n] = $i; next }
  inph && $1 ~ /^0x/ && n > 0 { for (i = 1; i <= NF; i++) tok[++n] = $i; next }
  END { flush() }
')"
[[ -n "$RECORDS" ]] || { echo "❌ $LABEL: no program headers parsed from $SO ($READELF -lW)" >&2; exit 1; }

loads=0
relro_seen=0
load_aligns=()
while read -r ptype vaddr memsiz align; do
  case "$ptype" in
    LOAD)
      loads=$((loads + 1))
      align=$((align))
      load_aligns+=("$align")
      if (( align < PAGE )); then
        echo "❌ $LABEL: LOAD segment aligned to $align bytes ($(printf '0x%x' "$align")); 16 KB-page devices need at least $PAGE. File: $SO. Link with -Wl,-z,max-page-size=16384 (.cargo/config.toml)." >&2
        exit 1
      fi
      ;;
    GNU_RELRO)
      relro_seen=1
      vaddr=$((vaddr)); memsiz=$((memsiz))
      end=$((vaddr + memsiz))
      rem=$((end % PAGE))
      relro_summary="GNU_RELRO VirtAddr=$(printf '0x%x' "$vaddr") MemSiz=$(printf '0x%x' "$memsiz") end=$(printf '0x%x' "$end") end%0x4000=$rem"
      if (( rem != 0 )); then
        echo "❌ $LABEL: $relro_summary; the RELRO region must end on a 16 KB boundary. File: $SO. Link with -Wl,-z,common-page-size=16384 as well (.cargo/config.toml); do not disable RELRO." >&2
        exit 1
      fi
      ;;
  esac
done <<< "$RECORDS"

(( loads > 0 )) || { echo "❌ $LABEL: no LOAD program header found in $SO" >&2; exit 1; }
if (( relro_seen == 0 )); then
  echo "❌ $LABEL: no GNU_RELRO program header in $SO; RELRO must stay enabled (do not pass -z norelro)." >&2
  exit 1
fi
echo "✅ $LABEL: LOAD align$(printf ' 0x%x' "${load_aligns[@]}"); $relro_summary"
