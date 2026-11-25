#!/bin/bash
# id: 46
# title: What deleting a checkpoint frees while a local Time Machine snapshot exists
# tier: quick
# time: about 5 minutes
# needs: 3 GB free on the volume of ~/SiloKit, the Command Line Tools (./kit build), no VM; it makes one local Time Machine snapshot of that volume (after asking), which macOS removes within 24 hours, and with --allow-sudo the kit removes it at the end
# step: 4
# decides: low-disk (#378: the level where Silo pauses computers must not count on space freed by removing checkpoints in the same minute), space-the-engine-keeps (a reserve the engine holds, since deleting may free nothing), checkpoint-storage (how Silo counts what a checkpoint holds)
# question: On the volume Silo keeps checkpoints on, a clone shares blocks with the file it came from and a write to it takes new ones. Without a local snapshot, does deleting the clone and its file give their space back at once? Once a local Time Machine snapshot exists, how much does the same deletion free, what private size and clone id does APFS report for the files, and how much comes back when the snapshot goes?
# outcome snapshot-holds-space: With a local snapshot, deleting the files freed under a quarter of what they took, as #378 says, and the
#   space came back when the snapshot went (or the kit could not remove it; the facts say). low-disk: Silo never counts on space freed by
#   deleting checkpoints while a local snapshot exists, and shows "frees once macOS removes its local snapshots"; space-the-engine-keeps:
#   the reserve file is the defence that holds.
# outcome delete-frees: Deleting freed at least three quarters of what the files took even with a local snapshot: #378's trap does not
#   hold on this macOS; the facts give the private sizes APFS reported.
# outcome no-snapshot: macOS made no local snapshot (Time Machine refused it without sudo, or is turned off by a profile); only the control
#   ran. Rerun with --allow-sudo.
# outcome inconclusive: The files could not be made, or free space moved too much for a reading (another program wrote meanwhile).
. "$KIT_ROOT/lib/common.sh"

