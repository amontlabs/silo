#!/bin/bash
# id: 31
# title: The checkpoint engine's prototype on APFS: full, cbw, nbd, delta, and the catalog under kill
# tier: long
# time: about 50 minutes (four runs of an 8 GiB image, then the catalog killed at 300 steps)
# needs: 40 GB free, no VM, no sudo; Zig 0.15.2 (downloaded from ziglang.org, about 51 MB, if not on PATH) and the prototype's source (fetched from this repository's branch if not next to the kit); both asked first
# step: 4
# decides: checkpoint-per-system (which way a checkpoint is taken on a Mac: a clone, cbw, or delta), checkpoint-at-each-turn, thinning-checkpoints (the wait of a drop), research 2026-11-24-checkpoint-engine
# question: On APFS, does cloning a file that shares blocks with the live disk image (the last checkpoint, which nobody writes) make the live image's writes wait, as it did on XFS (up to 1.0 s), and how do the prototype's ways compare there?
# outcome full-free: Even a clone of the live image kept every write under 0.1 s: the plan's checkpoint (a clone) holds on APFS at this fragmentation;
#   cbw or delta are needed only for the list of changed blocks, not for the wait. Check the pieces in the sentence against test 03 before relying on it.
# outcome shared-clone-free: A clone of the live image made writes wait, but cloning a checkpoint that shares blocks with it did not (0.1 s or less):
#   cbw works on a Mac as on the prototype, through Silo's NBD server for a macOS computer (test 32); one copy of each block per checkpoint, no deltas to merge.
# outcome shared-clone-waits: Cloning a checkpoint that shares blocks with the live image made writes wait 0.1 to 1 s, as cbw did on XFS:
#   cbw costs that wait at each checkpoint; prefer delta (named in the sentence with its own waits) wherever the wait is felt, as with checkpoint-at-each-turn.
# outcome shared-clone-stalls: Cloning a checkpoint that shares blocks with the live image made writes wait more than 1 s, as on XFS at 1.0 s:
#   on a Mac, nothing that shares blocks with the live image may be cloned while it runs; take checkpoints as delta.
# outcome not-a-moment: A checkpoint on APFS did not hold exactly the writes up to one moment: the prototype's way is wrong on APFS; nothing is built on it until the cause is found (the run's files are attached).
# outcome catalog-broke: The catalog, killed at each of 300 steps on APFS with F_FULLFSYNC and RENAME_SWAP, did not recover, or a planted bug was not caught: the catalog's design needs another look before a Mac relies on it.
# outcome inconclusive: The prototype did not build or a way did not run; look at the attached logs.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/ckpt.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
WAYS="${KIT_31_WAYS:-full cbw nbd delta}"
# mac.sh's arguments: an 8 GiB image in pieces, a stand-in computer writing 1000 times a second,
# 8 checkpoints 5 s apart, 3 dropped, and a clone of an older checkpoint (unrelated=1).
ARGS="${KIT_31_ARGS:-size_gib=8 frag=300000 rate=1000 interval_ms=5000 count=8 drop=3 unrelated=1}"
ROUNDS="${KIT_31_ROUNDS:-300}"
R="$KIT_WORK/runs"
LOOP_MNT=""
cleanup() {
  rm -rf "$KIT_WORK/d"
  if [ -n "$LOOP_MNT" ]; then umount "$LOOP_MNT" 2>/dev/null; rm -f "$KIT_WORK/xfs.img"; fi
}
trap cleanup EXIT

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  # Linux analogue: the same runs, small, on XFS with reflinks, to check the test's own logic.
  [ "$(id -u)" = 0 ] && command -v mkfs.xfs >/dev/null 2>&1 || kit_skip "Linux analogue needs root and mkfs.xfs"
  ARGS="${KIT_31_ARGS:-size_gib=1 frag=20000 rate=500 interval_ms=500 count=4 drop=1 unrelated=1}"
  ROUNDS="${KIT_31_ROUNDS:-30}"
  truncate -s 6G "$KIT_WORK/xfs.img"
  mkfs.xfs -q -f -m reflink=1 "$KIT_WORK/xfs.img" || kit_fail "mkfs.xfs"
  LOOP_MNT="$KIT_WORK/mnt"; mkdir -p "$LOOP_MNT"
  mount -o loop "$KIT_WORK/xfs.img" "$LOOP_MNT" || kit_skip "cannot mount a loop device here"
  D="$LOOP_MNT/d"
  kit_note "Linux analogue on XFS (reflink), small: numbers say nothing of APFS"
else
  kit_need_mac 14
  kit_need_free_gb 40
  D="$KIT_WORK/d"
  kit_confirm "Test 31 builds the checkpoint engine's prototype and runs it on four 8 GiB images on this Mac's disk, then kills its catalog at 300 steps: about 50 minutes."
fi
ckpt_build || kit_skip "the prototype could not be built here (see the log)"
kit_fact ckpt_args "$ARGS"
rm -rf "$R"; mkdir -p "$R" "$D"
: > "$R/pieces.txt"

for w in $WAYS; do
  d="$D/run-$w"
  kit_log "way $w (in $d)"
  # shellcheck disable=SC2086  # the arguments are words on purpose
  "$CKPT_BIN" bench strategy="$w" dir="$d" $ARGS > "$R/$w.jsonl" 2> "$R/$w.err"
  rc=$?
  [ "$rc" = 0 ] || kit_note "ckpt bench strategy=$w exited $rc (see $w.err)"
  kit_attach "$R/$w.jsonl"
  [ -s "$R/$w.err" ] && kit_attach "$R/$w.err"
  if [ -x "$BIN/extents" ] && [ -f "$d/live.img" ]; then
    p="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["extents"])' "$("$BIN/extents" "$d/live.img")" 2>/dev/null)"
    [ -n "$p" ] && echo "$w $p" >> "$R/pieces.txt"
  fi
  rm -rf "$d"
done

d="$D/crash"
"$CKPT_BIN" crash root="$d" rounds="$ROUNDS" systematic=1 > "$R/crash.jsonl" 2> "$R/crash.err"
kit_fact crash_exit "$?"
kit_attach "$R/crash.jsonl"
rm -rf "$d"
: > "$R/mutants.txt"
for m in two-renames no-tmp; do
  d="$D/mutant-$m"
  if CKPT_MUTANT="$m" "$CKPT_BIN" crash root="$d" rounds="$ROUNDS" systematic=1 > "$R/mutant-$m.txt" 2>&1; then
    echo "$m NOT caught" >> "$R/mutants.txt"
  else
    echo "$m caught" >> "$R/mutants.txt"
  fi
  rm -rf "$d"
done

python3 "$KIT_ROOT/py/31-ckpt.py" "$R" > "$R/record.tsv" || kit_fail "py/31-ckpt.py failed on the runs"
res=""
while IFS="$(printf '\t')" read -r kind a b c e; do
  case "$kind" in
    M) kit_measure "$a" "$b" "$c" "$e" ;;
    F) kit_fact "$a" "$b" ;;
    O) res="$a|$b" ;;
  esac
done < "$R/record.tsv"
[ -n "$res" ] || kit_fail "no outcome from py/31-ckpt.py"
kit_outcome "${res%%|*}" "${res#*|}"
