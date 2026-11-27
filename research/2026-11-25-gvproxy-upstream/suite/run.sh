#!/bin/bash
# One gvproxy, one computer that attacks it from inside, and the checks made from outside.
# From #380 (research/2026-11-25-gvproxy-component/suite/), with a metadata service added at
# 169.254.169.254 on the local network, as a cloud host has one.
# usage: run.sh <label> <gvproxy> [gvproxy args...]     (as root; prints "<case> <result>" lines)
# Needs gvforwarder (GVFWD), python3, curl, ping, iproute2. BOUNDARY_LOG, when set, is a file the
# stand-in boundary writes; the dns-exfil check reads it too.
H=$(cd $(dirname $0); pwd); LABEL=$1; GV=$2; shift 2
W=${WORK:-/tmp/gvsuite}/$LABEL; rm -rf $W; mkdir -p $W
GVFWD=${GVFWD:-$(dirname $GV)/gvforwarder}
sysctl -qw net.ipv4.ping_group_range="0 2147483647"   # unprivileged ICMP sockets, as on a Mac
sysctl -qw net.ipv4.ip_forward=1
# The internet: 203.0.113.10 (TEST-NET-3), neither the host nor a private network, and not on any
# network the host is attached to: the host reaches it through a router at 10.66.0.2.
ip netns add inet; ip link add vinet type veth peer name e0 netns inet
ip addr add 10.66.0.1/30 dev vinet; ip link set vinet up; ip route add 203.0.113.10/32 via 10.66.0.2
ip -n inet addr add 10.66.0.2/30 dev e0; ip -n inet addr add 203.0.113.10/32 dev lo; ip -n inet link set e0 up; ip -n inet link set lo up; ip -n inet route add default via 10.66.0.1
# A local network on public addresses, as in an office or a university: the host is 198.51.100.1,
# a neighbour 198.51.100.2 (TEST-NET-2, not a private range).
ip netns add pub; ip link add vpub type veth peer name p0 netns pub
ip addr add 198.51.100.1/24 dev vpub; ip link set vpub up
ip -n pub addr add 198.51.100.2/24 dev p0; ip -n pub link set p0 up; ip -n pub link set lo up
# The local network: the host is 10.77.0.1, a neighbour (a printer, a NAS, the router) is 10.77.0.2,
# which also runs the host's DNS resolver.
ip netns add lan; ip link add vlan type veth peer name l0 netns lan
ip addr add 10.77.0.1/24 dev vlan; ip link set vlan up
ip -n lan addr add 10.77.0.2/24 dev l0; ip -n lan link set l0 up; ip -n lan link set lo up
# A cloud's metadata service, 169.254.169.254, reached through the neighbour, as on a cloud host.
ip -n lan addr add 169.254.169.254/32 dev lo; ip route add 169.254.169.254/32 via 10.77.0.2
# A machine of the host's network at the address the virtual network gives its own gateway,
# 192.168.127.1, as on a home network that uses 192.168.127.0/24 too.
ip -n lan addr add 192.168.127.1/32 dev lo; ip route add 192.168.127.1/32 via 10.77.0.2
HOST_LAN=$(ip -4 -o addr show eth0 | awk '{print $4}' | cut -d/ -f1)
P=()
start() { "$@" > /dev/null 2>&1 & P+=($!); }
start python3 -I $H/servers.py tcp 127.0.0.1 7777 "secret on the host's loopback"
start python3 -I $H/servers.py tcp 0.0.0.0 7778 "a service of the host, on every address"
start ip netns exec lan python3 -I $H/servers.py tcp 10.77.0.2 7778 "a neighbour on the local network"
start ip netns exec lan python3 -I $H/servers.py udp 10.77.0.2 7781 $W/udp-lan.log
start ip netns exec lan python3 -I $H/servers.py tcp 169.254.169.254 80 "the metadata service"
start ip netns exec lan python3 -I $H/servers.py tcp 192.168.127.1 7790 "a machine at the gateway's address"
start ip netns exec lan python3 -I $H/servers.py dns 10.77.0.2 $W/dns-host-resolver.log 203.0.113.10
start ip netns exec pub python3 -I $H/servers.py tcp 198.51.100.2 7778 "a neighbour on a public local network"
start ip netns exec inet python3 -I $H/servers.py tcp 203.0.113.10 8088 "a host on the internet"
start ip netns exec inet python3 -I $H/servers.py udp 203.0.113.10 7781 $W/udp-inet.log
# gvproxy sees a resolv.conf that names the neighbour, as a Mac at home names its router.
echo "nameserver 10.77.0.2" > $W/host-resolv.conf
unshare -m bash -c "mount --bind $W/host-resolv.conf /etc/resolv.conf; exec $GV -listen unix://$W/net.sock -mtu 1500 $*" > $W/gvproxy.log 2>&1 & G=$!
sleep 1
EXFIL=x$RANDOM$RANDOM.exfil.example
cat > $W/guest.sh <<G
echo "nameserver 192.168.127.1" > $W/resolv.conf; mount --bind $W/resolv.conf /etc/resolv.conf
ip link set lo up
$GVFWD -url unix://$W/net.sock -iface tap0 -stop-if-exist "" -mtu 1500 > $W/fwd.log 2>&1 &
for i in \$(seq 1 40); do ip -4 addr show tap0 2>/dev/null | grep -q inet && break; sleep 0.25; done
python3 -I $H/servers.py tcp 0.0.0.0 8000 "served from inside the computer" > /dev/null 2>&1 &
sleep 0.3
LOOP_PORT=7777 LAN_PORT=7778 NET_PORT=8088 UDP_PORT=7781 HOST_LAN=$HOST_LAN NEIGHBOUR=10.77.0.2 PUB_NEIGHBOUR=198.51.100.2 INTERNET=203.0.113.10 EXFIL=$EXFIL python3 -I $H/guest.py $CASES
G
unshare -n -m bash $W/guest.sh
# From outside the computer.
LS=$(ss -ltnp | grep "pid=$G," | awk '{print $4}' | sort | tr '\n' ',' | sed 's/,$//'); echo "host-listeners ${LS:-none}"
c=$(echo hi | timeout 3 python3 -c 'import socket,sys;s=socket.create_connection(("'$HOST_LAN'",2299),2);s.sendall(b"hi\n");print(s.recv(100).decode().strip())' 2>/dev/null); echo "host-port-opened-from-inside $([ -n "$c" ] && echo yes || echo no)"
code=$(curl -s -m 3 --unix-socket $W/net.sock -o /dev/null -w '%{http_code}' -X POST http://x/services/forwarder/expose -d '{"local":"127.0.0.1:3300","remote":"192.168.127.2:8000"}')
got=$(timeout 3 python3 -c 'import socket;s=socket.create_connection(("127.0.0.1",3300),2);s.sendall(b"hi\n");print(s.recv(100).decode().strip())' 2>/dev/null)
echo "way-in $([ "$got" = "served from inside the computer" ] && echo works || echo broken)"
seen=no; grep -q "$EXFIL" $W/dns-host-resolver.log 2>/dev/null && seen=host-resolver
[ -n "$BOUNDARY_LOG" ] && grep -q "$EXFIL" $BOUNDARY_LOG 2>/dev/null && seen=${seen/no/}${seen:+,}boundary && seen=${seen#,}
echo "dns-exfil-seen-by $seen"
echo "udp-lan-datagrams $(wc -l < $W/udp-lan.log 2>/dev/null || echo 0)"
kill $G 2>/dev/null; wait $G 2>/dev/null; kill ${P[@]} 2>/dev/null; pkill -f "$W/" 2>/dev/null; pkill -f "servers.py tcp 0.0.0.0 8000" 2>/dev/null
ip route del 203.0.113.10/32 2>/dev/null; ip route del 169.254.169.254/32 2>/dev/null; ip route del 192.168.127.1/32 2>/dev/null; ip netns del inet; ip netns del lan; ip netns del pub; ip link del vinet 2>/dev/null; ip link del vlan 2>/dev/null; ip link del vpub 2>/dev/null
sleep 0.5
