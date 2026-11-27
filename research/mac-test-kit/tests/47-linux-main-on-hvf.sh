#!/bin/bash
# id: 47
# title: Option B's Linux main through libkrun on this Mac: boot, stop, memory given back, disk speed (#381 L1 to L3)
# tier: vm
# time: about 20 minutes, plus about 1 GB downloaded once (Ubuntu 26.04's arm64 cloud image, kernel and initrd)
# needs: Homebrew's libkrun (brew tap slp/krun; brew install libkrun) and ./kit build after it, 8 GB free, no sudo, no macOS 27 (libkrun's floor is macOS 14, to confirm)
# step: 2
# decides: main-on-macos-first (option B, a Linux main first, #381), linux-vm-engine (#381's measurements 1, 4 and 5 on a Mac), mac-silo-needs (which macOS option B needs)
# question: On this Mac and its macOS, does libkrun on Apple's Hypervisor framework boot Ubuntu 26.04 to SSH over vsock with no network device, how long do the first and second boots take, is systemd "running"? Does libkrun's stop request (its shutdown eventfd, with the seed's HandleRebootKey=poweroff) power main off? After main touches and frees 4 GiB, how much does the Mac get back? How fast does main write to its disk, against the Mac's own?
# outcome boots-and-stops: Main booted to SSH over vsock, systemd running, and libkrun's stop request powered it off: option B's first build
#   runs on this macOS; the measures give its boot, the memory it gives back and its disk against the Mac's.
# outcome stop-ignored: Main booted, but the stop request did not power it off within 60 s: the helper needs another way to stop main
#   (a poweroff over SSH, then a kill), which #381's step 2 must say.
# outcome no-boot: Main never answered SSH over vsock within 5 minutes: option B does not run on this Mac as specified; console.log says why.
# outcome inconclusive: The image could not be prepared, or a measurement failed.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/krun.sh"
krun_need
kit_fact macos "$(sw_vers -productVersion) ($(sw_vers -buildVersion))"
kit_fact libkrun "$(brew list --versions libkrun 2>/dev/null) $(brew list --versions libkrunfw 2>/dev/null)"
V=k47
cleanup() { krun_destroy "$V"; }
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

krun_new "$V" || kit_fail "could not make the computer"
krun_boot "$V"
first="$(krun_wait_ssh "$V" 300)" || { kit_attach "$KRUN_HOME/vms/$V/console.log"; kit_attach "$KRUN_HOME/vms/$V/krun.log"; kit_outcome no-boot "no SSH over vsock within 300 s of the first boot"; exit 0; }
kit_measure first_boot_to_ssh "$first" s "Ubuntu 26.04, cloud-init's first boot"
krun_ssh "$V" 'cloud-init status --wait' >/dev/null 2>&1
kit_fact systemd "$(krun_ssh "$V" 'systemctl is-system-running' 2>/dev/null)"
kit_fact guest_kernel "$(krun_ssh "$V" 'uname -r')"
kit_fact network_devices "$(krun_ssh "$V" 'ls /sys/class/net | grep -v ^lo | tr "\n" " "')"

# L2: memory given back to the Mac.
p="$(krun_pid "$V")"
rss() { ps -o rss= -p "$p" | tr -d ' '; }
r0="$(rss)"
krun_ssh "$V" 'python3 -c "b = bytearray(4 << 30); [b.__setitem__(i, 1) for i in range(0, len(b), 4096)]; import time; time.sleep(5)"'
r1="$(rss)"
krun_ssh "$V" 'sync; echo 3 | sudo tee /proc/sys/vm/drop_caches >/dev/null'
sleep 60; r2="$(rss)"
kit_measure rss_idle "$((r0 / 1024))" MiB "krun-boot's resident memory on the Mac, idle"
kit_measure rss_after_4g "$((r1 / 1024))" MiB "while main holds 4 GiB it touched"
kit_measure rss_60s_after_free "$((r2 / 1024))" MiB "60 s after main freed it"
kit_measure memory_given_back "$(( (r1 - r2) / 1024 ))" MiB "of about 4096"

# L3: disk speed, main against the Mac.
g="$(krun_ssh "$V" 'dd if=/dev/zero of=/home/kit/seq bs=1M count=1024 oflag=direct conv=fsync 2>&1 | tail -1; rm -f /home/kit/seq')"
kit_fact main_seq_write "$g"
s="$(krun_ssh "$V" 'python3 -c "
import os, time
fd = os.open(\"/home/kit/rnd\", os.O_RDWR | os.O_CREAT | os.O_DSYNC, 0o644); os.ftruncate(fd, 256 << 20)
t = time.time(); n = 2000
for i in range(n): os.pwrite(fd, b\"x\" * 4096, (i * 7919 % 65536) * 4096)
print(\"%.0f\" % (n / (time.time() - t)))"; rm -f /home/kit/rnd')"
kit_measure main_4k_sync_writes "${s:-0}" writes/s "O_DSYNC, in main"
m="$(python3 -c "
import fcntl, os, time
p = os.path.join('$KIT_WORK', 'rnd'); fd = os.open(p, os.O_RDWR | os.O_CREAT, 0o644); os.ftruncate(fd, 256 << 20)
t = time.time(); n = 2000
for i in range(n):
    os.pwrite(fd, b'x' * 4096, (i * 7919 % 65536) * 4096); fcntl.fcntl(fd, 51)  # F_FULLFSYNC
print('%.0f' % (n / (time.time() - t))); os.unlink(p)")"
kit_measure mac_4k_fullfsync_writes "${m:-0}" writes/s "the same on the Mac, F_FULLFSYNC after each"

# L1: the stop request, then a second boot.
st="$(krun_term "$V" 60)"
kit_fact stop_request "$st"
if [ "$st" = none ]; then
  krun_kill "$V"
  kit_attach "$KRUN_HOME/vms/$V/krun.log"
  kit_outcome stop-ignored "booted to SSH in $first s, but libkrun's stop request did not power main off within 60 s ($(grep -m1 'shutdown eventfd' "$KRUN_HOME/vms/$V/krun.log"))"
  exit 0
fi
krun_boot "$V" --no-seed
second="$(krun_wait_ssh "$V" 120)" && kit_measure second_boot_to_ssh "$second" s ""
kit_outcome boots-and-stops "Ubuntu booted to SSH over vsock in $first s (second boot ${second:-?} s), systemd $(krun_ssh "$V" 'systemctl is-system-running' 2>/dev/null), the stop request powered it off in $st s; memory given back $(( (r1 - r2) / 1024 )) MiB of about 4096"
