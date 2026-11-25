#!/bin/bash
# id: 06
# title: Cost of a forced write, and overwrites after a clone
# tier: quick
# time: about 6 minutes
# needs: 3 GB free, the Command Line Tools, no VM, no sudo; run on mains power with other apps closed
# step: 1
# decides: engine-saved-state, events-until-the-recorder, low-disk, record-speed-cost
# question: What do fsync, F_BARRIERFSYNC and F_FULLFSYNC cost, alone, grouped and while another program writes, and does an overwrite take new space once a clone shares the file?
# outcome full-fast-reserve-needed: F_FULLFSYNC's p99 while another program writes is 50 ms or less, and an overwrite after a clone takes new space: the engine flushes each write group with F_FULLFSYNC (engine-saved-state as proposed in research 02), and low-disk keeps a reserve, since preallocating does not protect once a checkpoint or local snapshot shares the file.
# outcome full-slow-reserve-needed: F_FULLFSYNC's p99 under a writer passes 50 ms, and an overwrite after a clone takes new space: the engine groups its writes (one flush per group, ticks never inputs), core-never-waits on it, and low-disk keeps a reserve.
# outcome full-fast-prealloc-protects: F_FULLFSYNC is 50 ms or less under a writer, and an overwrite after a clone took no new space (not expected on APFS): preallocating the engine's files is enough, no reserve.
# outcome full-slow-prealloc-protects: F_FULLFSYNC passes 50 ms under a writer, and an overwrite after a clone took no new space: group the engine's writes; preallocating is enough.
# outcome inconclusive: A measurement failed or the space readings moved too much; look at 06-fsync.csv and the space lines.
. "$KIT_ROOT/lib/common.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
[ -x "$BIN/fsyncbench" ] && [ -x "$BIN/extents" ] || kit_skip "the kit's programs are not built: run ./kit build"
ROUNDS="${KIT_06_ROUNDS:-3}"
OVER_MIB="${KIT_06_OVERWRITE_MIB:-512}"
SCALE=1
LOOP_MNT=""; BG=""
cleanup() {
  [ -n "$BG" ] && { kill "$BG" 2>/dev/null; wait "$BG" 2>/dev/null; }
  rm -rf "$KIT_WORK/data"
  if [ -n "$LOOP_MNT" ]; then umount "$LOOP_MNT" 2>/dev/null; rm -f "$KIT_WORK/xfs.img"; fi
}
trap cleanup EXIT
DIR="$KIT_WORK/data"; mkdir -p "$DIR"
SPACE_DIR="$DIR"

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  kit_note "Linux analogue: barrier and full fall back to fdatasync; numbers say nothing of a Mac"
  SCALE=10; ROUNDS="${KIT_06_ROUNDS:-1}"; OVER_MIB="${KIT_06_OVERWRITE_MIB:-64}"
  if [ "$(id -u)" = 0 ] && command -v mkfs.xfs >/dev/null 2>&1; then
    truncate -s 1G "$KIT_WORK/xfs.img" && mkfs.xfs -q -f -m reflink=1 "$KIT_WORK/xfs.img" \
      && LOOP_MNT="$KIT_WORK/mnt" && mkdir -p "$LOOP_MNT" && mount -o loop "$KIT_WORK/xfs.img" "$LOOP_MNT" \
      && SPACE_DIR="$LOOP_MNT" || LOOP_MNT=""
  fi
else
  kit_need_mac 14
  kit_need_free_gb 3
  kit_fact power "$(pmset -g batt | head -1 | sed "s/.*'\(.*\)'.*/\1/")"
  kit_fact lowpowermode "$(pmset -g | awk '/lowpowermode/{print $2}')"
  kit_fact filevault "$(fdesetup status | head -1)"
  kit_fact local_snapshots "$(tmutil listlocalsnapshots / 2>/dev/null | grep -c com.apple || true)"
fi

CSV="$KIT_WORK/06-fsync.csv"
echo "mode,record_bytes,records_per_flush,preallocated,flushes,p50_us,p99_us,max_us,records_per_s,flush_call,round,load" > "$CSV"
run() { # run MODE RECORD PER_FLUSH FLUSHES PRE
  local n=$(($4 / SCALE)); [ "$n" -lt 20 ] && n=20
  local line; line="$("$BIN/fsyncbench" "$DIR" "$1" "$2" "$3" "$n" "$5")" || kit_fail "fsyncbench $*"
  echo "$line,$ROUND,$LOAD" >> "$CSV"
}

LOAD=quiet
ROUND=1
while [ "$ROUND" -le "$ROUNDS" ]; do
  kit_log "round $ROUND of $ROUNDS, quiet disk"
  for pre in 1 0; do for rs in 128 4096; do
    run none "$rs" 1 2000 "$pre"; run fsync "$rs" 1 1000 "$pre"; run barrier "$rs" 1 1000 "$pre"; run full "$rs" 1 300 "$pre"
  done; done
  run full 128 8 300 1; run full 128 64 300 1; run barrier 128 64 1000 1
  ROUND=$((ROUND + 1))
done

kit_log "under load: another program keeps writing 1 GB files to the same disk, as a computer's disk does"
LOAD=writer; ROUND=1
( while :; do dd if=/dev/zero of="$DIR/load.img" bs=1048576 count=$((1024 / SCALE)) 2>/dev/null; done ) &
BG=$!
sleep 1
run full 128 1 300 1; run barrier 128 1 1000 1; run fsync 128 1 1000 1; run full 128 64 100 1; run barrier 128 64 300 1
kill "$BG" 2>/dev/null; wait "$BG" 2>/dev/null; BG=""
rm -f "$DIR/load.img"
kit_attach "$CSV"

