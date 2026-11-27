#!/bin/bash
# How much of a running computer stays the same as the memory snapshot it was forked from.
# A fork that maps its person's snapshot copy-on-write shares every page it has not written with
# the other forks of that person, and with nobody else's. QEMU without KVM (TCG); one Debian 12
# guest, 1 GiB, 1 processor, no network. cloud-init prints READY on the serial line, waits 30 s
# (the snapshot is taken then), and, with MODE=busy, runs work (reads files, compresses, allocates).
# The guest's RAM is dumped (pmemsave) at the snapshot and at each of TIMES seconds after.
# $1 = base image. Run as root.
B=$1; MODE=${MODE:-idle}; W=$(mktemp -d); H=$(cd $(dirname $0); pwd); TIMES=${TIMES:-60 180 360 600}
cat > $W/user-data <<UD
#cloud-config
runcmd:
  - [ sh, -c, 'echo READY > /dev/ttyS0; sleep 30; [ $MODE = busy ] && sh /root/work.sh' ]
write_files:
  - path: /root/work.sh
    content: |
      while true; do
        find /usr -type f 2>/dev/null | head -30000 | xargs cat > /dev/null 2>&1
        tar cf - /usr/share 2>/dev/null | gzip -1 > /var/tmp/a.tgz; rm -f /var/tmp/a.tgz
        python3 -c 'a = bytearray(200 << 20); [a.__setitem__(i, 1) for i in range(0, len(a), 4096)]'
        sleep 5
      done
UD
echo "instance-id: s1" > $W/meta-data; cloud-localds $W/seed.iso $W/user-data $W/meta-data
qemu-img create -q -f qcow2 -b $B -F qcow2 $W/g.qcow2
qemu-system-x86_64 -accel tcg,thread=multi,tb-size=64 -m 1024 -smp 1 -display none -nic none \
  -serial file:$W/serial.log -drive file=$W/g.qcow2,if=virtio -drive file=$W/seed.iso,if=virtio,format=raw \
  -qmp unix:$W/qmp,server,nowait -pidfile $W/pid -daemonize
qmp() { python3 - "$W/qmp" "$1" <<'PY'
import socket, sys, json
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); f = s.makefile("rw")
f.readline(); f.write('{"execute":"qmp_capabilities"}\n'); f.flush(); f.readline()
f.write(sys.argv[2] + "\n"); f.flush()
while True:
    l = json.loads(f.readline())
    if "return" in l or "error" in l: print(l); break
PY
}
dump() { qmp "{\"execute\":\"pmemsave\",\"arguments\":{\"val\":0,\"size\":1073741824,\"filename\":\"$W/$1\"}}" >/dev/null; }
for i in $(seq 1 300); do grep -q READY $W/serial.log 2>/dev/null && break; sleep 2; done
echo "mode $MODE: READY after $((i*2)) s"; sleep 5; dump snap; t0=$(date +%s); D=""
for t in $TIMES; do while [ $(( $(date +%s) - t0 )) -lt $t ]; do sleep 1; done; dump at-${t}s; D="$D $W/at-${t}s"; done
python3 $H/pagesame.py $W/snap $D
kill $(cat $W/pid); rm -rf $W
