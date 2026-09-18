#!/usr/bin/env bash
# Deterministic regression test of scripts/android/verify-elf-page-alignment.sh. A fake
# readelf on $READELF prints canned program-header listings, so the test needs no NDK, no
# real library and does not depend on the host readelf's formatting. Fixtures cover
# llvm-readelf's layout, GNU readelf's wide layout and GNU readelf's wrapped two-line 64-bit
# layout, each accepted with the same parsed values; then the rejections.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERIFY="$SCRIPT_DIR/verify-elf-page-alignment.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
: > "$TMP/fake.so"
cat > "$TMP/readelf" <<'FAKE'
#!/usr/bin/env bash
cat "$FIXTURE"
FAKE
chmod +x "$TMP/readelf"

# Valid: every LOAD 0x4000; GNU_RELRO 0xc7560 + 0x8aa0 = 0xd0000, a multiple of 0x4000.
cat > "$TMP/llvm-valid.txt" <<'FX'

Elf file type is DYN (Shared object file)
Entry point 0x0
There are 9 program headers, starting at offset 64

Program Headers:
  Type           Offset   VirtAddr           PhysAddr           FileSiz  MemSiz   Flg Align
  PHDR           0x000040 0x0000000000000040 0x0000000000000040 0x0001f8 0x0001f8 R   0x8
  LOAD           0x000000 0x0000000000000000 0x0000000000000000 0x046710 0x046710 R   0x4000
  LOAD           0x046710 0x000000000004a710 0x000000000004a710 0x078e50 0x078e50 R E 0x4000
  LOAD           0x0bf560 0x00000000000c7560 0x00000000000c7560 0x005b98 0x005b98 RW  0x4000
  LOAD           0x0c50f8 0x00000000000d10f8 0x00000000000d10f8 0x0000c8 0x000a40 RW  0x4000
  DYNAMIC        0x0c4c58 0x00000000000ccc58 0x00000000000ccc58 0x000180 0x000180 RW  0x8
  GNU_RELRO      0x0bf560 0x00000000000c7560 0x00000000000c7560 0x005b98 0x008aa0 R   0x1
  GNU_EH_FRAME   0x03e2f4 0x000000000003e2f4 0x000000000003e2f4 0x00841c 0x00841c R   0x4
  GNU_STACK      0x000000 0x0000000000000000 0x0000000000000000 0x000000 0x000000 RW  0x0

 Section to Segment mapping:
  Segment Sections...
   00
   01     .note.android.ident .dynsym
FX
cat > "$TMP/gnu-wide-valid.txt" <<'FX'

Elf file type is DYN (Shared object file)
Entry point 0x0
There are 9 program headers, starting at offset 64

Program Headers:
  Type           Offset             VirtAddr           PhysAddr           FileSiz            MemSiz              Flags  Align
  PHDR           0x0000000000000040 0x0000000000000040 0x0000000000000040 0x00000000000001f8 0x00000000000001f8  R      0x8
  LOAD           0x0000000000000000 0x0000000000000000 0x0000000000000000 0x0000000000046710 0x0000000000046710  R      0x4000
  LOAD           0x0000000000046710 0x000000000004a710 0x000000000004a710 0x0000000000078e50 0x0000000000078e50  R E    0x4000
  LOAD           0x00000000000bf560 0x00000000000c7560 0x00000000000c7560 0x0000000000005b98 0x0000000000005b98  RW     0x4000
  LOAD           0x00000000000c50f8 0x00000000000d10f8 0x00000000000d10f8 0x00000000000000c8 0x0000000000000a40  RW     0x4000
  DYNAMIC        0x00000000000c4c58 0x00000000000ccc58 0x00000000000ccc58 0x0000000000000180 0x0000000000000180  RW     0x8
  GNU_RELRO      0x00000000000bf560 0x00000000000c7560 0x00000000000c7560 0x0000000000005b98 0x0000000000008aa0  R      0x1
  GNU_EH_FRAME   0x000000000003e2f4 0x000000000003e2f4 0x000000000003e2f4 0x000000000000841c 0x000000000000841c  R      0x4
  GNU_STACK      0x0000000000000000 0x0000000000000000 0x0000000000000000 0x0000000000000000 0x0000000000000000  RW     0x0

 Section to Segment mapping:
  Segment Sections...
   00
FX
cat > "$TMP/gnu-wrapped-valid.txt" <<'FX'

