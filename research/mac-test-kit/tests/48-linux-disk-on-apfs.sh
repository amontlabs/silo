#!/bin/bash
# id: 48
# title: Option B's disk.raw on APFS: a clonefile while main writes, and discards (#381 L5, L8)
# tier: vm
# time: about 15 minutes, after test 47's image
# needs: Homebrew's libkrun and ./kit build, the image test 47 prepares, 10 GB free, no sudo
# step: 4
# decides: linux-vm-engine (#381: a clone of a running main's disk as a checkpoint, under B), space-the-engine-keeps (whether a deleted file's space comes back to the Mac), main-on-macos-first (option B's checkpoints)
# question: Main writes numbered 4 KiB blocks, each flushed before the next. Does every clonefile of disk.raw taken meanwhile hold a prefix of them (every block up to some n and none after), read through #381's ext4 reader with the journal replayed? After main writes 1 GiB, deletes it and runs fstrim, how much of disk.raw's space comes back to the Mac?
# outcome moment-and-trim: Every clone held a prefix, and at least 900 MiB came back after fstrim: under B, a checkpoint is a clonefile taken
#   while main runs, and space follows main's deletions.
# outcome torn: A clone held block n+1 without block n: under B a checkpoint needs a pause around the clone (#381's K07 question on APFS).
# outcome trim-ignored: Every clone held a prefix, but under 900 MiB came back: libkrun on a Mac does not punch holes for discards, so
#   the engine keeps a reserve and #381's step 4 says disk.raw only grows.
# outcome inconclusive: Main did not boot (see test 47), or the reader failed.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/krun.sh"
krun_need
kit_need_free_gb 10
V=k48; D="$KRUN_HOME/vms/$V"
cleanup() { rm -f "$D"/c*.raw; krun_destroy "$V"; }
trap cleanup EXIT
trap 'exit 130' INT TERM HUP
krun_new "$V" || kit_fail "could not make the computer"
krun_boot "$V"
krun_wait_ssh "$V" 300 >/dev/null || { kit_outcome inconclusive "main did not answer SSH (see test 47)"; exit 0; }

krun_ssh "$V" 'cat > /tmp/w.py <<"P"
import os, mmap
fd = os.open("/home/kit/seq", os.O_RDWR | os.O_CREAT | os.O_DIRECT | os.O_DSYNC, 0o644)
os.ftruncate(fd, 64 << 20); os.fsync(fd)
buf = mmap.mmap(-1, 4096); i = 0
while True:
    buf.seek(0); buf.write(b"SILO%012d" % i + b"\0" * (4096 - 16))
    os.pwrite(fd, buf, (i % 16384) * 4096); i += 1
P
nohup python3 /tmp/w.py >/dev/null 2>&1 &'
sleep 3
n=0
for k in 1 2 3 4 5 6 7 8 9 10; do cp -c "$D/disk.raw" "$D/c$k.raw" && n=$((n + 1)); sleep 1; done
krun_ssh "$V" 'pkill -f /tmp/w.py'
torn=0; : > "$KIT_WORK/clones.txt"
for k in $(seq 1 "$n"); do
  r="$(python3 -I "$KIT_ROOT/py/linuxmain/prefix.py" "$D/c$k.raw" /home/kit/seq 2>&1)" || true
  echo "clone $k: $r" >> "$KIT_WORK/clones.txt"
  case "$r" in prefix*) ;; *) torn=$((torn + 1)) ;; esac
  rm -f "$D/c$k.raw"
done
kit_attach "$KIT_WORK/clones.txt"
grep -q "prefix" "$KIT_WORK/clones.txt" || { kit_outcome inconclusive "the reader found no prefix in any clone: $(head -1 "$KIT_WORK/clones.txt")"; exit 0; }

krun_ssh "$V" 'dd if=/dev/urandom of=/home/kit/big bs=1M count=1024 conv=fsync status=none'
a="$(du -k "$D/disk.raw" | cut -f1)"
kit_fact fstrim "$(krun_ssh "$V" 'rm /home/kit/big && sync && sudo fstrim -v / 2>&1')"
sleep 3; b="$(du -k "$D/disk.raw" | cut -f1)"
freed=$(( (a - b) / 1024 ))
kit_measure freed_after_fstrim "$freed" MiB "of 1024 written and deleted in main"
if [ "$torn" -gt 0 ]; then kit_outcome torn "$torn of $n clones are not a prefix of main's flushed writes"
elif [ "$freed" -lt 900 ]; then kit_outcome trim-ignored "every clone ($n) held a prefix; only $freed MiB of 1024 came back after fstrim"
else kit_outcome moment-and-trim "every clone ($n) held a prefix; $freed MiB of 1024 came back after fstrim"; fi
