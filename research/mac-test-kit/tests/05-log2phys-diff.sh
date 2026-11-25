#!/bin/bash
# id: 05
# title: Which blocks two clones differ in, from the Mac's block map
# tier: quick
# time: about 5 minutes
# needs: 12 GB free, no VM, no sudo
# step: 5
# decides: file-changes-from-outside (catching writes past APFS), research 11 "Where the trust comes from", the "how much changed" of a gap
# question: Does F_LOG2PHYS_EXT on two APFS clones of a disk image tell exactly which blocks differ, matching a byte comparison, and how fast?
# outcome map-tells: Every block whose bytes differ has a different device place, and few equal blocks are flagged: the engine can list the blocks written between two checkpoints from the Mac's side, and a write past APFS inside main shows (research 11's design point 4).
# outcome map-overstates: No differing block is missed, but many equal blocks are flagged: usable as a bound (what to read), not as a count of what changed.
# outcome map-misses: Some blocks differ in bytes at the same device place: the map cannot be trusted for this; the reader's trust rests on APFS rules alone and the record says writes past APFS are not seen.
# outcome no-map: F_LOG2PHYS_EXT gives no map of a clone here; the same as map-misses for the design.
. "$KIT_ROOT/lib/common.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
for p in clonebench extents log2phys; do [ -x "$BIN/$p" ] || kit_skip "the kit's programs are not built: run ./kit build"; done
SIZE_GIB="${KIT_05_SIZE_GIB:-64}"    # a sparse disk image, as a computer's
DATA_MIB="${KIT_05_DATA_MIB:-8192}"  # of which this much is written
WRITES="${KIT_05_WRITES:-20000}"     # random 4-64 KiB writes to one clone
LOOP_MNT=""
cleanup() {
  rm -rf "$KIT_WORK/d"
  if [ -n "$LOOP_MNT" ]; then umount "$LOOP_MNT" 2>/dev/null; rm -f "$KIT_WORK/xfs.img"; fi
}
trap cleanup EXIT
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  [ "$(id -u)" = 0 ] && command -v mkfs.xfs >/dev/null 2>&1 || kit_skip "Linux analogue needs root and mkfs.xfs"
  SIZE_GIB="${KIT_05_SIZE_GIB:-4}"; DATA_MIB="${KIT_05_DATA_MIB:-512}"; WRITES="${KIT_05_WRITES:-3000}"
  truncate -s 3G "$KIT_WORK/xfs.img"; mkfs.xfs -q -f -m reflink=1 "$KIT_WORK/xfs.img" || kit_fail mkfs.xfs
  LOOP_MNT="$KIT_WORK/mnt"; mkdir -p "$LOOP_MNT"
  mount -o loop "$KIT_WORK/xfs.img" "$LOOP_MNT" || kit_skip "cannot mount a loop device here"
  D="$LOOP_MNT/d"
  kit_note "Linux analogue: FIEMAP on XFS reflink copies; says nothing of APFS"
else
  kit_need_mac 14
  kit_need_free_gb 12
  D="$KIT_WORK/d"
fi
mkdir -p "$D"
jget() { python3 -c 'import json,sys; print(json.loads(sys.argv[1]).get(sys.argv[2], ""))' "$1" "$2"; }

