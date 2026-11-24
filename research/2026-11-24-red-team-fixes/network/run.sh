#!/bin/bash
# Three gvproxy builds, one after the other, each with a guest that attacks it from inside.
#   stock   gvproxy as built, with its defaults
#   token   the same, with --api-token-file (the token gvproxy offers)
#   sealed  gvproxy with gvproxy-sealed.patch, started with -sealed
# usage: run.sh <gv-stock> <gv-sealed> <gvforwarder> <workdir>
H=$(cd $(dirname $0); pwd); STOCK=$1; SEALED=$2; FWD=$3; W=$4
PUB=$(hostname -I | awk '{print $1}')
# The internet: a network namespace at 203.0.113.10 (TEST-NET-3), routed by the host. It is neither
# the host nor a private network, so a sealed gvproxy must still reach it.
ip netns add inet 2>/dev/null; ip link add vinet type veth peer name e0 netns inet 2>/dev/null
ip addr add 203.0.113.1/24 dev vinet 2>/dev/null; ip link set vinet up; ip -n inet addr add 203.0.113.10/24 dev e0 2>/dev/null; ip -n inet link set e0 up; ip -n inet link set lo up
ip -n inet route add default via 203.0.113.1 2>/dev/null
mkdir -p $W/mac $W/net; echo "secret page served on the Mac's 127.0.0.1" > $W/mac/index.html; echo "a host on the internet" > $W/net/index.html
(cd $W/mac && exec python3 -m http.server 7777 --bind 127.0.0.1 > /dev/null 2>&1) & M=$!
(cd $W/net && exec ip netns exec inet python3 -m http.server 8088 --bind 203.0.113.10 > /dev/null 2>&1) & N=$!
mkdir -p $W/lan; echo "a service of the Mac, on all its interfaces" > $W/lan/index.html
(cd $W/lan && exec python3 -m http.server 7778 --bind 0.0.0.0 > /dev/null 2>&1) & L=$!
for v in stock token sealed; do
  D=$W/$v; rm -rf $D; mkdir -p $D; cp $FWD $D/gvforwarder
  B=$STOCK; ARGS=""
  [ $v = token ] && { head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' > $D/token; chmod 600 $D/token; ARGS="-api-token-file $D/token"; }
  [ $v = sealed ] && { B=$SEALED; ARGS="-sealed"; }
  $B -listen unix://$D/net.sock -mtu 1500 $ARGS > $D/gvproxy.log 2>&1 & G=$!
  sleep 1
  echo "== $v"
  MACLAN=$PUB:7778 INTERNET=203.0.113.10:8088 unshare -n -m bash $H/guest.sh $D
  echo "-- on the host, outside the computer"
  LS=$(ss -ltnp | grep "pid=$G," | awk '{print $4}' | sort | tr '\n' ' '); echo "   gvproxy listens on: ${LS:-nothing}"
  echo "   the host's public address, port 2299: $(curl -s -m 2 http://$PUB:2299/ || echo '(nothing)')"
  echo "   Silo opens a port the right way, through the API on its own socket: $(curl -s -m 3 --unix-socket $D/net.sock -o /dev/null -w '[%{http_code}]' -X POST http://x/services/forwarder/expose -d '{"local":"127.0.0.1:3300","remote":"192.168.127.2:8000"}')"
  echo "   and the Mac reaches it: $(curl -s -m 2 http://127.0.0.1:3300/ || echo '(nothing)')"
  kill $G; wait $G 2>/dev/null; pkill -f "$D/gvforwarder" 2>/dev/null; pkill -f "http.server 8000" 2>/dev/null
  echo
done
kill $M $N $L; pkill -f "http.server 8088"; ip netns del inet; ip link del vinet 2>/dev/null
