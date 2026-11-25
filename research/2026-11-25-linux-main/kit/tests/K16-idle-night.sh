#!/bin/sh
# id: K16
# title: What Ubuntu writes by itself on an idle night
# needs: K07
# decides: 5-the-record.md (linux-noise.zon)
# question: With main idle for KIT_K16_HOURS (default 8), which paths change between hourly checkpoints?
TEST=K16; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k16; vm_boot k16; vm_wait_ssh k16 300 >/dev/null || fail "no SSH"
h=${KIT_K16_HOURS:-8}
cp --reflink=always "$KIT/vms/k16/disk.raw" "$KIT/vms/k16/h0.raw" || skip "no reflink"
off=$(python3 -I -c "import struct,sys;f=open(sys.argv[1],'rb');f.seek(512+72);l,n,s=struct.unpack('<QII',f.read(16));f.seek(l*512+32);print(struct.unpack('<Q',f.read(8))[0]*512)" "$KIT/vms/k16/h0.raw")
for k in $(seq 1 "$h"); do
  sleep 3600; cp --reflink=always "$KIT/vms/k16/disk.raw" "$KIT/vms/k16/h$k.raw"
  KIT_FS_OFFSET=$off python3 -I "$KIT_SRC/../proto/ext4read.py" diff "$KIT/vms/k16/h$((k-1)).raw" "$KIT/vms/k16/h$k.raw" >> "$KIT/vms/k16/noise.txt"
  rm -f "$KIT/vms/k16/h$((k-1)).raw"
done
vm_stop k16 >/dev/null
result K16 measured "$(grep -c '^F' "$KIT/vms/k16/noise.txt") file facts over ${h} idle hours; vms/k16/noise.txt is the start of linux-noise.zon"
