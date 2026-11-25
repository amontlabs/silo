#!/bin/bash
# id: 38
# title: The Mac's volume fills under a running computer
# tier: vm
# time: about 30 minutes
# needs: the installed computer of test 00, macOS 27 on the Mac, 3 GB free (a 2 GB volume of its own, in a disk image the kit makes and deletes), no sudo on the Mac
# step: 4
# decides: space-the-engine-keeps (whether a full volume stops the computer, damages it, or only fails its writes until room is made), low-disk and checkpoint-storage (what a full volume does to a running computer, which Silo's levels are set against)
# question: When the volume holding a running computer's disk image fills, what does the computer see (write errors, writes that wait, or nothing), does Apple's framework stop it, and once room is made, do its writes work again on a volume that still verifies?
# outcome io-error-recovers: The computer's writes failed while the volume was full, the computer kept running, its
#   writes worked again within a minute of room being made, and its volume verified: a reserve file freed when
#   the volume fills is enough; Silo stops checkpoints at the floor and says why.
# outcome writes-wait: The computer's writes hung while the volume was full (no error, no progress), and went on by
#   themselves once room was made: the volume verified. The engine still frees its reserve at once, since a
#   computer whose writes hang looks dead to its agents.
# outcome framework-stops: Apple's framework stopped the computer when the volume filled (the run ended, or the
#   computer stopped answering): the engine must keep room before the volume fills (reserve-file), never after.
# outcome guest-damaged: The computer kept running, but its volume did not verify after room was made: a full
#   volume can damage main; the engine must never let it fill (reserve-file and the floor).
# outcome inconclusive: The volume did not fill within 10 minutes, or the computer did not start with the extra
#   disk; the sentence says which.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/net.sh"

if guest_local; then kit_skip "a full volume under Apple's Virtualization framework has no Linux analogue: linux/test-38.sh checks the decision"; fi
kit_need_mac 27
kit_need_silovm; kit_need_base_vm
kit_need_cmd hdiutil "it ships with macOS"
kit_need_free_gb 3
kit_confirm "This makes a 2 GB disk image with a volume of its own, starts one macOS computer with a second disk stored on that volume, and fills it until the volume is full; then frees room and checks the computer. Your own volume is not filled. About 30 minutes."

W="$KIT_WORK"; SUM="$W/summary.env"; : > "$SUM"
put() { echo "$1=$2" >> "$SUM"; }
V="$W/small"; DMG="$W/small.dmg"
cleanup() {
  guest_cleanup
  hdiutil detach "$V" -force >/dev/null 2>&1
  rm -f "$DMG"
}
trap cleanup EXIT

hdiutil create -size 2g -fs APFS -volname SiloKit38 -type UDIF "$DMG" >/dev/null || kit_fail "hdiutil create failed"
mkdir -p "$V"
hdiutil attach "$DMG" -nobrowse -mountpoint "$V" >/dev/null || kit_fail "hdiutil attach failed"
free_kb() { df -k "$V" | awk 'NR==2 {print $4}'; }
# The extra disk: an 8 GB image, sparse, on the 2 GB volume, so the computer can fill the volume.
python3 -c 'import os,sys; f=open(sys.argv[1],"wb"); f.truncate(8 << 30); f.close()' "$V/extra.img"
# A ballast of 200 MB: the reserve file the engine would free.
dd if=/dev/zero of="$V/ballast" bs=1048576 count=200 2>/dev/null
kit_fact volume_free_mb_before "$(( $(free_kb) / 1024 ))"

guest_up full --extra-disk "$V/extra.img"
kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"

# The extra disk inside: the physical disk of about 8.6 GB, erased as APFS (root inside the computer).
DISK="$(guest_sh 'for d in $(diskutil list physical | awk "/^\/dev\/disk/ {print \$1}"); do s=$(diskutil info "$d" | awk -F"[()]" "/Disk Size/ {print \$2}" | awk "{print \$1}"); [ "$s" = 8589934592 ] && echo "$d"; done' | head -1)"
[ -n "$DISK" ] || { kit_outcome inconclusive "the computer shows no 8 GB disk: the extra disk did not attach"; exit 0; }
kit_fact guest_disk "$DISK"
guest_sudo "diskutil eraseDisk APFS SiloKit38x GPT $DISK" > "$W/erase.txt" 2>&1 || { kit_attach "$W/erase.txt"; kit_outcome inconclusive "the extra disk could not be erased inside"; exit 0; }
MNT=/Volumes/SiloKit38x
guest_sudo "chown $VM_USER $MNT" || true

# Inside: a writer that fills the disk, and a probe that writes and syncs 4 KB every second and logs
# "time ok|fail error" (to the computer's own disk, so the log survives the extra one filling).
PROBE='use strict; use IO::Handle; use Time::HiRes qw(time sleep);
my $log = "$ENV{HOME}/silokit38-probe.txt"; open(my $L, ">>", $log) or die; $L->autoflush(1);
while (1) { my $t = time; my $ok = open(my $f, ">", "'"$MNT"'/probe") && print($f "x" x 4096) && $f->flush && $f->sync && close($f);
  printf $L "%.3f %s %s\n", $t, $ok ? "ok" : "fail", $ok ? "" : ($! + 0); sleep(1) }'
