#!/bin/sh
# id: K03
# title: The way in over vsock, with and without a network device
# needs: K01
# decides: 2-main-runs.md (the way in), network-before-rules (a closed main keeps SSH)
# question: Does SSH reach main over vsock port 22 with no network device at all? Does systemd's own generator listen on vsock too?
TEST=K03; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k03; vm_boot k03; vm_wait_ssh k03 300 >/dev/null || fail "no SSH"
gen=$(vm_ssh k03 'systemctl list-units --no-legend "sshd-vsock*" "*ssh*vsock*" 2>/dev/null | wc -l')
vm_stop k03 >/dev/null
vm_boot k03 --no-net --no-seed
t=$(vm_wait_ssh k03 120) || fail "no SSH without a network device"
ifs=$(vm_ssh k03 'ls /sys/class/net | grep -v ^lo | wc -l')
vm_stop k03 >/dev/null
[ "$ifs" = 0 ] || fail "a network device exists ($ifs) though none was given"
result K03 passed "SSH over vsock with no network device in ${t}s; vsock socket units: $gen"
