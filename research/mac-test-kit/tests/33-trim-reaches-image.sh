#!/bin/bash
# id: 33
# title: Does the guest's TRIM free the blocks of the disk image on the Mac
# tier: vm
# time: about 30 minutes (two starts of a computer, 9 GB written in it)
# needs: the base computer of test 00, 40 GB free, no sudo on the Mac; ASIF needs macOS 26
# decides: checkpoint-per-system and thinning-checkpoints (whether a macOS computer's deleted files keep growing its checkpoints), checkpoint-storage, research 2026-11-24-checkpoint-engine ("TRIM")
# question: When a macOS guest writes then deletes a few GB, does the disk image on the Mac shrink back (the guest's TRIM reaching the image as a hole), at once, after a sync and a minute, only when the volume is mounted again, or never; for the main raw image, a raw extra disk and an ASIF extra disk?
# outcome trim-reaches: A raw image gave back at least 75% of what the write took within a minute of the delete and a sync:
#   what a macOS computer deletes stops costing disk; a checkpoint keeps only what was there when it was taken, and the space model of the engine's research holds as written.
# outcome trim-at-mount: A raw image gave back at least 75% only after the volume was mounted again or the computer restarted:
#   deleted files cost disk until the next start; Silo can count on the space coming back at each start (and thinning sees it then).
# outcome asif-only: Only the ASIF image gave space back: main's disk should be ASIF where space matters; a raw image keeps what was deleted.
# outcome partial: Between 25% and 75% came back: the guest trims some of what it frees; the sentence says how much per image and when.
# outcome no-trim: Less than 25% came back on every image, even after a restart: every block a macOS computer ever wrote stays in its image,
#   so each daily checkpoint keeps what the guest's file system moved or deleted; the engine's space numbers are a floor, and Silo needs
#   another way to free it (the NBD route of test 32, where Silo sees each TRIM, or zeroing free space inside the guest).
# outcome inconclusive: A write did not grow its image or a step failed; look at 33-alloc.csv and the logs.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"

GB="${KIT_33_GB:-3}"           # written then deleted on each disk
SETTLE="${KIT_33_SETTLE_S:-60}"
CSV="$KIT_WORK/33-alloc.csv"
echo "target,stage,alloc_bytes,size_bytes" > "$CSV"

# alloc STAGE TARGET:PATH...: the space each image takes on the Mac's disk (allocated blocks,
# as du -k and stat -f %b count them), not its size.
alloc() {
  local stage="$1" tp
  shift
  for tp in "$@"; do
    python3 - "${tp%%:*}" "${tp#*:}" "$stage" >> "$CSV" <<'EOF'
import os, sys
st = os.stat(sys.argv[2])
print("%s,%s,%d,%d" % (sys.argv[1], sys.argv[3], st.st_blocks * 512, st.st_size))
EOF
  done
  kit_log "allocated at $stage: $(grep ",$stage," "$CSV" | awk -F, '{printf "%s %.2f GB  ", $1, $3/1e9}')"
}

