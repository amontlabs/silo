#!/bin/bash
# Codex 0.172.0 through #378's closed network on Linux, signed in with a key or with ChatGPT, in
# function mode (gpt-5.5) and code mode (gpt-6.2-sol), against provider.py. Built on #403's run.sh.
#
# Outside: provider.py (TLS on 127.0.0.1:18443 for api.openai.com, chatgpt.com and
# auth.openai.com, signed by a test authority) and #403's proxy.py on a Unix socket (vsock's
# stand-in). Inside: a network namespace with only its loopback; relay.py carries 127.0.0.1:3128
# to the socket, and inside.sh runs each case in a fresh home folder.
#
# Needs Linux with unprivileged user namespaces, Python 3, OpenSSL, and Codex as CODEX.
#   CASES="gpt-code-meta" ./run.sh      runs only that case (see inside.sh)
# Every key and token is a fake one; nothing leaves the machine.
set -e
H=$(cd "$(dirname "$0")" && pwd); OUT=${OUT:-$H/out}
mkdir -p "$OUT/tls" "$OUT/bodies"; rm -f "$OUT/results.tsv" "$OUT/proxy.jsonl" "$OUT/provider.jsonl" "$OUT/ready" "$OUT"/bodies/*
CODEX=${CODEX:-$(command -v codex)}
T=$OUT/tls
if [ ! -s "$T/chain.crt" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$T/ca.key" -out "$T/ca.crt" -days 30 \
    -subj "/CN=Test authority" -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -keyout "$T/leaf.key" -out "$T/leaf.csr" -subj "/CN=api.openai.com" 2>/dev/null
  printf "subjectAltName=DNS:api.openai.com,DNS:*.openai.com,DNS:chatgpt.com,DNS:*.chatgpt.com\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nauthorityKeyIdentifier=keyid\nsubjectKeyIdentifier=hash\n" > "$T/ext.cnf"
  openssl x509 -req -in "$T/leaf.csr" -CA "$T/ca.crt" -CAkey "$T/ca.key" -CAcreateserial \
    -out "$T/leaf.crt" -days 30 -extfile "$T/ext.cnf" 2>/dev/null
  cat "$T/leaf.crt" "$T/ca.crt" > "$T/chain.crt"
fi
cat /etc/ssl/certs/ca-certificates.crt "$T/ca.crt" > "$T/store-with-ca.crt"

echo none > "$OUT/case"
PORT=18443 TLS_CERT=$T/chain.crt TLS_KEY=$T/leaf.key LOG=$OUT/provider.jsonl BODIES=$OUT/bodies CASE_FILE=$OUT/case \
  SCRIPT=$H/night.js python3 "$H/provider.py" & PROV=$!
SOCK=$OUT/proxy.sock LOG=$OUT/proxy.jsonl ALLOW="*" \
  MAP='{"api.openai.com:443":"127.0.0.1:18443","chatgpt.com:443":"127.0.0.1:18443","ab.chatgpt.com:443":"127.0.0.1:18443","auth.openai.com:443":"127.0.0.1:18443"}' \
  python3 "$H/proxy.py" & PROXY=$!
trap 'kill $PROV $PROXY 2>/dev/null || true' EXIT
sleep 1
unshare -rnm env -i PATH="$PATH" TERM=dumb H="$H" STORE=$T/store-with-ca.crt SOCK=$OUT/proxy.sock \
  RES=$OUT/results.tsv READY=$OUT/ready OUT="$OUT" CODEX="$CODEX" CASES="${CASES:-}" bash "$H/inside.sh"
cat "$OUT/results.tsv"