W="$KIT_WORK"
BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
[ -x "$BIN/cloneattrs" ] || kit_skip "cloneattrs is not built: run ./kit build"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; else kit_need_mac; fi
TM="${KIT_TMUTIL:-tmutil}"
if [ "$LINUX" = 1 ]; then CLONE="cp --reflink=auto"; else CLONE="cp -c"; fi
MB="${KIT_46_MB:-1024}"; MOD=$((MB / 4))
kit_need_free_gb 3
DATA="$W/data"; mkdir -p "$DATA"
SNAP=""
cleanup() {
  rm -rf "$DATA"
  if [ -n "$SNAP" ]; then
    if "$TM" deletelocalsnapshots "$SNAP" >/dev/null 2>&1; then :
    elif [ "${KIT_SUDO:-0}" = 1 ]; then sudo "$TM" deletelocalsnapshots "$SNAP" >/dev/null 2>&1
    else kit_log "the local snapshot $SNAP stays until macOS removes it (within 24 hours), or: sudo tmutil deletelocalsnapshots $SNAP"; fi
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

attrs() { "$BIN/cloneattrs" "$@"; }
free_b() { attrs "$W" | python3 -c 'import json,sys; print(json.loads(sys.stdin.read())["volume_free"])'; }
settle() { sync; sleep "${KIT_46_SETTLE:-5}"; }
# make NAME: a file of MB MiB, a clone of it, then MOD MiB rewritten in the clone.
make() {
  dd if=/dev/urandom of="$DATA/$1" bs=1048576 count="$MB" 2>/dev/null || return 1
  $CLONE "$DATA/$1" "$DATA/$1.clone" || return 1
  dd if=/dev/urandom of="$DATA/$1.clone" bs=1048576 count="$MOD" conv=notrunc 2>/dev/null || return 1
  settle
}
mib() { python3 -c "print('%.0f' % (($1) / 1048576))"; }

kit_fact local_snapshots_before "$("$TM" listlocalsnapshots / 2>/dev/null | grep -c com.apple || true)"

# --- the control: no snapshot made by the test -------------------------------------------------
f0="$(free_b)"
make ctl || { kit_outcome inconclusive "could not write the control files"; exit 0; }
f1="$(free_b)"; took=$((f0 - f1))
attrs "$DATA/ctl" "$DATA/ctl.clone" > "$W/ctl.attrs.jsonl"; kit_attach "$W/ctl.attrs.jsonl"
rm -f "$DATA/ctl" "$DATA/ctl.clone"; settle
f2="$(free_b)"; freed=$((f2 - f1))
kit_measure control_took "$(mib $took)" MiB "a $MB MiB file and its clone with $MOD MiB rewritten"
kit_measure control_freed "$(mib $freed)" MiB "deleting both, no snapshot made by the test"
python3 -c "import sys; sys.exit(0 if $took > $MB * 1048576 * 0.8 else 1)" ||
  { kit_outcome inconclusive "the files took $(mib $took) MiB, not about $((MB + MOD)): another program is writing to this volume"; exit 0; }

# --- with a local snapshot -----------------------------------------------------------------------
make snap || { kit_outcome inconclusive "could not write the files"; exit 0; }
kit_confirm "Make one local Time Machine snapshot of this Mac's volumes (tmutil localsnapshot). It holds what is deleted after it until macOS removes it (within 24 hours); with --allow-sudo the kit removes it at the end."
out="$("$TM" localsnapshot 2>&1)" || {
  if [ "${KIT_SUDO:-0}" = 1 ]; then out="$(kit_sudo "make a local Time Machine snapshot" "$TM" localsnapshot 2>&1)"; else out=""; fi; }
SNAP="$(echo "$out" | sed -n 's/.*\([0-9]\{4\}-[0-9][0-9]-[0-9][0-9]-[0-9]\{6\}\).*/\1/p' | head -1)"
if [ -z "$SNAP" ]; then
  kit_fact tmutil_localsnapshot "$(echo "$out" | head -2 | tr '\n' ' ')"
  kit_outcome no-snapshot "no local snapshot was made ($(echo "$out" | head -1)); the control freed $(mib $freed) of $(mib $took) MiB at once"
  exit 0
fi
kit_fact snapshot "$SNAP"
settle
attrs "$DATA/snap" "$DATA/snap.clone" > "$W/snap.attrs.jsonl"; kit_attach "$W/snap.attrs.jsonl"
kit_fact private_sizes_with_snapshot "$(python3 -c '
import json, sys
print(", ".join("%s private %s clone id %s" % (d["path"].rsplit("/", 1)[-1], d.get("private_size", "n/a"), d.get("clone_id", "n/a")) for d in map(json.loads, open(sys.argv[1]))))' "$W/snap.attrs.jsonl")"
g1="$(free_b)"
rm -f "$DATA/snap" "$DATA/snap.clone"; settle
g2="$(free_b)"; freed_snap=$((g2 - g1))
kit_measure snapshot_freed "$(mib $freed_snap)" MiB "deleting the same files while a local snapshot holds them"
back="not removed"
if "$TM" deletelocalsnapshots "$SNAP" >/dev/null 2>&1 || { [ "${KIT_SUDO:-0}" = 1 ] && kit_sudo "remove the test's local snapshot" "$TM" deletelocalsnapshots "$SNAP" >/dev/null 2>&1; }; then
  SNAP=""; settle
  g3="$(free_b)"; back="$(mib $((g3 - g2))) MiB"
  kit_measure freed_when_snapshot_went "$(mib $((g3 - g2)))" MiB ""
fi
kit_fact snapshot_removed_by_kit "$([ -z "$SNAP" ] && echo yes || echo "no: sudo tmutil deletelocalsnapshots $SNAP, or wait for macOS")"

r="$(python3 -c "print('%.2f' % ($freed_snap / max(1, $took)))")"
where="$([ "$LINUX" = 1 ] && echo "Linux analogue (a fake tmutil whose snapshot is hard links): " || echo "")"
if python3 -c "import sys; sys.exit(0 if $r < 0.25 else 1)"; then
  kit_outcome snapshot-holds-space "${where}with a local snapshot, deleting freed $(mib $freed_snap) of $(mib $took) MiB; without, $(mib $freed); when the snapshot went: $back"
elif python3 -c "import sys; sys.exit(0 if $r >= 0.75 else 1)"; then
  kit_outcome delete-frees "${where}deleting freed $(mib $freed_snap) of $(mib $took) MiB even with a local snapshot"
else
  kit_outcome inconclusive "${where}deleting with a snapshot freed $(mib $freed_snap) of $(mib $took) MiB: between a quarter and three quarters"
fi
