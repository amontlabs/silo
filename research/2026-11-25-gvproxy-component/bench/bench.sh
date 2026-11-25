#!/bin/bash
# Throughput, round trip and new connections per second through gvproxy, per build and option.
# Rig of #366 (research/2026-11-24-linux-host/net.sh): namespace "vm" is the computer, its tap0
# carried by fwd (the monitor's part) to gvproxy over a unix socket; namespace "srv" is the internet
# at 203.0.113.10, not a private address and not on the host's networks, so a sealed gvproxy reaches it.
# A label that ends in "-dgram" carries the frames over a unix datagram socket, one frame per
# datagram (fwdgram, gvproxy's -listen-vfkit), as Apple's framework does on a Mac.
# usage: bench.sh <bin-dir> <runs> <label>:<binary>:<mtu>[:<args>] ...
B=$1; RUNS=$2; shift 2; H=$(cd $(dirname $0); pwd)
cleanup() { ip netns del vm 2>/dev/null; pkill -x fwd; pkill -x fwdgram; pkill -f "^$B/gvproxy"; pkill -f "^$B/standin-boundary"; sleep 0.5; }
ip netns del srv 2>/dev/null; ip link del hs 2>/dev/null
# 203.0.113.10 is off the host's own networks, behind a router at 10.66.0.2, so that -sealed
# (which refuses the networks the host is attached to) still reaches it.
ip route del 203.0.113.10/32 2>/dev/null
ip netns add srv; ip link add hs type veth peer name s0 netns srv
ip addr add 10.66.0.1/30 dev hs; ip link set hs up; ip route add 203.0.113.10/32 via 10.66.0.2
ip -n srv addr add 10.66.0.2/30 dev s0; ip -n srv addr add 203.0.113.10/32 dev lo; ip -n srv link set s0 up; ip -n srv link set lo up
ip -n srv route add default via 10.66.0.1
ip netns exec srv iperf3 -s -D; ip netns exec srv python3 -I $H/rr.py server 7000 > /dev/null 2>&1 & sleep 0.5
gbps() { python3 -c 'import json,sys;print(round(json.load(sys.stdin)["end"]["sum_received"]["bits_per_second"]/1e9,2))'; }
for spec in "$@"; do
  IFS=: read label bin mtu args <<< "$spec"
  for r in $(seq 1 $RUNS); do
    cleanup; rm -f /tmp/gvb.sock /tmp/gvb-api.sock
    if [[ "$args" == *outbound-handoff* ]]; then
      $B/standin-boundary -sock /tmp/silo-boundary.sock -log /dev/null & sleep 0.3
    fi
    if [[ $label == *-dgram ]]; then L="-listen-vfkit unixgram:///tmp/gvb.sock"; F=fwdgram; else L="-listen-qemu unix:///tmp/gvb.sock"; F=fwd; fi
    $B/$bin $L -listen unix:///tmp/gvb-api.sock -mtu $mtu $args > /tmp/gvb.log 2>&1 & sleep 1
    ip netns add vm; ip -n vm link set lo up
    ip netns exec vm $B/$F tap0 /tmp/gvb.sock & sleep 0.5
    ip -n vm link set tap0 mtu $mtu; ip -n vm addr add 192.168.127.2/24 dev tap0; ip -n vm link set tap0 up; ip -n vm route add default via 192.168.127.1
    sleep 1
    up=$(ip netns exec vm iperf3 -c 203.0.113.10 -t 6 -J | gbps)
    down=$(ip netns exec vm iperf3 -c 203.0.113.10 -t 6 -R -J | gbps)
    rr=$(ip netns exec vm python3 -I $H/rr.py client 203.0.113.10 7000)
    rss=$(ps -o rss= -p $(pgrep -f "$B/$bin" | head -1))
    echo "$label mtu=$mtu run=$r up_gbps=$up down_gbps=$down $rr gvproxy_rss_kb=$rss"
  done
done
cleanup; pkill iperf3; pkill -f "rr.py server"; ip netns del srv; ip route del 203.0.113.10/32 2>/dev/null
