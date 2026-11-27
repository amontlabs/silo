#!/bin/bash
# The two fixes for the network of a shared host, each shown failing first:
#  1. host-out-of-reach: a computer reaches only its boundary on the host. One nft table for all
#     computer links, found by an interface group, not a rule per link.
#  2. computer-known-by-its-link, when a stopped computer's link name is given to a new computer.
# Computers are network namespaces c1..cN with a veth link hK each; Ana owns c1, Ben c2.
# This kernel has no IPv6, so IPv6 link-local addresses were not tried.
# Run as root; removes what it made.
H=$(cd $(dirname $0); pwd); W=$(mktemp -d)
clean() { for n in $(ip netns list | awk '/^c[0-9]/{print $1}'); do ip netns del $n; done; nft delete table inet silo 2>/dev/null
  pkill -f meta2.py; pkill -f httpok.py; pkill -f "iperf3 -s"; sleep 0.5; }
mk() { # mk K OWNER: computer cK, link hK in group 7, 169.254.169.254 on the host side of every link
  ip netns add c$1; ip link add h$1 type veth peer name e0 netns c$1; ip link set h$1 group 7
  ip addr add 169.254.169.254/32 dev h$1; sysctl -qw net.ipv4.conf.h$1.arp_announce=2; ip link set h$1 up
  ip netns exec c$1 ip link set lo up; ip netns exec c$1 ip addr add 10.77.$1.2/32 dev e0; ip netns exec c$1 ip link set e0 up
  ip netns exec c$1 ip route add 169.254.169.254 dev e0; ip netns exec c$1 ip route add default via 169.254.169.254 dev e0 onlink
  ip route add 10.77.$1.2/32 dev h$1; echo "h$1 $2" >> $W/links; }
get() { ip netns exec $1 curl -s -m 2 http://$2/ || echo "no answer"; }
reach() { for t in 1 2 3; do ip netns exec $1 curl -s -m 2 -o /dev/null -w '%{http_code}' "$2" 2>/dev/null | grep -q 200 && { echo reached; return; }; done; echo "not reached (3 tries)"; }
clean; : > $W/links; HOSTIP=$(ip -4 route get 1.1.1.1 | awk '{for(i=1;i<NF;i++) if($i=="src") print $(i+1)}')
mk 1 Ana; mk 2 Ben
python3 $H/meta2.py $W/links >/dev/null & sleep 1
python3 $H/httpok.py 0.0.0.0 8088 >/dev/null 2>&1 & sleep 1   # Ana's dev server, every address
ip netns exec c1 python3 $H/httpok.py 10.77.1.2 8000 >/dev/null 2>&1 & sleep 1
FWD=$(cat /proc/sys/net/ipv4/ip_forward)
case1() {
  echo "Ana asks the metadata server:            $(get c1 169.254.169.254)"
  echo "Ben asks the metadata server:            $(get c2 169.254.169.254)"
  echo "Ben reaches the dev server, at his link: $(reach c2 http://169.254.169.254:8088/)"
  echo "Ben reaches it at the host's own address: $(reach c2 http://$HOSTIP:8088/)"
  echo 1 > /proc/sys/net/ipv4/ip_forward; sleep 2
  echo "Ben reaches Ana's computer, host forwarding on (as Docker sets it): $(reach c2 http://10.77.1.2:8000/)"
  echo $FWD > /proc/sys/net/ipv4/ip_forward
}
echo "== 1. host-out-of-reach: a link per computer, no host firewall"; case1
nft -f - <<'NFT'
table inet silo {
  chain input { type filter hook input priority filter - 10; policy accept;
    iifgroup 7 ct state established,related accept
    iifgroup 7 ip daddr 169.254.169.254 tcp dport { 80, 5201 } accept
    iifgroup 7 counter drop
  }
  chain forward { type filter hook forward priority filter - 10; policy accept;
    iifgroup 7 counter drop
    oifgroup 7 counter drop
  }
}
NFT
echo "-- one nft table: from a link in group 7, only the boundary's ports at 169.254.169.254"; case1
echo "== 2. A stopped computer's link name given to a new computer of Ben's"
pkill -f "httpok.py 10.77.1.2"; sleep 0.5; ip netns del c1; for i in $(seq 50); do ip link show h1 >/dev/null 2>&1 || break; sleep 0.1; done
echo "Ana's computer stopped; link h1 gone:    $(ip link show h1 >/dev/null 2>&1 && echo still there || echo gone)"
ip netns add c9; ip link add h1 type veth peer name e0 netns c9; ip link set h1 group 7; ip addr add 169.254.169.254/32 dev h1; ip link set h1 up
ip netns exec c9 ip link set lo up; ip netns exec c9 ip addr add 10.77.9.2/32 dev e0; ip netns exec c9 ip link set e0 up; ip netns exec c9 ip route add 169.254.169.254 dev e0
sleep 1
echo "Ben's new computer on a new h1 asks, the server's listeners held by link index: $(get c9 169.254.169.254)"
pkill -f meta2.py; sleep 0.5; python3 $H/meta2.py $W/links by-name >/dev/null & sleep 1
echo "the same, a server restarted that keys its table by link name, the table not yet rewritten: $(get c9 169.254.169.254)"
pkill -f meta2.py; sed -i 's/^h1 Ana$/h1 Ben/' $W/links; python3 $H/meta2.py $W/links >/dev/null & sleep 1
echo "the engine rewrites the table with the link, then the listener opens: $(get c9 169.254.169.254)"
clean; ip link del h1 2>/dev/null; rm -rf $W
