#!/bin/bash
# What KSM saves on five idle Linux guests booted from one image, one per member.
# QEMU without KVM (TCG); each guest 1 GiB, 1 processor, its own overlay on a Debian 12 cloud image.
# Measures the guests' resident RAM at the login prompt, then turns KSM on and reads what it
# merged every 30 s, STEPS times (10). ZERO=1 counts pages of zeros apart (use_zero_pages). $1 = the base image. Run as root; stops the guests after.
B=$1; W=$(mktemp -d); H=$(cd $(dirname $0); pwd); K=/sys/kernel/mm/ksm; N=${N:-5}
echo 0 > $K/run; echo 2 > $K/run; echo 0 > $K/run   # 2 unmerges everything first
for g in $(seq 1 $N); do
  qemu-img create -q -f qcow2 -b $B -F qcow2 $W/g$g.qcow2
  qemu-system-x86_64 -accel tcg,thread=multi,tb-size=64 -m 1024 -smp 1 -display none \
    -serial file:$W/g$g.log -drive file=$W/g$g.qcow2,if=virtio -nic none -pidfile $W/g$g.pid -daemonize
done
for i in $(seq 1 120); do sleep 5; n=$(grep -l 'login:' $W/g*.log 2>/dev/null | wc -l); [ $n = $N ] && break; done
echo "$N guests at the login prompt after $((i*5)) s"; sleep 30
P=$(cat $W/g*.pid); tot() { python3 $H/guestram.py $P | awk '{s+=$5} END {print s}'; }
echo "guest RAM resident, KSM off: $(tot) MiB in all"
echo ${ZERO:-0} > $K/use_zero_pages; echo 1000 > $K/pages_to_scan; echo 20 > $K/sleep_millisecs; echo 1 > $K/run; t0=$(date +%s)
for k in $(seq 1 ${STEPS:-10}); do sleep 30
  echo "KSM on $(( $(date +%s)-t0 )) s: resident $(tot) MiB; merged into $(( $(cat $K/pages_shared)*4/1024 )) MiB, saving $(( $(cat $K/pages_sharing)*4/1024 )) MiB, plus $(( $(cat $K/ksm_zero_pages)*4/1024 )) MiB of zero pages; ksmd CPU $(ps -o time= -C ksmd)"
done
echo 0 > $K/run; echo 0 > $K/use_zero_pages; for g in $P; do kill $g; done; rm -rf $W
