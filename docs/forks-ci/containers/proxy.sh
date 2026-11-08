#!/usr/bin/env bash
# Starts the host-side HTTP(S) forward proxy on port 3128 for the containers: proxy.sh tinyproxy | squid.
# Both accept only clients in 10.200.0.0/16 (the containers' veth addresses) and CONNECT to port 443 only.
# They run as the calling user, not as root, and the other one is stopped first.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
kind=${1:?tinyproxy or squid}
for pidfile in "$WORK/tinyproxy.pid" "$WORK/squid.pid"; do
  [ -s "$pidfile" ] && kill "$(cat "$pidfile")" 2>/dev/null || true
done
for _ in $(seq 50); do ss -ltn 'sport = :3128' | grep -q LISTEN || break; sleep 0.2; done
case $kind in
  tinyproxy)
    cat > "$WORK/tinyproxy.conf" <<CONF
Port 3128
Listen 0.0.0.0
Timeout 600
LogFile "$WORK/tinyproxy.log"
LogLevel Info
PidFile "$WORK/tinyproxy.pid"
MaxClients 256
Allow 10.200.0.0/16
ConnectPort 443
DisableViaHeader Yes
CONF
    echo "tinyproxy $(tinyproxy -v 2>&1 | head -1)"
    tinyproxy -c "$WORK/tinyproxy.conf" </dev/null >"$WORK/tinyproxy.out" 2>&1
    ;;
  squid)
    cat > "$WORK/squid.conf" <<CONF
http_port 3128
acl containers src 10.200.0.0/16
acl SSL_ports port 443
acl Safe_ports port 80 443
acl CONNECT method CONNECT
http_access deny !Safe_ports
http_access deny CONNECT !SSL_ports
http_access allow containers
http_access deny all
cache deny all
access_log stdio:$WORK/squid-access.log
cache_log $WORK/squid-cache.log
pid_filename $WORK/squid.pid
coredump_dir $WORK
via off
forwarded_for delete
visible_hostname silo-c1
buffered_logs off
CONF
    echo "squid $(squid -v | head -1)"
    squid -N -f "$WORK/squid.conf" </dev/null >"$WORK/squid.out" 2>&1 &
    disown
    ;;
  *) echo "unknown proxy $kind" >&2; exit 2;;
esac
for _ in $(seq 50); do ss -ltn 'sport = :3128' | grep -q LISTEN && break; sleep 0.2; done
echo "proxy listening: $(ss -ltn 'sport = :3128' | tail -n +2)"
