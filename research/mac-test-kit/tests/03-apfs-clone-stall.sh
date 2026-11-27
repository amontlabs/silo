#!/bin/bash
# id: 03
# title: APFS clone time against fragmentation, with a writer
# tier: quick
# time: about 25 minutes (stops early past its time limit)
# needs: 30 GB free, no VM, no sudo (the ASIF part asks for sudo only if its device is not yours)
# step: 4
# decides: checkpoint-per-system, speed-budgets (the checkpoint's target), research 07 "What to do if APFS behaves like XFS"
# question: Does cloning a disk image stall a program writing to it, and does the stall grow with the pieces checkpoints leave the image in, raw and ASIF?
# outcome no-stall: At 1000 checkpoints a write waited at most 0.1 s during a clone: checkpoints need none of research 07's mitigations; the checkpoint's target can be written as the clone time.
# outcome clone-slow-writes-free: The clone itself grows past 1 s with the pieces but writes do not wait for it (0.1 s or less): a checkpoint delays the model's reply (checkpoint-at-each-turn) but not main; the target names the clone time.
# outcome stall-small: The longest write wait during a clone stays between 0.1 and 1 s at 1000 checkpoints: write the checkpoint's target as that wait at that fragmentation, and count pieces at each checkpoint (research 07 step 1, "measure and show").
# outcome stall-grows: A write waits more than 1 s during a clone at 1000 checkpoints, growing with the pieces as on XFS (11 to 23 s at about 500,000 pieces): checkpoint-per-system needs research 07's mitigations, making the disk whole when main stops first, a layer per boot next.
# outcome inconclusive: The runs did not get far enough or disagree; look at 03-steps.csv and the stall lines in the log.
. "$KIT_ROOT/lib/common.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
for p in clonebench extents; do [ -x "$BIN/$p" ] || kit_skip "the kit's programs are not built: run ./kit build"; done

MIB="${KIT_03_MIB:-8192}"              # the image: 8 GiB, as research 07
WRITES="${KIT_03_WRITES:-300}"         # random 4-64 KiB writes between two checkpoints
MILESTONES="${KIT_03_MILESTONES:-1 10 100 1000}"
KEEP="${KIT_03_KEEP:-10}"              # clones kept, older ones deleted (thinning)
BUDGET="${KIT_03_BUDGET_S:-1200}"      # seconds for the raw part's checkpoint loop
ASIF_BUDGET="${KIT_03_ASIF_BUDGET_S:-420}"
ASIF_MILESTONES="${KIT_03_ASIF_MILESTONES:-1 10 100}"

LOOP_MNT=""
cleanup() {
  [ -n "${ASIF_DEV:-}" ] && { hdiutil detach "$ASIF_DEV" >/dev/null 2>&1 || diskutil eject "$ASIF_DEV" >/dev/null 2>&1; }
  rm -rf "${D:-$KIT_WORK/03-none}"
  if [ -n "$LOOP_MNT" ]; then umount "$LOOP_MNT" 2>/dev/null; rm -f "$KIT_WORK/xfs.img"; fi
}
trap cleanup EXIT

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  # Linux analogue: the same loop on XFS with reflinks, smaller, to check the logic.
  [ "$(id -u)" = 0 ] && command -v mkfs.xfs >/dev/null 2>&1 || kit_skip "Linux analogue needs root and mkfs.xfs"
  MIB="${KIT_03_MIB:-512}"; WRITES="${KIT_03_WRITES:-100}"; MILESTONES="${KIT_03_MILESTONES:-1 10 40}"
  truncate -s 3G "$KIT_WORK/xfs.img"
  mkfs.xfs -q -f -m reflink=1 "$KIT_WORK/xfs.img" || kit_fail "mkfs.xfs"
  LOOP_MNT="$KIT_WORK/mnt"; mkdir -p "$LOOP_MNT"
  mount -o loop "$KIT_WORK/xfs.img" "$LOOP_MNT" || kit_skip "cannot mount a loop device here"
  D="$LOOP_MNT/d"
  kit_note "Linux analogue on XFS (reflink): numbers say nothing of APFS"
else
  kit_need_mac 14
  kit_need_free_gb 30
  D="$KIT_WORK/d"
  kit_confirm "Test 03 writes an $((MIB / 1024)) GiB image and clones it up to $(echo "$MILESTONES" | awk '{print $NF}') times, about 25 minutes."
fi
mkdir -p "$D"

jget() { python3 -c 'import json,sys; print(json.loads(sys.argv[1])[sys.argv[2]])' "$1" "$2"; }
STEPS="$KIT_WORK/03-steps.csv"
echo "format,checkpoint,extents,clone_s" > "$STEPS"

