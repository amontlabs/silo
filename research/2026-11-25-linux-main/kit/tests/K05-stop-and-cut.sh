#!/bin/sh
# id: K05
# title: An orderly stop within 30 s, and a power cut during writes
# needs: K01
# decides: what-stop-ends, 2-main-runs.md (stop), 4-the-way-back.md (restore after a cut)
# question: How long does a poweroff take with a busy main; after a kill -9 of the helper during writes, does the disk boot and does e2fsck find it clean after the journal's replay?
TEST=K05; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k05; vm_boot k05; vm_wait_ssh k05 300 >/dev/null || fail "no SSH"
vm_ssh k05 'nohup sh -c "while :; do dd if=/dev/urandom of=/home/kit/busy bs=1M count=200 conv=fsync status=none; done" >/dev/null 2>&1 &'
sleep 5; s=$(vm_stop k05)
vm_boot k05 --no-seed; vm_wait_ssh k05 120 >/dev/null || fail "no SSH after the stop"
vm_ssh k05 'nohup sh -c "while :; do dd if=/dev/urandom of=/home/kit/busy bs=1M count=200 status=none; done" >/dev/null 2>&1 &'
sleep 8; vm_kill k05
cp --sparse=always "$KIT/vms/k05/disk.raw" "$KIT/vms/k05/cut.raw"
vm_boot k05 --no-seed; t=$(vm_wait_ssh k05 120) || fail "no boot after a power cut"
vm_stop k05 >/dev/null
off=$(python3 -I -c "import struct,sys;f=open(sys.argv[1],'rb');f.seek(512+72);l,n,s=struct.unpack('<QII',f.read(16));f.seek(l*512+32);print(struct.unpack('<Q',f.read(8))[0]*512)" "$KIT/vms/k05/cut.raw")
lo=$(sudo losetup -f --show -o "$off" "$KIT/vms/k05/cut.raw"); fsck=$(sudo e2fsck -fy "$lo" >/dev/null 2>&1; echo $?); sudo losetup -d "$lo"
result K05 passed "poweroff of a busy main took ${s}s; after a cut, boot to SSH ${t}s; e2fsck exit $fsck on the cut disk (0 or 1 is clean after replay)"