measures() { # one measure per image and stage, from the CSV
  python3 - "$CSV" <<'EOF' > "$KIT_WORK/m.txt"
import csv, sys
for r in csv.DictReader(open(sys.argv[1])):
    print("%s_%s_alloc_gb %.3f" % (r["target"], r["stage"], int(r["alloc_bytes"]) / 1e9))
EOF
  while read -r n v; do kit_measure "$n" "$v" GB "allocated on the host's disk"; done < "$KIT_WORK/m.txt"
}

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  # Linux analogue: ext4 on a loop device over a sparse image file. Deleting a file frees nothing
  # in the image until fstrim, which stands here for the trim a guest's file system does at mount.
  [ "$(id -u)" = 0 ] && command -v mkfs.ext4 >/dev/null 2>&1 && command -v fstrim >/dev/null 2>&1 \
    || kit_skip "Linux analogue needs root, mkfs.ext4 and fstrim"
  GB_MB="${KIT_33_MB:-256}"
  I="$KIT_WORK/raw.img"; M="$KIT_WORK/mnt"
  rm -f "$I"; truncate -s 2G "$I"; mkfs.ext4 -q -F -E nodiscard "$I" || kit_fail mkfs.ext4
  mkdir -p "$M"; mount -o loop "$I" "$M" || kit_skip "cannot mount a loop device here"
  trap 'umount "$M" 2>/dev/null; rm -f "$I"' EXIT
  kit_note "Linux analogue: ext4 in a loop-mounted image; fstrim stands for a trim at mount"
  sync; alloc before "raw:$I"
  head -c $((GB_MB * 1048576)) /dev/urandom > "$M/big"; sync; alloc written "raw:$I"
  rm "$M/big"; sleep 1; alloc deleted "raw:$I"
  sync; sleep 1; alloc settled "raw:$I"
  fstrim "$M" || kit_note "fstrim failed"
  sync; alloc remount "raw:$I"
  kit_attach "$CSV"
  measures
  res="$(python3 "$KIT_ROOT/py/33-decide.py" "$CSV" $((GB_MB * 1048576)))"
  kit_outcome "${res%%|*}" "Linux analogue: ${res#*|}"
  exit 0
fi

kit_need_mac 14
kit_need_silovm
kit_need_base_vm
kit_need_free_gb 40
kit_confirm "Test 33 starts a macOS computer twice and writes then deletes ${GB} GB on each of its disks: about 30 minutes."

B="$(vm_fresh t33)" || exit 1
XR="$KIT_WORK/extra-raw.img"; XA="$KIT_WORK/extra.asif"
trap 'vm_destroy "$B"; rm -f "$XR" "$XA"' EXIT
kit_fact main_disk_format "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("disk_format", "raw"))' "$B/config.json")"

# The extra disks, told apart in the guest by size: raw 12 GiB (sparse), ASIF 10 GiB.
RAW_BYTES=$((12 * 1073741824)); ASIF_BYTES=$((10 * 1073741824))
python3 -c 'import sys; f = open(sys.argv[1], "wb"); f.truncate(int(sys.argv[2]))' "$XR" "$RAW_BYTES"
EXTRA="--extra-disk $XR"
TARGETS="main:$B/disk.img raw:$XR"
if [ "$(sw_vers -productVersion | cut -d. -f1)" -ge 26 ] && \
   diskutil image create blank --fs none --format ASIF --size 10G "$XA" > "$KIT_WORK/asif.log" 2>&1; then
  [ -f "$XA" ] || { [ -f "$XA.asif" ] && mv "$XA.asif" "$XA"; }
  EXTRA="$EXTRA --extra-disk $XA"
  TARGETS="$TARGETS asif:$XA"
else
  kit_attach "$KIT_WORK/asif.log" 2>/dev/null
  kit_fact asif "not tried: needs macOS 26 and diskutil image create (see asif.log)"
fi

# shellcheck disable=SC2046,SC2086  # the options are words on purpose
vm_start "$B" $(vm_provision_args) $EXTRA >/dev/null
s="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_fail "the computer did not answer SSH after its first start"; }
kit_measure first_start_to_ssh_s "$s" s
vm_ssh "$B" "$VM_USER" 'mkdir -p ~/sk && cat > ~/sk/findsize.sh' < "$KIT_ROOT/guest/findsize.sh" || kit_fail "copy of findsize.sh"

# The extra disks become APFS volumes, as a macOS computer's own disk is.
GPATHS="main:/Users/$VM_USER/sk/big"
for x in raw asif; do
  case "$x" in raw) bytes=$RAW_BYTES vol=SKRAW ;; asif) bytes=$ASIF_BYTES vol=SKASIF ;; esac
  case "$TARGETS" in *"$x:"*) ;; *) continue ;; esac
  dev="$(vm_ssh "$B" "$VM_USER" "sh ~/sk/findsize.sh $bytes")"
  if [ -z "$dev" ] || ! vm_sudo "$B" "$VM_USER" "diskutil eraseDisk APFS $vol GPT $dev" > "$KIT_WORK/erase-$x.log" 2>&1; then
    kit_attach "$KIT_WORK/erase-$x.log" 2>/dev/null
    kit_note "$x: the extra disk was not found or not erased in the guest (device '${dev}'); left out"
    TARGETS="$(printf '%s\n' $TARGETS | grep -v "^$x:" | tr '\n' ' ')"
    continue
  fi
  kit_fact "${x}_guest_disk" "$dev, $(vm_ssh "$B" "$VM_USER" "diskutil info $dev | grep -iE 'solid state|trim|unmap|protocol' | tr -s ' ' | tr '\n' ';'")"
  GPATHS="$GPATHS $x:/Volumes/$vol/big"
