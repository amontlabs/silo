#!/bin/bash
# Inside the computer: a network namespace whose only interface is gvforwarder's tap device, so it
# sees what a virtual machine behind gvproxy sees. Run by run.sh as: unshare -n -m bash guest.sh <dir>
D=$1; H=$(cd $(dirname $0); pwd)
echo "nameserver 192.168.127.1" > $D/resolv.conf; mount --bind $D/resolv.conf /etc/resolv.conf
$D/gvforwarder -url unix://$D/net.sock -iface tap0 -stop-if-exist "" -mtu 1500 > $D/fwd.log 2>&1 &
for i in $(seq 1 40); do ip -4 addr show tap0 2>/dev/null | grep -q inet && break; sleep 0.25; done
echo "   guest address: $(ip -4 -o addr show tap0 | awk '{print $4}')"
mkdir -p $D/www && echo "served from inside the computer" > $D/www/index.html
(cd $D/www && exec python3 -m http.server 8000 --bind 0.0.0.0 > /dev/null 2>&1) &
sleep 0.5
echo "-- every TCP port of the gateway and of the host-loopback address"
python3 -I $H/scan.py 192.168.127.1 192.168.127.254
echo "-- the Mac's own 127.0.0.1:7777, through 192.168.127.254"
echo "   $(curl -s -m 3 http://192.168.127.254:7777/ || echo '(nothing)')"
echo "-- gvproxy's API: list, then open a port on every interface of the Mac (no token, then a guessed one)"
echo "   list: $(curl -s -m 3 -w ' [%{http_code}]' http://192.168.127.1/services/forwarder/all || echo '(nothing)')"
echo "   expose: $(curl -s -m 3 -o /dev/null -w '[%{http_code}]' -X POST http://192.168.127.1/services/forwarder/expose -d '{"local":"0.0.0.0:2299","remote":"192.168.127.2:8000"}' || echo '(nothing)')"
echo "   expose with a guessed token: $(curl -s -m 3 -o /dev/null -w '[%{http_code}]' -H 'Authorization: Bearer 0000000000000000000000000000000000000000000000000000000000000000' -X POST http://192.168.127.1/services/forwarder/expose -d '{"local":"0.0.0.0:2298","remote":"192.168.127.2:8000"}' || echo '(nothing)')"
echo "-- a service the Mac runs on every interface, reached at the Mac's own network address ($MACLAN)"
echo "   $(curl -s -m 3 http://$MACLAN/ || echo '(nothing)')"
echo "-- what the computer still needs: a host on the internet ($INTERNET), and a name lookup"
echo "   $(curl -s -m 3 http://$INTERNET/ || echo '(nothing)')"
echo "   host.containers.internal, asked of gvproxy's DNS: $(python3 -c 'import socket;print(socket.gethostbyname("host.containers.internal"))' 2>&1 | tail -1)"
sleep 1
