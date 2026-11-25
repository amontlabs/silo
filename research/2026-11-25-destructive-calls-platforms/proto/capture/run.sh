#!/usr/bin/env bash
# Usage: run.sh ca | setup | teardown | serve <cli> [server args...] | stop <cli>
# Same rig as #353's proto/capture/run.sh: a test CA, /etc/hosts pointing each CLI's API hosts at
# its own loopback address, and server2.py answering on 443. Captures go to captures/<cli>.jsonl.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PKI=/opt/clis/pki
ip_of() { case "$1" in railway) echo 127.0.0.5;; vercel) echo 127.0.0.6;; fly) echo 127.0.0.7;; wrangler) echo 127.0.0.8;; supabase) echo 127.0.0.10;; esac; }
case "$1" in
  ca)
    mkdir -p "$PKI"; cd "$PKI"
    grep -v '^#' "$HERE/hostmap.txt" | cut -d' ' -f2- | tr ' ' '\n' | sort -u > hosts.txt
    openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.crt -days 3 -subj "/CN=Silo Capture Test CA" \
      -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
    openssl req -newkey rsa:2048 -nodes -keyout leaf.key -out leaf.csr -subj "/CN=capture.test" 2>/dev/null
    printf "subjectAltName=%s\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n" "$(sed 's/^/DNS:/' hosts.txt | paste -sd,)" > ext.cnf
    openssl x509 -req -in leaf.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out leaf.crt -days 3 -extfile ext.cnf 2>/dev/null
    cat leaf.crt ca.crt > chain.crt; cat /etc/ssl/certs/ca-certificates.crt ca.crt > bundle.crt
    ;;
  setup)
    cp --update=none /etc/hosts /opt/clis/hosts.bak
    { cat /opt/clis/hosts.bak; echo "# silo-capture begin"; grep -v '^#' "$HERE/hostmap.txt"; echo "# silo-capture end"; } > /etc/hosts
    cp "$PKI/ca.crt" /usr/local/share/ca-certificates/silo-capture-test-ca.crt
    update-ca-certificates >/dev/null 2>&1
    ;;
  teardown)
    cp /opt/clis/hosts.bak /etc/hosts
    rm -f /usr/local/share/ca-certificates/silo-capture-test-ca.crt
    update-ca-certificates --fresh >/dev/null 2>&1
    ;;
  serve)
    cli="$2"; shift 2; ip=$(ip_of "$cli")
    pkill -f "server2.py --bind $ip " 2>/dev/null || true; sleep 0.3
    args=(--bind "$ip" --log "$HERE/captures/$cli.jsonl")
    [ -f "$HERE/canned/$cli.json" ] && args+=(--canned "$HERE/canned/$cli.json")
    [ -f "$HERE/canned/$cli-mock.json" ] && args+=(--mock "$HERE/canned/$cli-mock.json")
    nohup python3 "$HERE/server2.py" "${args[@]}" "$@" > /opt/clis/srv-$cli.log 2>&1 &
    sleep 1; cat /opt/clis/srv-$cli.log
    ;;
  stop)
    pkill -f "server2.py --bind $(ip_of "$2") " || true
    ;;
esac
