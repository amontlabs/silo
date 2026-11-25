#!/usr/bin/env bash
# Usage: run.sh ca | setup | teardown | serve <cli> | stop <cli>
# #383's rig (../../../2026-11-25-destructive-calls-platforms/proto/capture/run.sh): a test CA,
# /etc/hosts pointing each CLI's API hosts at its own loopback address, and server3.py on 443.
# Captures go to captures/<cli>.jsonl. Nothing here reaches a real provider.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PKI=/opt/clis/pki
ip_of() { grep -v '^#' "$HERE/hostmap.txt" | awk -v n="$1" 'BEGIN{split("aws gcloud az gh kubectl",c," ")} {i++; if (c[i]==n) print $1}'; }
case "$1" in
  ca)
    mkdir -p "$PKI"; cd "$PKI"
    { grep -v '^#' "$HERE/hostmap.txt" | cut -d' ' -f2- | tr ' ' '\n'; echo '*.amazonaws.com'; echo '*.us-east-1.amazonaws.com';
      echo '*.s3.amazonaws.com'; echo '*.s3.us-east-1.amazonaws.com'; echo '*.googleapis.com'; echo '*.blob.core.windows.net'; echo '*.vault.azure.net'; } | sort -u > hosts.txt
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
    mkdir -p /opt/clis/empty
    ;;
  teardown)
    cp /opt/clis/hosts.bak /etc/hosts
    ;;
  serve)
    cli="$2"; ip=$(ip_of "$cli")
    pkill -f "server3.py --bind $ip " 2>/dev/null || true; sleep 0.3
    mkdir -p "$HERE/captures" "$HERE/outputs"
    nohup python3 -B "$HERE/server3.py" --bind "$ip" --log "$HERE/captures/$cli.jsonl" --cli "$cli" > /opt/clis/srv-$cli.log 2>&1 &
    sleep 1; cat /opt/clis/srv-$cli.log
    ;;
  stop)
    pkill -f "server3.py --bind $(ip_of "$2") " || true
    ;;
esac
