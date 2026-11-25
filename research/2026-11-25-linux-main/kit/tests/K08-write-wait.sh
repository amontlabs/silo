#!/bin/sh
# id: K08
# title: How long main's writes wait during a checkpoint, clone against delta
# needs: K07; for delta, libkrun built with the write log patch (KIT_LIBKRUN_WRITE_LOG=1)
# decides: checkpoint-per-system, speed-budgets, 4-the-way-back.md (when B moves to delta)
# question: On a disk image fragmented by #365's script, the longest single 4 KiB write in main during a checkpoint, for clone and for delta, against the target of 100 ms at the 99th percentile.
TEST=K08; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k08; vm_boot k08; vm_wait_ssh k08 300 >/dev/null || fail "no SSH"
# Fragment the image as #365 did: random 4 KiB writes after a clone, from inside.
cp --reflink=always "$KIT/vms/k08/disk.raw" "$KIT/vms/k08/base.raw" || skip "no reflink under $KIT"
vm_ssh k08 'fio --name=frag --filename=/home/kit/frag --size=8G --rw=randwrite --bs=4k --io_size=1200M --direct=1 --output=/dev/null' 2>/dev/null || vm_ssh k08 'sudo apt-get -qq install -y fio >/dev/null && fio --name=frag --filename=/home/kit/frag --size=8G --rw=randwrite --bs=4k --io_size=1200M --direct=1 --output=/dev/null'
vm_ssh k08 'nohup fio --name=w --filename=/home/kit/frag --size=8G --rw=randwrite --bs=4k --rate_iops=1000 --direct=1 --fsync=1 --time_based --runtime=120 --write_lat_log=/home/kit/lat --log_avg_msec=0 >/dev/null 2>&1 &'
sleep 10; for k in 1 2 3 4 5 6; do s=$(date +%s%N); cp --reflink=always "$KIT/vms/k08/disk.raw" "$KIT/vms/k08/ck$k.raw"; echo "$s $(date +%s%N)" >> "$KIT/vms/k08/clones.txt"; sleep 5; done
sleep 60
vm_ssh k08 'cat /home/kit/lat_clat.1.log' > "$KIT/vms/k08/lat.log"
vm_stop k08 >/dev/null; rm -f "$KIT/vms/k08"/ck*.raw "$KIT/vms/k08/base.raw"
max=$(awk -F, '{v=$2/1e6; if (v>m) m=v} END{printf "%.1f", m}' "$KIT/vms/k08/lat.log")
p99=$(awk -F, '{print $2/1e6}' "$KIT/vms/k08/lat.log" | sort -n | awk '{a[NR]=$1} END{printf "%.1f", a[int(NR*0.99)]}')
result K08 measured "clone strategy: longest write ${max} ms, 99th percentile ${p99} ms over the run (vms/k08/lat.log, clones.txt). delta: run again with the patched libkrun"