# Medians over rounds, as measures.
python3 - "$CSV" <<'EOF' > "$KIT_WORK/06-med.txt"
import csv, sys, statistics
rows = list(csv.DictReader(open(sys.argv[1])))
g = {}
for r in rows:
    k = (r["load"], r["mode"], r["record_bytes"], r["records_per_flush"], r["preallocated"])
    g.setdefault(k, []).append(r)
for k, rs in g.items():
    load, mode, rb, per, pre = k
    name = "%s_%s_%sB_x%s_%s" % (load, mode, rb, per, "prealloc" if pre == "1" else "append")
    for f in ("p50_us", "p99_us", "max_us"):
        print(name + "_" + f.replace("_us", ""), statistics.median(float(r[f]) for r in rs), "us", rs[0]["flush_call"])
    print(name + "_records_per_s", statistics.median(float(r["records_per_s"]) for r in rs), "records/s", rs[0]["flush_call"])
EOF
while read -r n v u c; do kit_measure "$n" "$v" "$u" "$c"; done < "$KIT_WORK/06-med.txt"

# Does an overwrite take new space: unshared, then after a clone? And is the unshared overwrite in
# place (same device blocks)?
avail_kb() { # free space of the volume, once it has settled
  python3 - "$1" <<'EOF'
import os, sys, time
last = None
for _ in range(10):
    os.sync(); time.sleep(1.5)
    s = os.statvfs(sys.argv[1]); a = s.f_bavail * s.f_frsize // 1024
    if last is not None and abs(a - last) < 1024: break
    last = a
print(a)
EOF
}
f="$SPACE_DIR/over.img"
dd if=/dev/urandom of="$f" bs=1048576 count="$OVER_MIB" 2>/dev/null || kit_fail "dd"
"$BIN/extents" --map "$f" > "$KIT_WORK/map0.txt"
a0="$(avail_kb "$SPACE_DIR")"
dd if=/dev/urandom of="$f" bs=1048576 count="$OVER_MIB" conv=notrunc 2>/dev/null
a1="$(avail_kb "$SPACE_DIR")"
"$BIN/extents" --map "$f" > "$KIT_WORK/map1.txt"
cp -c "$f" "$f.clone" 2>/dev/null || cp --reflink=always "$f" "$f.clone" || kit_fail "clone"
a2="$(avail_kb "$SPACE_DIR")"
dd if=/dev/urandom of="$f" bs=1048576 count="$OVER_MIB" conv=notrunc 2>/dev/null
a3="$(avail_kb "$SPACE_DIR")"
rm -f "$f" "$f.clone"
inplace="$(python3 - "$KIT_WORK/map0.txt" "$KIT_WORK/map1.txt" <<'EOF'
import sys
def load(p):
    m = []
    for line in open(p):
        lo, dev, n = map(int, line.split()); m.append((lo, dev, n))
    return m
a, b = load(sys.argv[1]), load(sys.argv[2])
def same(a, b):  # bytes whose device offset did not change
    s = 0; j = 0
    for lo, dev, n in a:
        while j < len(b) and b[j][0] + b[j][2] <= lo: j += 1
        k = j
        while k < len(b) and b[k][0] < lo + n:
            blo, bdev, bn = b[k]
            s0, s1 = max(lo, blo), min(lo + n, blo + bn)
            if s1 > s0 and dev - lo == bdev - blo: s += s1 - s0
            k += 1
    return s
tot = sum(n for _, _, n in a) or 1
print("%.3f" % (same(a, b) / tot))
EOF
)"
mb() { python3 -c "print(round(($1 - $2) / 1024.0, 1))"; }
kit_measure overwrite_unshared_new_space_mb "$(mb "$a0" "$a1")" MB "file of $OVER_MIB MB, overwritten in full with no clone"
kit_measure clone_space_mb "$(mb "$a1" "$a2")" MB
kit_measure overwrite_after_clone_new_space_mb "$(mb "$a2" "$a3")" MB "file of $OVER_MIB MB, overwritten in full while a clone shares it"
kit_measure overwrite_unshared_in_place_fraction "$inplace" fraction "share of the file's bytes at the same device offset after an overwrite with no clone"
kit_attach "$KIT_WORK/06-med.txt"

res="$(python3 - "$KIT_WORK/06-med.txt" "$OVER_MIB" "$(mb "$a2" "$a3")" "$inplace" <<'EOF'
import sys
m = {}
for line in open(sys.argv[1]):
    p = line.split(); m[p[0]] = float(p[1])
mib, after, inplace = float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
p99 = m.get("writer_full_128B_x1_prealloc_p99"); q50 = m.get("quiet_full_128B_x1_prealloc_p50")
g = m.get("quiet_full_128B_x64_prealloc_records_per_s")
if p99 is None: print("inconclusive|no F_FULLFSYNC run under a writer"); sys.exit()
fast = p99 <= 50000
if after > 0.5 * mib: space = "reserve-needed"
elif after < 0.1 * mib: space = "prealloc-protects"
else: print("inconclusive|an overwrite after a clone took %.0f MB of %.0f: neither none nor all" % (after, mib)); sys.exit()
s = ("F_FULLFSYNC of 128 B: p50 %.1f ms quiet, p99 %.1f ms while another program writes; grouped by 64, %.0f records/s; "
     "an overwrite of %.0f MB after a clone took %.0f MB of new space; with no clone, %.0f%% of an overwritten file stayed in place"
     % ((q50 or 0) / 1000, p99 / 1000, g or 0, mib, after, inplace * 100))
print("full-%s-%s|%s" % ("fast" if fast else "slow", space, s))
EOF
)"
kit_outcome "${res%%|*}" "${res#*|}"
