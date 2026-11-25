#!/bin/bash
# The ways into a new computer before Silo's relay is in it, each measured the same way on Linux.
# usage: ways.sh <bin-dir> <way>...      ways: nat gvproxy-closed one-connection relay serial
# <bin-dir> holds gvproxy-silo and fwdgram (#380's build.sh) and cardend (../cardend). As root.
#
# The computer is a network namespace. Its card is a tap device whose frames go one per datagram
# over a unix socket (fwdgram), as Apple's VZFileHandleNetworkDeviceAttachment hands them over,
# except for `nat`, where a veth pair and the kernel's NAT stand in for Apple's. sshd listens on
# port 22 in it, as Apple's provisioning leaves Remote Login. Around it, #380's suite places a
# service on the Mac's loopback, one on every address of the Mac, a neighbour on the local network
# that also runs the Mac's resolver, and an internet host. For each way, the computer probes what
# it can reach, and the Mac tries the way in with ssh. Results: out/<way>/result.txt.
B=$(cd $1; pwd); shift
H=$(cd $(dirname $0); pwd); SUITE=$H/../../2026-11-25-gvproxy-component/suite
O=${OUT:-$H/out}; mkdir -p $O
sysctl -qw net.ipv4.ping_group_range="0 2147483647"; sysctl -qw net.ipv4.ip_forward=1
HOST_LAN=$(ip -4 -o addr show eth0 | awk '{print $4}' | cut -d/ -f1)

world_up() {
  ip netns add inet; ip link add vinet type veth peer name e0 netns inet
  ip addr add 203.0.113.1/24 dev vinet; ip link set vinet up
  ip -n inet addr add 203.0.113.10/24 dev e0; ip -n inet link set e0 up; ip -n inet link set lo up; ip -n inet route add default via 203.0.113.1
  ip netns add lan; ip link add vlan type veth peer name l0 netns lan
  ip addr add 10.77.0.1/24 dev vlan; ip link set vlan up
  ip -n lan addr add 10.77.0.2/24 dev l0; ip -n lan link set l0 up; ip -n lan link set lo up
  WP=()
  w() { "$@" > /dev/null 2>&1 & WP+=($!); }
  w python3 -I $SUITE/servers.py tcp 127.0.0.1 7777 "secret on the host's loopback"
  w python3 -I $SUITE/servers.py tcp 0.0.0.0 7778 "a service of the host, on every address"
  w ip netns exec lan python3 -I $SUITE/servers.py tcp 10.77.0.2 7778 "a neighbour on the local network"
  w ip netns exec lan python3 -I $SUITE/servers.py udp 10.77.0.2 7781 $W/udp-lan.log
  w ip netns exec lan python3 -I $SUITE/servers.py dns 10.77.0.2 $W/dns-host-resolver.log 203.0.113.10
  w ip netns exec inet python3 -I $SUITE/servers.py tcp 203.0.113.10 8088 "a host on the internet"
  w ip netns exec inet python3 -I $SUITE/servers.py udp 203.0.113.10 7781 $W/udp-inet.log
}
world_down() {
  kill ${WP[@]} 2>/dev/null; ip netns del inet; ip netns del lan; ip link del vinet 2>/dev/null; ip link del vlan 2>/dev/null
}

