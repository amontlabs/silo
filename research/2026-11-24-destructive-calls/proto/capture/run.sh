#!/usr/bin/env bash
# Usage: run.sh ca | setup | teardown | serve <cli> [server args...]
# ca: makes the test CA and one leaf certificate for every host in hostmap.txt under /opt/clis/pki.
# setup: points the API hosts at 127.0.0.1 (backs up /etc/hosts) and trusts the test CA.
# serve: starts server.py on <ip of cli in clihosts.txt>:443 logging to captures/<cli>.jsonl, using canned/<cli>.json when present.
# Client env for a run: source env.sh (unsets the proxy, points every CA variable at the test bundle).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PKI=/opt/clis/pki
case "$1" in
  ca)
    mkdir -p "$PKI"; cd "$PKI"
    grep -v '^#' "$HERE/hostmap.txt" | cut -d' ' -f2- | tr ' ' '\n' | sort -u > hosts.txt
    openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.crt -days 3 -subj "/CN=Silo Capture Test CA" \
      -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign"
    openssl req -newkey rsa:2048 -nodes -keyout leaf.key -out leaf.csr -subj "/CN=capture.test"
    printf "subjectAltName=%s\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n" "$(sed 's/^/DNS:/' hosts.txt | paste -sd,)" > ext.cnf
    openssl x509 -req -in leaf.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out leaf.crt -days 3 -extfile ext.cnf
    cat leaf.crt ca.crt > chain.crt; cat /etc/ssl/certs/ca-certificates.crt ca.crt > bundle.crt
    ;;
  setup)
    cp --update=none /etc/hosts /opt/clis/hosts.bak
    { cat /opt/clis/hosts.bak; echo "# silo-capture begin"; grep -v '^#' "$HERE/hostmap.txt"; echo "# silo-capture end"; } > /etc/hosts
    cp "$PKI/ca.crt" /usr/local/share/ca-certificates/silo-capture-test-ca.crt
    update-ca-certificates >/dev/null
    ;;
  teardown)
    cp /opt/clis/hosts.bak /etc/hosts
    rm -f /usr/local/share/ca-certificates/silo-capture-test-ca.crt
    update-ca-certificates --fresh >/dev/null
    ;;
  serve)
    cli="$2"; shift 2
    canned=()
    [ -f "$HERE/canned/$cli.json" ] && canned=(--canned "$HERE/canned/$cli.json")
    ip=$(awk -v c="$cli" '$2==c{print $1}' "$HERE/clihosts.txt")
    exec python3 "$HERE/server.py" --bind "$ip" --log "$HERE/captures/$cli.jsonl" "${canned[@]}" "$@"
    ;;
esac