printf '%s\n' "$PROBE" > "$W/probe.pl"
guest_put "$W/probe.pl" silokit38-probe.pl
guest_sh '(nohup perl "$HOME/silokit38-probe.pl" > /dev/null 2>&1 &); (nohup dd if=/dev/urandom of='"$MNT"'/fill bs=1048576 > "$HOME/silokit38-dd.txt" 2>&1 &); echo started' >/dev/null
T_START="$(kit_now)"

# --- 1. Wait for the volume to fill (under 8 MB free), up to 10 minutes.
full=no; i=0
while [ "$i" -lt 600 ]; do
  [ "$(free_kb)" -lt 8192 ] && { full=yes; break; }
  vm_running "$GB" || break
  sleep 1; i=$((i + 1))
done
T_FULL="$(kit_now)"
put filled "$full"
kit_fact seconds_to_full "$(python3 -c "print('%.0f' % ($T_FULL - $T_START))")"
if [ "$full" != yes ] && vm_running "$GB"; then kit_outcome inconclusive "the volume did not fill within 10 minutes ($(( $(free_kb) / 1024 )) MB still free)"; exit 0; fi

# --- 2. A minute on a full volume.
sleep 60
r=yes; vm_running "$GB" || r=no
put running_full "$r"
s=no; net_limit 30 guest_sh true 2>/dev/null && s=yes
put ssh_full "$s"
cp "$GB/run.jsonl" "$W/run-events.jsonl"; kit_attach "$W/run-events.jsonl"
kit_fact events_since_fill "$(python3 -c '
import json, sys
t = float(sys.argv[2]); ev = []
for line in open(sys.argv[1]):
    try: e = json.loads(line)
    except ValueError: continue
    if e.get("t", 0) >= t: ev.append(json.dumps(e)[:160])
print(" | ".join(ev) or "none")' "$GB/run.jsonl" "$T_START")"
tail -20 "$GB/run.log" > "$W/silovm-stderr.txt" 2>/dev/null; kit_attach "$W/silovm-stderr.txt"

# --- 3. Room is made: the ballast goes, as the engine frees its reserve.
rm -f "$V/ballast"
T_FREE="$(kit_now)"
sleep 60
r=yes; vm_running "$GB" || r=no
put running_end "$r"
s=no
if [ "$r" = yes ]; then
  i=0; while [ "$i" -lt 12 ]; do net_limit 30 guest_sh true 2>/dev/null && { s=yes; break; }; sleep 5; i=$((i + 1)); done
fi
put ssh_end "$s"
if [ "$s" = yes ]; then
  guest_sh 'pkill -f silokit38-probe.pl; pkill -x dd; sleep 1; cat "$HOME/silokit38-dd.txt"' > "$W/dd.txt" 2>&1; kit_attach "$W/dd.txt"
  guest_get silokit38-probe.txt "$W/probe.txt"; kit_attach "$W/probe.txt"
  python3 - "$W/probe.txt" "$T_FULL" "$T_FREE" >> "$SUM" <<'PY'
import sys
rows = []
for line in open(sys.argv[1]):
    p = line.split()
    if len(p) >= 2:
        rows.append((float(p[0]), p[1], p[2] if len(p) > 2 else ""))
t_full, t_free = float(sys.argv[2]), float(sys.argv[3])
# The computer's clock may differ a little from the Mac's: the windows are wide.
during = [r for r in rows if t_full - 30 <= r[0] < t_free]
after = [r for r in rows if r[0] >= t_free + 5]
gaps = [b[0] - a[0] for a, b in zip(rows, rows[1:]) if a[0] >= t_full - 30 and a[0] < t_free]
print("probe_fail_full=%d" % sum(1 for r in during if r[1] == "fail"))
print("probe_errors=%s" % ",".join(sorted(set(r[2] for r in during if r[1] == "fail"))))
print("probe_stall_max=%.0f" % max(gaps, default=0))
print("probe_ok_after=%d" % sum(1 for r in after if r[1] == "ok"))
print("probe_after=%d" % len(after))
PY
  guest_sudo "diskutil unmount force $MNT >/dev/null 2>&1; diskutil verifyVolume $DISK 2>&1" > "$W/verify-extra.txt" 2>&1
  kit_attach "$W/verify-extra.txt"
  guest_sudo "diskutil verifyVolume / 2>&1 | tail -3" > "$W/verify-root.txt" 2>&1
  kit_attach "$W/verify-root.txt"
  if grep -qi "appears to be OK" "$W/verify-extra.txt"; then put verify ok; else put verify damaged; fi
fi
kit_attach "$SUM"
res="$(python3 -I "$KIT_ROOT/py/38-decide.py" "$SUM")"
kit_outcome "${res%%|*}" "${res#*|}"