# The computer: a namespace with sshd on port 22, Silo's key authorised, as after provisioning.
computer_up() {
  ip netns add vm; ip -n vm link set lo up
  ssh-keygen -q -t ed25519 -N "" -f $W/hostkey; ssh-keygen -q -t ed25519 -N "" -f $W/silokey
  cp $W/silokey.pub $W/authorized_keys; chmod 600 $W/authorized_keys
  cat > $W/sshd_config <<C
HostKey $W/hostkey
AuthorizedKeysFile $W/authorized_keys
PermitRootLogin prohibit-password
PasswordAuthentication no
PidFile none
StrictModes no
C
  mkdir -p /run/sshd
  ip netns exec vm /usr/sbin/sshd -D -f $W/sshd_config -o ListenAddress=0.0.0.0 -o ListenAddress=:: > $W/sshd.log 2>&1 & SSHD=$!
}
SSH() { ssh -q -i $W/silokey -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -o BatchMode=yes "$@"; }
EXFIL=x$RANDOM$RANDOM.exfil.example
probe() {  # <resolv nameserver or none> <addresses to scan>
  ip netns exec vm bash -c "
    mkdir -p $W/etc; echo ${1/none/# no name server} > $W/etc/resolv.conf; mount --bind $W/etc/resolv.conf /etc/resolv.conf
    LOOP_PORT=7777 LAN_PORT=7778 NET_PORT=8088 UDP_PORT=7781 HOST_LAN=$HOST_LAN NEIGHBOUR=10.77.0.2 INTERNET=203.0.113.10 EXFIL=$EXFIL python3 -I $H/probe.py $SUITE/guest.py $2" 2>/dev/null
  seen=nobody; grep -q "$EXFIL" $W/dns-host-resolver.log 2>/dev/null && seen=host-resolver
  echo "dns-exfil-seen-by $seen"
}
# Wait for the way in, then time and check it: one command over ssh, then a 2 MB copy (the relay).
way_in() {  # ssh args...
  t0=$(date +%s.%N); ok=no
  for i in $(seq 1 60); do out=$(SSH "$@" 'echo in' 2>/dev/null); [ "$out" = in ] && { ok=yes; break; }; sleep 0.25; done
  t1=$(date +%s.%N)
  echo "way-in $ok"; echo "way-in-wait-s $(python3 -c "print(round($t1-$T_START,1))")"
  head -c 2000000 /dev/urandom > $W/blob; sum=$(sha256sum < $W/blob | cut -c1-16)
  t2=$(date +%s.%N); got=$(SSH "$@" 'cat > /tmp/relay.blob; sha256sum < /tmp/relay.blob | cut -c1-16' < $W/blob 2>/dev/null); t3=$(date +%s.%N)
  echo "copy-2MB $([ "$got" = "$sum" ] && echo intact || echo broken) $(python3 -c "print(round($t3-$t2,2))")s"
}
frames() { [ -s $W/counts.json ] && echo "frames-dropped $(python3 -c "import json;print('; '.join(f'{k} x{n}' for k,n in json.load(open('$W/counts.json'))))")"; }
stop_all() { kill $SSHD $MAC $FWD $RELAY 2>/dev/null; sleep 0.3; ip netns del vm 2>/dev/null; iptables -t nat -F 2>/dev/null; iptables -F FORWARD 2>/dev/null; ip link del vnat 2>/dev/null; }

for WAY in "$@"; do
  W=$O/$WAY; rm -rf $W; mkdir -p $W; SSHD= MAC= FWD= RELAY=
  world_up; computer_up; T_START=$(date +%s.%N)
  {
  echo "# way: $WAY"
  case $WAY in
  nat)  # Apple's NAT: the Mac is the computer's router at 192.168.64.1 and forwards its lookups.
    ip link add vnat type veth peer name en0 netns vm
    ip addr add 192.168.64.1/24 dev vnat; ip link set vnat up
    ip -n vm addr add 192.168.64.2/24 dev en0; ip -n vm link set en0 up; ip -n vm route add default via 192.168.64.1
    iptables -t nat -A POSTROUTING -s 192.168.64.0/24 ! -d 192.168.64.0/24 -j MASQUERADE
    iptables -t nat -A PREROUTING -i vnat -d 192.168.64.1 -p udp --dport 53 -j DNAT --to 10.77.0.2:53
    way_in root@192.168.64.2
    probe "nameserver 192.168.64.1" 192.168.64.1 ;;
  gvproxy-closed)  # #380's patched gvproxy, closed; Silo's way in is a forward on gvproxy's own socket.
    $B/gvproxy-silo -closed -listen-vfkit unixgram://$W/frames.sock -listen unix://$W/api.sock -mtu 1500 > $W/mac.log 2>&1 & MAC=$!; sleep 1
    ip netns exec vm $B/fwdgram tap0 $W/frames.sock > $W/fwd.log 2>&1 & FWD=$!; sleep 0.5
    ip -n vm addr add 192.168.127.2/24 dev tap0; ip -n vm link set tap0 up; ip -n vm route add default via 192.168.127.1
    curl -s -m 3 --unix-socket $W/api.sock -X POST http://x/services/forwarder/expose -d '{"local":"127.0.0.1:2222","remote":"192.168.127.2:22"}' > /dev/null
    way_in -p 2222 root@127.0.0.1
    probe "nameserver 192.168.127.1" 192.168.127.1,192.168.127.254 ;;
  one-connection)  # cardend -dial: no address handed out, one TCP connection to port 22 at the computer's link-local address.
    $B/cardend -frames $W/frames.sock -counts $W/counts.json -dial $W/dial.sock > $W/mac.log 2>&1 & MAC=$!; sleep 0.5
    ip netns exec vm $B/fwdgram tap0 $W/frames.sock > $W/fwd.log 2>&1 & FWD=$!; sleep 0.3
    # What macOS does with no DHCP answer: it gives itself a 169.254 address and announces it by ARP.
    ip -n vm addr add 169.254.77.7/16 dev tap0; ip -n vm link set tap0 up
    ip netns exec vm arping -q -U -c 2 -I tap0 169.254.77.7
    way_in -o "ProxyCommand=socat - UNIX-CONNECT:$W/dial.sock" root@computer
    probe none 169.254.81.16,10.254.0.1
    sleep 1; frames ;;
  relay)  # The state every way leads to: the dead-end card (cardend without -dial) and SSH over the vsock relay.
    $B/cardend -frames $W/frames.sock -counts $W/counts.json > $W/mac.log 2>&1 & MAC=$!; sleep 0.5
    ip netns exec vm $B/fwdgram tap0 $W/frames.sock > $W/fwd.log 2>&1 & FWD=$!; sleep 0.3
    ip -n vm addr add 10.254.0.2/24 dev tap0; ip -n vm link set tap0 up; ip -n vm route add default via 10.254.0.1
    kill $SSHD; SSHD=   # Remote Login off: the relay starts sshd -i for each connection.
    ip netns exec vm python3 -I $H/relay.py $W/vsock.sock $W/sshd_config > $W/relay.log 2>&1 & RELAY=$!
    way_in -o "ProxyCommand=socat - UNIX-CONNECT:$W/vsock.sock" root@computer
    probe none 10.254.0.1
    sleep 1; frames ;;
  serial)  # No network card at all; a shell on the computer's console, the way a Linux main can be reached.
    kill $SSHD; SSHD=
    python3 -I $H/serial.py $W
    probe none none ;;
  esac
  } > $W/result.txt 2>&1
  stop_all; world_down
  cat $W/result.txt; echo
done
