#!/bin/sh
# id: K02
# title: Docker runs in main
# needs: K01, network
# decides: linux-vm-engine (measurement 3), containers-on-macos
# question: Does Docker's own check pass on Silo's kernel, and does a container run with a bridge network?
TEST=K02; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k02; vm_boot k02; vm_wait_ssh k02 300 >/dev/null || fail "no SSH"
vm_ssh k02 cloud-init status --wait >/dev/null 2>&1
vm_ssh k02 'curl -fsSL https://get.docker.com | sudo sh' > "$KIT/vms/k02/install.log" 2>&1 || fail "Docker's installer failed; vms/k02/install.log"
out=$(vm_ssh k02 'sudo docker run --rm hello-world 2>&1 | grep -c "Hello from Docker"; sudo docker run --rm alpine wget -qO- http://example.com | grep -c Example')
vm_ssh k02 'curl -fsSL https://raw.githubusercontent.com/moby/moby/master/contrib/check-config.sh | sudo bash' > "$KIT/vms/k02/check-config.txt" 2>&1 || true
missing=$(grep -c 'missing' "$KIT/vms/k02/check-config.txt" || true)
vm_stop k02 >/dev/null
[ "$(echo "$out" | tr -d '\n')" = 11 ] && result K02 passed "hello-world and a bridged container ran; check-config lists $missing missing (optional ones included)" || fail "a container did not run ($out); see vms/k02"