# loop FORMAT IMAGE WRITE_TARGET SIZE MILESTONES BUDGET: the checkpoint loop. WRITE_TARGET is where
# the writes go (the image itself, or the ASIF image's device).
run_loop() {
  local fmt="$1" img="$2" wt="$3" size="$4" ms="$5" budget="$6"
  local last k=0 t0 out ck ext st sc del oldest m
  last="$(echo "$ms" | awk '{print $NF}')"
  t0="$(kit_now)"
  # before any checkpoint: the image in one piece
  ext="$(jget "$("$BIN/extents" "$img")" extents)"
  kit_measure "${fmt}_ck0_extents" "$ext" pieces
  st="$("$BIN/clonebench" stall --write "$wt" --size "$size" --clone "$img" "$D/probe")" || kit_fail "stall at 0"
  rm -f "$D/probe"
  kit_measure "${fmt}_ck0_longest_write_s" "$(jget "$st" longest_during_s)" s "uncached writer, during one clone, no checkpoint yet"
  kit_measure "${fmt}_base_write_p50_s" "$(jget "$st" base_p50_s)" s "the writer's usual 4 KiB write"
  while [ "$k" -lt "$last" ]; do
    k=$((k + 1))
    "$BIN/clonebench" frag "$wt" "$WRITES" "$k" 4 64 --size "$size" >/dev/null || kit_fail "frag failed at checkpoint $k"
    ck="$D/ck-$k"
    out="$("$BIN/clonebench" clone "$img" "$ck")" || kit_fail "clone failed at checkpoint $k"
    sc="$(jget "$out" seconds)"
    oldest=$((k - KEEP))
    m=0; for x in $ms; do [ "$x" = "$k" ] && m=1; done
    if [ "$m" = 1 ]; then
      ext="$(jget "$("$BIN/extents" "$img")" extents)"
      echo "$fmt,$k,$ext,$sc" >> "$STEPS"
      kit_measure "${fmt}_ck${k}_extents" "$ext" pieces "after $k checkpoints of $WRITES random 4-64 KiB writes each"
      kit_measure "${fmt}_ck${k}_clone_s" "$sc" s "clonefile(2), nothing writing"
      st="$("$BIN/clonebench" stall --write "$wt" --size "$size" --clone "$img" "$D/probe")" || kit_fail "stall at $k"
      rm -f "$D/probe"
      kit_measure "${fmt}_ck${k}_clone_under_writer_s" "$(jget "$st" action_s)" s
      kit_measure "${fmt}_ck${k}_longest_write_s" "$(jget "$st" longest_during_s)" s "uncached 4 KiB writes, one after another, during the clone"
      kit_measure "${fmt}_ck${k}_writes_during_clone" "$(jget "$st" writes_during)" writes
      if [ "$fmt" = raw ]; then
        st="$("$BIN/clonebench" stall --write "$wt" --size "$size" --cached --clone "$img" "$D/probe")" || kit_fail "cached stall at $k"
        rm -f "$D/probe"
        kit_measure "${fmt}_ck${k}_longest_cached_write_s" "$(jget "$st" longest_during_s)" s "cached writes (the Mac's cache, as disk-cache cached), during the clone"
      fi
      if [ "$oldest" -ge 1 ] && [ -f "$D/ck-$oldest" ]; then
        del="$("$BIN/clonebench" stall --write "$wt" --size "$size" --delete "$D/ck-$oldest")" || kit_fail "delete stall at $k"
        kit_measure "${fmt}_ck${k}_delete_s" "$(jget "$del" action_s)" s "unlink of the oldest kept clone"
        kit_measure "${fmt}_ck${k}_longest_write_during_delete_s" "$(jget "$del" longest_any_s)" s "longest write in the second after the delete"
      fi
    else
      echo "$fmt,$k,,$sc" >> "$STEPS"
    fi
    [ "$oldest" -ge 1 ] && rm -f "$D/ck-$oldest"
    if python3 -c "import sys; sys.exit(0 if $(kit_now) - $t0 > $budget else 1)"; then
      kit_note "$fmt: stopped at checkpoint $k of $last, past the time limit of $budget s"
      ext="$(jget "$("$BIN/extents" "$img")" extents)"
      st="$("$BIN/clonebench" stall --write "$wt" --size "$size" --clone "$img" "$D/probe")" || kit_fail "stall at $k"
      rm -f "$D/probe"
      echo "$fmt,$k,$ext,$(jget "$st" action_s)" >> "$STEPS"
      kit_measure "${fmt}_ck${k}_extents" "$ext" pieces "the last checkpoint reached"
      kit_measure "${fmt}_ck${k}_longest_write_s" "$(jget "$st" longest_during_s)" s "the last checkpoint reached"
      break
    fi
  done
  kit_measure "${fmt}_checkpoints_reached" "$k" checkpoints
  rm -f "$D"/ck-*
}

# Raw: a file, written in full as after an install.
"$BIN/clonebench" fill "$D/raw.img" "$MIB" >/dev/null || kit_fail "fill"
run_loop raw "$D/raw.img" "$D/raw.img" $((MIB * 1048576)) "$MILESTONES" "$BUDGET"
t="$(kit_now)"; cp -c "$D/raw.img" "$D/cp-c.img" 2>/dev/null || cp --reflink=always "$D/raw.img" "$D/cp-c.img"
kit_measure raw_cp_c_s "$(python3 -c "print('%.3f' % ($(kit_now) - $t))")" s "cp -c of the most fragmented image, with process start"
rm -f "$D/cp-c.img" "$D/raw.img"

