#!/bin/sh
# id: K01
# title: Silo's kernel boots Ubuntu 26.04 through libkrun, and systemd reaches running
# needs: K00
# decides: linux-vm-engine (measurement 1), 2-main-runs.md (the kernel, the seed, the first run)
# question: How long from start to SSH on the first boot and on the next; is systemd "running" with no modules?
# outcome running: first and second boot times recorded; systemctl is-system-running says running
# outcome degraded: a unit failed: the list is kept; the kernel's configuration or the seed needs a fix
TEST=K01; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k01
vm_boot k01
first=$(vm_wait_ssh k01 300) || fail "no SSH within 300 s; see $KIT/vms/k01/console.log"
vm_ssh k01 cloud-init status --wait >/dev/null 2>&1
state=$(vm_ssh k01 systemctl is-system-running 2>/dev/null)
vm_ssh k01 systemctl --failed --no-legend > "$KIT/vms/k01/failed.txt" 2>/dev/null
modprobe_out=$(vm_ssh k01 'sudo modprobe dummy 2>&1; echo $?' | tail -1)
vm_stop k01 >/dev/null
vm_boot k01 --no-seed
second=$(vm_wait_ssh k01 120) || fail "no SSH on the second boot"
cid=$(vm_ssh k01 test -f /etc/cloud/cloud-init.disabled && echo disabled || echo enabled)
vm_stop k01 >/dev/null
[ "$state" = running ] && o=running || o=degraded
result K01 $o "first boot to SSH ${first}s, second ${second}s, systemd $state, modprobe exit $modprobe_out, cloud-init $cid; failed units in vms/k01/failed.txt"
