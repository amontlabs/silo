#!/bin/sh
# id: K10
# title: A deleted file's space comes back to the host after fstrim
# needs: K01
# decides: space-the-engine-keeps, 4-the-way-back.md (discards)
# question: After main writes then deletes 1 GiB and runs fstrim, how much of disk.raw's allocated space is freed on the host?
TEST=K10; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k10; vm_boot k10; vm_wait_ssh k10 300 >/dev/null || fail "no SSH"
vm_ssh k10 'dd if=/dev/urandom of=/home/kit/big bs=1M count=1024 conv=fsync status=none'
a=$(du -k "$KIT/vms/k10/disk.raw" | cut -f1)
vm_ssh k10 'rm /home/kit/big && sync && sudo fstrim -v /' > "$KIT/vms/k10/fstrim.txt" 2>&1
sleep 2; b=$(du -k "$KIT/vms/k10/disk.raw" | cut -f1)
vm_stop k10 >/dev/null
freed=$(( (a - b) / 1024 ))
[ $freed -ge 900 ] && result K10 passed "${freed} MiB of 1024 came back to the host" || result K10 failed "only ${freed} MiB came back; vms/k10/fstrim.txt"
