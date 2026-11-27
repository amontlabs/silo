#!/bin/sh
# id: K06
# title: A cold fork from a cloned disk
# needs: K01, a reflink filesystem under $KIT (XFS or Btrfs)
# decides: linux-vm-engine (measurement 2), fork-shares-disk-blocks
# question: How long from a running main's clone to SSH in the fork, and does the fork get its own machine id?
TEST=K06; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k06; vm_boot k06; vm_wait_ssh k06 300 >/dev/null || fail "no SSH"
mid=$(vm_ssh k06 cat /etc/machine-id)
mkdir -p "$KIT/vms/k06f"; cp "$KIT/vms/k06/id" "$KIT/vms/k06/id.pub" "$KIT/vms/k06f/"
s=$(date +%s.%N); cp --reflink=always "$KIT/vms/k06/disk.raw" "$KIT/vms/k06f/disk.raw" || skip "no reflink under $KIT"
c=$(echo "$(date +%s.%N) - $s" | bc)
vm_ssh k06f true 2>/dev/null; vm_boot k06f --no-seed
t=$(vm_wait_ssh k06f 120) || fail "the fork did not boot"
vm_ssh k06f 'sudo rm -f /etc/machine-id && sudo systemd-machine-id-setup >/dev/null'; fmid=$(vm_ssh k06f cat /etc/machine-id)
vm_kill k06f; vm_stop k06 >/dev/null
[ "$mid" != "$fmid" ] || fail "the fork kept main's machine id"
result K06 passed "clone ${c}s, fork to SSH ${t}s, machine id renewed (Silo passes it on the command line instead)"
