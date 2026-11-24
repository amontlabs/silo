#!/bin/bash
# Network paths for a Linux computer, without a virtual machine: a namespace "vm" stands for the
# computer, a namespace "srv" for the internet. iperf3 and rr.py from vm to srv through:
#   kernel: veth + host forwarding + an nftables allow-list (what tap + nftables gives)
#   passt / gvproxy: a tap in vm, carried by fwd (the monitor's part) to the user-space stack
# Needs: iperf3, nft, passt, gvproxy, fwd (built from fwd.go). Run as root.
H=$(dirname $(readlink -f $0)); GV=${GV:-/var/tmp/gob/gvproxy}; FWD=${FWD:-/var/tmp/fwd/fwd}
ip netns del vm 2>/dev/null; ip netns del srv 2>/dev/null; nft delete table inet silo 2>/dev/null; pkill -x passt; pkill -x gvproxy; pkill -x fwd; sleep 0.5
ip netns add srv; ip link add hs type veth peer name s0 netns srv
ip addr add 10.99.0.1/24 dev hs; ip link set hs up
ip -n srv addr add 10.99.0.2/24 dev s0; ip -n srv link set s0 up; ip -n srv link set lo up
ip -n srv route add default via 10.99.0.1
ip netns exec srv iperf3 -s -D; ip netns exec srv python3 $H/rr.py server 7000 & sleep 0.5
sysctl -qw net.ipv4.ip_forward=1
measure() { # $1 label
  up=$(ip netns exec vm iperf3 -c 10.99.0.2 -t 8 -J | python3 -c 'import json,sys;print(round(json.load(sys.stdin)["end"]["sum_received"]["bits_per_second"]/1e9,2))')
  down=$(ip netns exec vm iperf3 -c 10.99.0.2 -t 8 -R -J | python3 -c 'import json,sys;print(round(json.load(sys.stdin)["end"]["sum_received"]["bits_per_second"]/1e9,2))')
  rr=$(ip netns exec vm python3 $H/rr.py client 10.99.0.2 7000)
  echo "$1: up_gbps=$up down_gbps=$down $rr $2"
}
# kernel path
ip netns add vm; ip link add hv type veth peer name v0 netns vm
ip addr add 10.98.0.1/24 dev hv; ip link set hv up
ip -n vm addr add 10.98.0.2/24 dev v0; ip -n vm link set v0 up; ip -n vm link set lo up; ip -n vm route add default via 10.98.0.1
nft -f - <<NFT
table inet silo {
  set allowed { type ipv4_addr . inet_service; elements = { 10.99.0.2 . 5201, 10.99.0.2 . 7000 } }
  chain gate { type filter hook forward priority 0; policy drop;
    iifname "hv" ip daddr . tcp dport @allowed counter accept
    ct state established,related counter accept
  }
}
NFT
measure kernel-nftables
nft delete table inet silo; ip netns del vm
# a user-space stack: $1 label, $2 tap mtu, $3 gateway/addr setup is static
userspace() {
  ip netns add vm; ip -n vm link set lo up
  ip netns exec vm $FWD tap0 $1 & sleep 0.5
  ip -n vm link set tap0 mtu $2; ip -n vm addr add $3 dev tap0; ip -n vm link set tap0 up; ip -n vm route add default via $4
  sleep 1
}
passt -f -q -s /tmp/passt.sock -a 10.97.0.2 -n 24 -g 10.97.0.1 --no-dhcp --no-dhcpv6 --no-ndp --no-ra >/tmp/passt.log 2>&1 & sleep 1
userspace /tmp/passt.sock 65520 10.97.0.2/24 10.97.0.1
P=$(pgrep -f "^passt"); measure passt "rss_kb=$(ps -o rss= -p $P)"
pkill -x fwd; pkill -f ^passt; sleep 1; ip netns del vm
for mtu in 1500 65520; do
  rm -f /tmp/gv.sock /tmp/gvapi.sock
  $GV -listen-qemu unix:///tmp/gv.sock -listen unix:///tmp/gvapi.sock -mtu $mtu >/tmp/gv.log 2>&1 & sleep 1
  userspace /tmp/gv.sock $mtu 192.168.127.2/24 192.168.127.1
  P=$(pgrep -x gvproxy); measure gvproxy-mtu$mtu "rss_kb=$(ps -o rss= -p $P)"
  pkill -x fwd; pkill -x gvproxy; sleep 1; ip netns del vm
done
pkill iperf3; pkill -f "rr.py server"; ip netns del srv