# ASIF: written through its attached device, as the Virtualization framework writes it.
if kit_is_mac && [ "${KIT_FAKE:-0}" != 1 ]; then
  major="$(sw_vers -productVersion | cut -d. -f1)"
  if [ "$major" -lt 26 ]; then
    kit_fact asif "not tried: needs macOS 26"
  elif ! diskutil image create blank --fs none --format ASIF --size "${MIB}MiB" "$D/a.asif" >"$KIT_WORK/asif.log" 2>&1; then
    kit_attach "$KIT_WORK/asif.log"; kit_fact asif "diskutil image create failed (see asif.log)"
  else
    out="$( (diskutil image attach --noMount "$D/a.asif" || hdiutil attach -nomount "$D/a.asif") 2>&1)"
    ASIF_DEV="$(echo "$out" | grep -o '/dev/disk[0-9]*' | head -1)"
    if [ -z "$ASIF_DEV" ]; then
      echo "$out" > "$KIT_WORK/asif.log"; kit_attach "$KIT_WORK/asif.log"; kit_fact asif "attach failed (see asif.log)"
    else
      rdev="/dev/r${ASIF_DEV#/dev/}"
      kit_fact asif_device "$rdev owner $(ls -l "$rdev" | awk '{print $3":"$4}')"
      if [ ! -w "$rdev" ] && [ "${KIT_SUDO:-0}" = 1 ]; then
        kit_sudo "let this test write to the ASIF image it attached; the device disappears when the test detaches it" chown "$(id -un)" "$rdev"
      fi
      if [ -w "$rdev" ]; then
        # A failure here must not lose the raw part's answer: it becomes a fact, not kit_fail.
        if "$BIN/clonebench" fill "$rdev" "$MIB" > "$KIT_WORK/asif-fill.log" 2>&1; then
          kit_measure asif_file_bytes "$(stat -f %z "$D/a.asif")" bytes "after a full write"
          run_loop asif "$D/a.asif" "$rdev" $((MIB * 1048576)) "$ASIF_MILESTONES" "$ASIF_BUDGET"
        else
          kit_attach "$KIT_WORK/asif-fill.log"
          kit_fact asif "writing $MIB MiB to $rdev failed (see asif-fill.log; is the device smaller than the image size asked?)"
        fi
      else
        kit_fact asif "skipped: $rdev is not writable by you; rerun with --allow-sudo to measure ASIF"
      fi
    fi
  fi
fi
kit_attach "$STEPS"

# The answer, from the most fragmented point reached of the raw image.
res="$(python3 - "$KIT_OUT" "$KIT_ID" <<'EOF'
import json, re, sys
m = {}
for line in open(sys.argv[1]):
    e = json.loads(line)
    if e.get("test") == sys.argv[2] and e.get("kind") == "measure": m[e["name"]] = e["value"]
ks = sorted(int(x.group(1)) for x in (re.match(r"raw_ck(\d+)_longest_write_s$", n) for n in m) if x)
if not ks or ks[-1] == 0: print("inconclusive|no checkpoint was measured"); sys.exit()
k = ks[-1]; w = m["raw_ck%d_longest_write_s" % k]; ext = m.get("raw_ck%d_extents" % k, 0)
c = m.get("raw_ck%d_clone_s" % k, m.get("raw_ck%d_clone_under_writer_s" % k, 0))
pts = [(m["raw_ck%d_extents" % i], m["raw_ck%d_clone_s" % i]) for i in ks if "raw_ck%d_clone_s" % i in m and "raw_ck%d_extents" % i in m]
slope = ""
if len(pts) >= 2 and pts[-1][0] > pts[0][0]:
    us = (pts[-1][1] - pts[0][1]) / (pts[-1][0] - pts[0][0]) * 1e6
    slope = ", %.2f us per piece (XFS: 21)" % us
a = ""
if "asif_checkpoints_reached" in m:
    ka = int(m["asif_checkpoints_reached"]); aw = m.get("asif_ck%d_longest_write_s" % ka)
    if aw is not None: a = "; ASIF after %d checkpoints: %s pieces, longest write %.3f s" % (ka, m.get("asif_ck%d_extents" % ka), aw)
s = "after %d checkpoints the raw image is in %d pieces; a clone takes %.3f s and the longest write during it %.3f s%s%s" % (k, ext, c, w, slope, a)
if k < 100: key = "inconclusive"
elif w <= 0.1 and c > 1.0: key = "clone-slow-writes-free"
elif w <= 0.1: key = "no-stall"
elif w <= 1.0: key = "stall-small"
else: key = "stall-grows"
if key in ("no-stall", "clone-slow-writes-free", "stall-small") and k < 1000: s += " (the 1000 checkpoints were not reached)"
print(key + "|" + s)
EOF
)"
kit_outcome "${res%%|*}" "${res#*|}"