done
kit_fact main_guest_disk "$(vm_ssh "$B" "$VM_USER" "diskutil info / | grep -iE 'solid state|trim|unmap|protocol|device node' | tr -s ' ' | tr '\n' ';'")"

gpath() { printf '%s\n' $GPATHS | grep "^$1:" | cut -d: -f2-; }
each_guest() { # each_guest CMD-WITH-@: run CMD as root in the guest for each disk, @ its file
  local t p
  for t in $(printf '%s\n' $GPATHS | cut -d: -f1); do
    p="$(gpath "$t")"
    vm_sudo "$B" "$VM_USER" "$(printf '%s' "$1" | sed "s#@#$p#g")" >> "$KIT_WORK/guest.log" 2>&1 || kit_note "$t: '$1' failed in the guest"
  done
}

vm_sudo "$B" "$VM_USER" sync
sleep 5
# shellcheck disable=SC2086
alloc before $TARGETS
each_guest "dd if=/dev/urandom of=@ bs=1048576 count=$((GB * 1024)) 2>/dev/null; chown $VM_USER @"
vm_sudo "$B" "$VM_USER" sync; sleep 5
# shellcheck disable=SC2086
alloc written $TARGETS
each_guest "rm -f @"
sleep 5
# shellcheck disable=SC2086
alloc deleted $TARGETS
vm_sudo "$B" "$VM_USER" sync
sleep "$SETTLE"
# shellcheck disable=SC2086
alloc settled $TARGETS
for x in raw asif; do
  case "$GPATHS" in *"$x:"*) ;; *) continue ;; esac
  vol="$(gpath "$x" | cut -d/ -f3)"
  vm_sudo "$B" "$VM_USER" "diskutil unmount /Volumes/$vol && diskutil mount $vol" >> "$KIT_WORK/guest.log" 2>&1 || kit_note "$x: unmount and mount failed"
done
sleep 20
# shellcheck disable=SC2086
alloc remount $TARGETS
vm_sudo "$B" "$VM_USER" "log show --last 10m --predicate 'sender == \"apfs\"' 2>/dev/null | grep -i trim | tail -40" > "$KIT_WORK/guest-trim-log.txt" 2>&1
kit_attach "$KIT_WORK/guest-trim-log.txt"

how="$(vm_stop "$B" 120)"
kit_fact first_stop "$how"
# shellcheck disable=SC2086
vm_start "$B" $EXTRA >/dev/null
if vm_wait_ssh "$B" "$VM_USER" 900 >/dev/null; then
  sleep "$SETTLE"
  # shellcheck disable=SC2086
  alloc restart $TARGETS
  vm_sudo "$B" "$VM_USER" "log show --last 5m --predicate 'sender == \"apfs\"' 2>/dev/null | grep -i trim | tail -40" > "$KIT_WORK/guest-trim-log-restart.txt" 2>&1
  kit_attach "$KIT_WORK/guest-trim-log-restart.txt"
else
  kit_note "the computer did not answer SSH after its restart"
fi
vm_stop "$B" 60 >/dev/null
kit_attach "$CSV"
[ -s "$KIT_WORK/guest.log" ] && kit_attach "$KIT_WORK/guest.log"

measures
res="$(python3 "$KIT_ROOT/py/33-decide.py" "$CSV" $((GB * 1073741824)))"
kit_outcome "${res%%|*}" "${res#*|}"
