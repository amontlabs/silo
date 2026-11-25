mount --bind /tmp/claude-0/gv/resolv.conf /etc/resolv.conf
/tmp/claude-0/gvforwarder -url unix:///tmp/claude-0/gv/net.sock -iface tap0 -stop-if-exist "" -mtu 1500 > /tmp/claude-0/gv/fwd.log 2>&1 &
for i in $(seq 1 30); do ip -4 addr show tap0 2>/dev/null | grep -q inet && break; sleep 0.5; done
echo "guest address: $(ip -4 -o addr show tap0 | awk '{print $4}')"
echo "-- 1. the Mac's 127.0.0.1:7777 through 192.168.127.254:"; curl -s -m 5 http://192.168.127.254:7777/ || echo "(nothing)"
echo "-- 2. gvproxy's API from inside, no token:"; curl -s -m 5 http://192.168.127.1/services/forwarder/all; echo
mkdir -p /tmp/claude-0/gv/guestroot && echo "served from inside the computer" > /tmp/claude-0/gv/guestroot/index.html
(cd /tmp/claude-0/gv/guestroot && python3 -m http.server 8000 --bind 0.0.0.0 > /dev/null 2>&1 &)
sleep 0.5
echo "-- 3. the guest asks gvproxy to open a port on every interface of the Mac:"
curl -s -m 5 -X POST http://192.168.127.1/services/forwarder/expose -d '{"local":"0.0.0.0:2299","remote":"192.168.127.2:8000"}' -w "[%{http_code}]\n"
curl -s -m 5 http://192.168.127.1/services/forwarder/all; echo
sleep 20
