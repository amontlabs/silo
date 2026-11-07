#!/bin/bash
# Network path: only the host's proxy is reachable.
t() { local label=$1; shift; local out rc; out=$("$@" 2>&1); rc=$?; echo "NET $label rc=$rc $(echo "$out" | tr '\n' ' ' | cut -c1-200)"; }
t proxy-https curl -sS -m 30 -o /dev/null -w 'http=%{http_code}' https://example.com
t proxy-http curl -sS -m 30 -o /dev/null -w 'http=%{http_code}' http://example.com
t direct-https curl -sS -m 8 --noproxy '*' -o /dev/null -w 'http=%{http_code}' https://example.com
t direct-ip curl -sS -m 8 --noproxy '*' -o /dev/null -w 'http=%{http_code}' http://1.1.1.1/
t direct-dns getent hosts example.com
t host-canary-8000 curl -sS -m 8 --noproxy '*' -o /dev/null -w 'http=%{http_code}' "http://$C1_HOSTIP:8000/"
t host-proxy-direct-port curl -sS -m 8 --noproxy '*' -o /dev/null -w 'http=%{http_code}' "http://$C1_HOSTIP:3128/"
t host-ssh nc -zv -w 3 "$C1_HOSTIP" 22
t other-container-subnet curl -sS -m 5 --noproxy '*' -o /dev/null -w 'http=%{http_code}' http://10.200.99.2:3128/
python3 -m http.server 8080 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
t loopback-server curl -sS -m 5 --noproxy '*' -o /dev/null -w 'http=%{http_code}' http://127.0.0.1:8080/
kill %1 2>/dev/null
echo "NET_DONE"
