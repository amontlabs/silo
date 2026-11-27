#!/bin/sh
# id: K07
# title: A clone of a running main's disk is a moment in time
# needs: K01, a reflink filesystem under $KIT
# decides: checkpoint-per-system, 4-the-way-back.md (clone)
# question: Main writes numbered 4 KiB blocks with O_DIRECT and a flush after each. Does every clone taken meanwhile hold a prefix of the numbers (every block up to some n, none after)?
# outcome moment: every clone holds a prefix: clone can be taken while main runs
# outcome torn: a clone holds block n+1 without block n: clone needs a pause around it
TEST=K07; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k07; vm_boot k07; vm_wait_ssh k07 300 >/dev/null || fail "no SSH"
# The writer: block i of a 64 MiB file holds "SILO<i>" padded; each write is flushed before the next.
vm_ssh k07 'cat > /tmp/w.py <<"P"
import os
fd = os.open("/home/kit/seq", os.O_RDWR | os.O_CREAT | os.O_DIRECT | os.O_DSYNC, 0o644)
os.ftruncate(fd, 64 << 20); os.fsync(fd)
import mmap
buf = mmap.mmap(-1, 4096)
i = 0
while True:
    buf.seek(0); buf.write(b"SILO%012d" % i + b"\0" * (4096 - 16))
    os.pwrite(fd, buf, (i % 16384) * 4096); i += 1
P
nohup python3 /tmp/w.py >/dev/null 2>&1 &'
sleep 3; torn=0; n=0
for k in 1 2 3 4 5 6 7 8 9 10; do
  cp --reflink=always "$KIT/vms/k07/disk.raw" "$KIT/vms/k07/c$k.raw" || skip "no reflink under $KIT"
  n=$((n+1)); sleep 1
done
vm_kill k07
for k in $(seq 1 $n); do
  # Find the seq file's blocks through the reader, then check the numbers form a prefix.
  r=$(python3 -I "$KIT_SRC/tools/prefix.py" "$KIT/vms/k07/c$k.raw" /home/kit/seq) || r="error"
  case $r in prefix*) ;; *) torn=$((torn+1)) ;; esac
  echo "clone $k: $r" >> "$KIT/vms/k07/report.txt"; rm -f "$KIT/vms/k07/c$k.raw"
done
[ $torn = 0 ] && result K07 moment "$n clones, each a prefix of the completed writes" || result K07 torn "$torn of $n clones are not a prefix; vms/k07/report.txt"
