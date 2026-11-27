#!/bin/bash
# Tests 47 to 49's helpers on Linux (the tests themselves need a Mac and skip here):
# krun-boot.c compiles against a stub of libkrun.h, py/linuxmain/kernel.py turns a zboot, a gzip
# and a raw arm64 Image into the raw Image libkrun wants and refuses anything else, and krun_seed
# makes a cidata seed (with genisoimage or xorriso, when installed) whose user-data is cloud-config.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
tmp="$(mktemp -d /tmp/kitkrun.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*"; exit 1; }

mkdir -p "$tmp/inc"; cp "$KIT_ROOT/linux/fixtures/libkrun-stub.h" "$tmp/inc/libkrun.h"
cc -fsyntax-only -Wall -Werror -I "$tmp/inc" "$KIT_ROOT/krun/krun-boot.c" 2> "$tmp/cc.log" ||
  { cat "$tmp/cc.log"; fail "krun-boot.c does not compile against linux/fixtures/libkrun-stub.h"; }
echo "ok   krun-boot.c compiles against the libkrun stub"

python3 - "$tmp" <<'P'
import gzip, os, struct, sys
d = sys.argv[1]
img = bytearray(4096); img[56:60] = b"ARM\x64"; img[100:110] = b"silo-image"
open(os.path.join(d, "raw"), "wb").write(img)
open(os.path.join(d, "gz"), "wb").write(gzip.compress(bytes(img)))
pay = gzip.compress(bytes(img))
hdr = bytearray(64); hdr[0:2] = b"MZ"; hdr[4:8] = b"zimg"
struct.pack_into("<II", hdr, 8, 64, len(pay)); hdr[24:28] = b"gzip"
open(os.path.join(d, "zboot"), "wb").write(bytes(hdr) + pay)
open(os.path.join(d, "bad"), "wb").write(gzip.compress(b"\0" * 4096))
P
for form in raw gz zboot; do
  out="$(python3 -I "$KIT_ROOT/py/linuxmain/kernel.py" "$tmp/$form" "$tmp/$form.Image")" || fail "kernel.py refused the $form fixture"
  cmp -s "$tmp/$form.Image" "$tmp/raw" || fail "kernel.py's Image from the $form fixture is not the raw one"
  echo "ok   kernel.py: $form fixture read as $out"
done
python3 -I "$KIT_ROOT/py/linuxmain/kernel.py" "$tmp/bad" "$tmp/bad.Image" 2>/dev/null && fail "kernel.py took a payload with no arm64 magic"
echo "ok   kernel.py refuses a payload with no arm64 magic"

if command -v genisoimage >/dev/null 2>&1 || command -v xorriso >/dev/null 2>&1; then
  export KIT_ROOT SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="$tmp/root" KIT_WORK="$tmp/w" KIT_OUT="$tmp/r.jsonl" KIT_ID=47
  mkdir -p "$KIT_WORK" "$tmp/seed"
  bash -c '. "$KIT_ROOT/lib/common.sh"; . "$KIT_ROOT/lib/vm.sh"; . "$KIT_ROOT/lib/krun.sh"; krun_seed "$1" k47' _ "$tmp/seed" > "$tmp/seed.log" 2>&1 ||
    { cat "$tmp/seed.log"; fail "krun_seed failed"; }
  [ -s "$tmp/seed/seed.iso" ] || fail "krun_seed made no seed.iso"
  grep -aq "cidata" "$tmp/seed/seed.iso" || fail "the seed's volume is not named cidata"
  grep -aq "ListenStream=vsock::22" "$tmp/seed/seed.iso" || fail "the seed's user-data has no sshd on vsock port 22"
  echo "ok   krun_seed: a cidata seed with sshd on vsock port 22"
else
  echo "ok   krun_seed not checked: neither genisoimage nor xorriso is installed"
fi
