#!/bin/bash
# The native Claude Code build and Codex's WebSocket through #378's closed network, on Linux.
#
# Outside: provider.py (TLS on 127.0.0.1:18443, a certificate for the providers' hosts signed by a
# test authority) and proxy.py on a Unix socket, which stands in for vsock.
# Inside: a network namespace with only its loopback, no route and no name server, where
# relay.py carries 127.0.0.1:3128 to that Unix socket and inside.sh runs each case.
#
# Needs: Linux with unprivileged user namespaces, Python 3, OpenSSL, the native Claude Code build
# as CLAUDE (default: `claude` on PATH, checked to be an executable, not a script), and Codex as
# CODEX. Every key is a fake one; nothing leaves the machine.
#   CASES="cc-proxy-node-ca cx-proxy" ./run.sh     runs only those cases (see inside.sh)
#   TOOL="env | grep -ci proxy; curl -s -o /dev/null -w %{http_code} https://api.anthropic.com/v1/models" CASES=cc-tool-command ./run.sh
set -e
H=$(cd "$(dirname "$0")" && pwd); OUT=${OUT:-$H/out}
mkdir -p "$OUT/tls"; rm -f "$OUT/results.tsv" "$OUT/proxy.jsonl" "$OUT/provider.jsonl" "$OUT/ready"
SOCK=$OUT/proxy.sock
CLAUDE=${CLAUDE:-$(readlink -f "$(command -v claude)")}
CODEX=${CODEX:-$(command -v codex)}

T=$OUT/tls
if [ ! -s "$T/chain.crt" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$T/ca.key" -out "$T/ca.crt" -days 30 \
    -subj "/CN=Test authority" -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -keyout "$T/leaf.key" -out "$T/leaf.csr" -subj "/CN=api.anthropic.com" 2>/dev/null
  printf "subjectAltName=DNS:api.anthropic.com,DNS:*.anthropic.com,DNS:api.openai.com,DNS:*.openai.com,DNS:chatgpt.com,DNS:*.chatgpt.com\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nauthorityKeyIdentifier=keyid\nsubjectKeyIdentifier=hash\n" > "$T/ext.cnf"
  openssl x509 -req -in "$T/leaf.csr" -CA "$T/ca.crt" -CAkey "$T/ca.key" -CAcreateserial \
    -out "$T/leaf.crt" -days 30 -extfile "$T/ext.cnf" 2>/dev/null
  cat "$T/leaf.crt" "$T/ca.crt" > "$T/chain.crt"
fi
# The system store with the test authority added, for the cases that trust it through the store.
cat /etc/ssl/certs/ca-certificates.crt "$T/ca.crt" > "$T/store-with-ca.crt"

PORT=18443 TLS_CERT=$T/chain.crt TLS_KEY=$T/leaf.key LOG=$OUT/provider.jsonl WS=${WS:-1} TOOL="${TOOL:-}" \
  python3 "$H/provider.py" & PROV=$!
SOCK=$SOCK LOG=$OUT/proxy.jsonl ALLOW="*" \
  MAP='{"api.anthropic.com:443":"127.0.0.1:18443","api.openai.com:443":"127.0.0.1:18443","chatgpt.com:443":"127.0.0.1:18443"}' \
  python3 "$H/proxy.py" & PROXY=$!
trap 'kill $PROV $PROXY 2>/dev/null || true' EXIT
sleep 1
unshare -rnm env -i PATH="$PATH" TERM=dumb H="$H" CA=$T/ca.crt STORE=$T/store-with-ca.crt SOCK=$SOCK \
  RES=$OUT/results.tsv READY=$OUT/ready OUT="$OUT" CLAUDE="$CLAUDE" CODEX="$CODEX" CASES="${CASES:-}" \
  bash "$H/inside.sh"
cat "$OUT/results.tsv"
