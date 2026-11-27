#!/bin/sh
# id: K04
# title: What main's network reaches through gvproxy
# needs: K01
# decides: network-stack, network-before-rules, #358 and #363's patch
# question: From main, as root: the internet, the host's 127.0.0.1 (gvproxy maps 192.168.127.254), gvproxy's API at 192.168.127.1:80, the host's LAN?
# outcome sealed: only the internet: the patched gvproxy
# outcome open-to-host: the host's loopback or gvproxy's API answers: stock gvproxy; Silo must ship #363's patch
TEST=K04; . "$(dirname "$0")/../lib.sh"; need_kvm
python3 -m http.server 18080 --bind 127.0.0.1 >/dev/null 2>&1 & srv=$!
vm_new k04; vm_boot k04; vm_wait_ssh k04 300 >/dev/null || { kill $srv; fail "no SSH"; }
r=$(vm_ssh k04 'for u in https://example.com http://192.168.127.254:18080/ http://192.168.127.1/services/forwarder/all; do printf "%s=%s " "$u" "$(curl -s -o /dev/null -m 5 -w %{http_code} $u)"; done')
gw=$(ip route | awk '/default/{print $3; exit}')
lan=$(vm_ssh k04 "curl -s -o /dev/null -m 5 -w %{http_code} http://$gw/ || true")
kill $srv; vm_stop k04 >/dev/null
case $r in *192.168.127.254:18080/=200*|*forwarder/all=200*) o=open-to-host ;; *) o=sealed ;; esac
result K04 $o "$r host-gateway($gw)=$lan"