# A sparse image with DATA_MIB written at its start, then scattered writes over its first half,
# as an installed system's disk.
A="$D/a.img"; B="$D/b.img"
python3 -c "import os; f=open('$A','wb'); f.truncate($SIZE_GIB << 30)"
python3 - "$A" "$DATA_MIB" <<'EOF'
import fcntl, os, sys
fd = os.open(sys.argv[1], os.O_WRONLY); chunk = os.urandom(8 << 20)
for i in range(int(sys.argv[2]) // 8): os.pwrite(fd, chunk, i * (8 << 20))
(fcntl.fcntl(fd, 51) if sys.platform == 'darwin' else os.fsync(fd)); os.close(fd)  # F_FULLFSYNC
EOF
"$BIN/clonebench" frag "$A" 2000 1 4 64 --size $((SIZE_GIB << 29)) >/dev/null || kit_fail frag
kit_measure image_extents "$(jget "$("$BIN/extents" "$A")" extents)" pieces "image of $SIZE_GIB GiB, $DATA_MIB MiB written plus 2,000 scattered writes"

# 1. A clone with nothing written: no block may differ.
cp -c "$A" "$B" 2>/dev/null || cp --reflink=always "$A" "$B" || kit_fail "clone"
r0="$("$BIN/log2phys" diff "$A" "$B" --bytes)" || { kit_fact log2phys_error "$r0"; kit_outcome no-map "log2phys failed on a fresh clone"; exit 0; }
kit_fact fresh_clone "$r0"
kit_measure fresh_clone_differ_blocks "$(jget "$r0" differ_blocks)" blocks "a clone nothing wrote: expected 0"

# 2. WRITES random writes to the clone, plus 100 blocks written again with the bytes they held.
"$BIN/clonebench" frag "$B" "$WRITES" 7 4 64 --size $((SIZE_GIB << 30)) >/dev/null || kit_fail frag
python3 - "$B" <<'EOF'
import fcntl, os, random, sys
fd = os.open(sys.argv[1], os.O_RDWR); r = random.Random(3)
for _ in range(100):
    off = r.randrange(0, 1 << 28, 4096); b = os.pread(fd, 4096, off); os.pwrite(fd, b, off)
(fcntl.fcntl(fd, 51) if sys.platform == 'darwin' else os.fsync(fd)); os.close(fd)  # F_FULLFSYNC
EOF
r1="$("$BIN/log2phys" diff "$A" "$B" --bytes)" || kit_fail "log2phys diff"
kit_fact written_clone "$r1"
# (A function, not a case inside $(...): /bin/bash 3.2 ends the $( at the first pattern's ")".)
unit_of() { case "$1" in *seconds) echo s ;; pieces*) echo pieces ;; *ranges) echo ranges ;; *) echo blocks ;; esac; }
for k in map_seconds compare_seconds pieces_a pieces_b differ_ranges differ_blocks bytes_differ missed extra; do
  kit_measure "written_$k" "$(jget "$r1" "$k")" "$(unit_of "$k")"
done

# 3. The other direction: a write to the original after the clone.
"$BIN/clonebench" frag "$A" 500 9 4 4 --size $((SIZE_GIB << 30)) >/dev/null || kit_fail frag
r2="$("$BIN/log2phys" diff "$A" "$B" --bytes)" || kit_fail "log2phys diff"
kit_measure both_written_missed "$(jget "$r2" missed)" blocks "after 500 more writes to the original"

res="$(python3 - "$r0" "$r1" "$r2" "$WRITES" <<'EOF'
import json, sys
r0, r1, r2 = (json.loads(x) for x in sys.argv[1:4])
missed = r1["missed"] + r2["missed"]
s = ("after %s random writes to one clone: %d blocks differ in bytes, the map flags %d (missed %d, "
     "extra %d, of which 100 were rewritten with their own bytes); map read in %.2f s against %.1f s "
     "for the byte comparison; a fresh clone flags %d"
     % (sys.argv[4], r1["bytes_differ"], r1["differ_blocks"], missed, r1["extra"],
        r1["map_seconds"], r1["compare_seconds"], r0["differ_blocks"]))
if r1["pieces_b"] == 0: print("no-map|" + s)
elif missed > 0: print("map-misses|" + s)
elif r1["extra"] > max(0.05 * r1["bytes_differ"], 200) or r0["differ_blocks"] > 0: print("map-overstates|" + s)
else: print("map-tells|" + s)
EOF
)"
kit_outcome "${res%%|*}" "${res#*|}"