Elf file type is DYN (Shared object file)
Entry point 0x0
There are 9 program headers, starting at offset 64

Program Headers:
  Type           Offset             VirtAddr           PhysAddr
                 FileSiz            MemSiz              Flags  Align
  PHDR           0x0000000000000040 0x0000000000000040 0x0000000000000040
                 0x00000000000001f8 0x00000000000001f8  R      0x8
  LOAD           0x0000000000000000 0x0000000000000000 0x0000000000000000
                 0x0000000000046710 0x0000000000046710  R      0x4000
  LOAD           0x0000000000046710 0x000000000004a710 0x000000000004a710
                 0x0000000000078e50 0x0000000000078e50  R E    0x4000
  LOAD           0x00000000000bf560 0x00000000000c7560 0x00000000000c7560
                 0x0000000000005b98 0x0000000000005b98  RW     0x4000
  LOAD           0x00000000000c50f8 0x00000000000d10f8 0x00000000000d10f8
                 0x00000000000000c8 0x0000000000000a40  RW     0x4000
  DYNAMIC        0x00000000000c4c58 0x00000000000ccc58 0x00000000000ccc58
                 0x0000000000000180 0x0000000000000180  RW     0x8
  GNU_RELRO      0x00000000000bf560 0x00000000000c7560 0x00000000000c7560
                 0x0000000000005b98 0x0000000000008aa0  R      0x1
  GNU_EH_FRAME   0x000000000003e2f4 0x000000000003e2f4 0x000000000003e2f4
                 0x000000000000841c 0x000000000000841c  R      0x4
  GNU_STACK      0x0000000000000000 0x0000000000000000 0x0000000000000000
                 0x0000000000000000 0x0000000000000000  RW     0x0

 Section to Segment mapping:
  Segment Sections...
   00
FX
# Rejections derived from the llvm layout.
sed 's/0x078e50 R E 0x4000/0x078e50 R E 0x1000/' "$TMP/llvm-valid.txt" > "$TMP/load-4k.txt"
sed 's/0x005b98 0x008aa0 R   0x1/0x005b98 0x006aa0 R   0x1/' "$TMP/llvm-valid.txt" > "$TMP/relro-misaligned.txt"
grep -v 'GNU_RELRO' "$TMP/llvm-valid.txt" > "$TMP/no-relro.txt"

run() { FIXTURE="$TMP/$1" READELF="$TMP/readelf" "$VERIFY" "$TMP/fake.so" "test:$1" > "$TMP/out" 2>&1; }
EXPECT_SUMMARY='LOAD align 0x4000 0x4000 0x4000 0x4000; GNU_RELRO VirtAddr=0xc7560 MemSiz=0x8aa0 end=0xd0000 end%0x4000=0'

for fx in llvm-valid.txt gnu-wide-valid.txt gnu-wrapped-valid.txt; do
  run "$fx" || { echo "❌ $fx was rejected:"; cat "$TMP/out"; exit 1; }
  grep -qF "$EXPECT_SUMMARY" "$TMP/out" || { echo "❌ $fx: unexpected parsed values:"; cat "$TMP/out"; exit 1; }
  echo "✅ accepted with identical parsed values: $fx"
done

if run load-4k.txt; then echo "❌ LOAD aligned to 0x1000 was accepted"; exit 1; fi
grep -q 'LOAD segment aligned to 4096 bytes (0x1000)' "$TMP/out" || { echo "❌ LOAD rejection message wrong:"; cat "$TMP/out"; exit 1; }
echo "✅ rejected: LOAD alignment 0x1000"

if run relro-misaligned.txt; then echo "❌ misaligned RELRO end was accepted"; exit 1; fi
grep -q 'GNU_RELRO VirtAddr=0xc7560 MemSiz=0x6aa0 end=0xce000 end%0x4000=8192' "$TMP/out" || { echo "❌ RELRO rejection message wrong:"; cat "$TMP/out"; exit 1; }
echo "✅ rejected: LOAD valid but GNU_RELRO end 0xce000 not a multiple of 0x4000"

if run no-relro.txt; then echo "❌ library without GNU_RELRO was accepted"; exit 1; fi
grep -q 'no GNU_RELRO program header' "$TMP/out" || { echo "❌ missing-RELRO message wrong:"; cat "$TMP/out"; exit 1; }
echo "✅ rejected: GNU_RELRO absent (RELRO must stay enabled)"
