#!/bin/bash
# Two members' computers on one host, as network namespaces: cA (member A) and cB (member B).
# B is root in its own computer, as an agent is. Each case asks the shared metadata server for a
# token and reaches for the other member's computer and a host service.
# Run as root; removes what it made.
H=$(cd $(dirname $0); pwd)
clean() { ip netns del cA 2>/dev/null; ip netns del cB 2>/dev/null; ip link del br9 2>/dev/null; ip link del hA 2>/dev/null; ip link del hB 2>/dev/null; pkill -f "http.server --bind" 2>/dev/null; pkill -f meta.py 2>/dev/null; sleep 0.5; }
get() { ip netns exec $1 curl -s -m 2 http://$2/ || echo "no answer"; }
reach() { ip netns exec $1 curl -s -m 2 -o /dev/null -w '%{http_code}' http://$2/ 2>/dev/null | grep -q 200 && echo reached || echo "not reached"; }
clean; ip netns add cA; ip netns add cB
python3 -m http.server --bind 0.0.0.0 8088 --directory /var/empty >/dev/null 2>&1 & HS=$!   # a host service on every address
mkdir -p /var/empty
echo "== 1. One bridge for every computer; the server knows a computer by its address"
ip link add br9 type bridge; ip addr add 10.9.0.1/24 dev br9; ip link set br9 up
for c in A B; do
  ip link add v$c type veth peer name e0 netns c$c; ip link set v$c master br9 up
  ip netns exec c$c ip link set lo up; ip netns exec c$c ip link set e0 up
done
ip netns exec cA ip addr add 10.9.0.11/24 dev e0; ip netns exec cB ip addr add 10.9.0.12/24 dev e0
python3 $H/meta.py by-address & SRV=$!; sleep 1
ip netns exec cA python3 -m http.server --bind 10.9.0.11 80 --directory /var/empty >/dev/null 2>&1 &
sleep 1
echo "A asks:                         $(get cA 10.9.0.1)"
echo "B asks:                         $(get cB 10.9.0.1)"
echo "B reaches A's computer:         $(reach cB 10.9.0.11)"
echo "B reaches a host service:       $(reach cB 10.9.0.1:8088)"
spoof() { # A's computer is stopped (its link down); B takes A's address
  ip netns exec cA ip link set e0 down; ip neigh flush dev br9
  ip netns exec cB ip addr flush dev e0; ip netns exec cB ip addr add 10.9.0.11/24 dev e0
  echo "B, A stopped, with A's address: $(get cB 10.9.0.1)"
  ip netns exec cB ip addr flush dev e0; ip netns exec cB ip addr add 10.9.0.12/24 dev e0
  ip netns exec cA ip link set e0 up; ip neigh flush dev br9; sleep 1
}
spoof
echo "-- the bridge's port isolation on, and each port held to its own address and MAC"
bridge link set dev vA isolated on; bridge link set dev vB isolated on
MA=$(ip netns exec cA cat /sys/class/net/e0/address); MB=$(ip netns exec cB cat /sys/class/net/e0/address)
nft -f - <<NFT
table bridge silo9 {
  chain pre { type filter hook prerouting priority -200;
    iifname "vA" ether saddr != $MA drop
    iifname "vA" ip saddr != 10.9.0.11 drop
    iifname "vA" arp saddr ip != 10.9.0.11 drop
    iifname "vB" ether saddr != $MB drop
    iifname "vB" ip saddr != 10.9.0.12 drop
    iifname "vB" arp saddr ip != 10.9.0.12 drop
  }
}
NFT
echo "A asks:                         $(get cA 10.9.0.1)"
echo "B reaches A's computer:         $(reach cB 10.9.0.11)"
spoof
echo "B reaches a host service:       $(reach cB 10.9.0.1:8088)"
nft delete table bridge silo9; pkill -f meta.py; sleep 0.5
ip netns del cA; ip netns del cB; ip link del br9; ip netns add cA; ip netns add cB
echo "== 2. A link of its own for each computer; the server knows a computer by the link"
n=1; for c in A B; do
  ip link add h$c type veth peer name e0 netns c$c; ip addr add 10.9.$n.1/24 dev h$c; ip link set h$c up
  ip netns exec c$c ip link set lo up; ip netns exec c$c ip addr add 10.9.$n.2/24 dev e0; ip netns exec c$c ip link set e0 up
  ip netns exec c$c ip route add default via 10.9.$n.1; n=$((n+1))
done
python3 $H/meta.py by-link & SRV=$!; sleep 1
echo "A asks:                         $(get cA 10.9.1.1)"
echo "B asks:                         $(get cB 10.9.2.1)"
echo "B asks at A's link address:     $(get cB 10.9.1.1)"
ip netns exec cA python3 -m http.server --bind 10.9.1.2 80 --directory /var/empty >/dev/null 2>&1 & sleep 1
echo "B reaches A's computer (host forwarding $(cat /proc/sys/net/ipv4/ip_forward)): $(reach cB 10.9.1.2)"
echo "B reaches a host service:       $(reach cB 10.9.2.1:8088)"
ip netns exec cA ip link set e0 down; ip netns exec cB ip addr flush dev e0; ip netns exec cB ip addr add 10.9.1.2/24 dev e0
ip netns exec cB ip route add default via 10.9.2.1 dev e0 onlink
echo "B, A stopped, with A's address: $(get cB 10.9.2.1) (asking at its own link); $(get cB 10.9.1.1) (at A's)"
ip netns exec cA ip link set e0 up
clean
