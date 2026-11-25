#!/bin/bash
# A closed network with no network stack, tried on Linux.
#
# Outside: #374's stand-in provider (TLS on 127.0.0.1:18443) and proxy.py on a Unix socket.
# Inside: a network namespace with only its loopback, no route and no name server, where
# relay.py carries 127.0.0.1:3128 to that Unix socket (the stand-in for vsock) and inside.sh
# runs each case once without the proxy settings and once with them.
#
# Needs: Linux with unprivileged user namespaces, Python 3, OpenSSL, curl, git, npm, pip, Node
# and Claude Code on PATH. The stand-in provider is #374's: pass its folder as FAKE, for example
#   git show origin/claude/project-thread-cs43ax:research/2026-11-24-spend-budgets/fake/provider.py > fake/provider.py
# Every key is a fake one; nothing leaves the machine.
set -e
H=$(cd "$(dirname "$0")" && pwd); OUT=${OUT:-$H/out}; FAKE=${FAKE:-$H/fake}
mkdir -p "$OUT" "$OUT/tls"; rm -f "$OUT/results.tsv" "$OUT/proxy.jsonl" "$OUT/ready"
SOCK=$OUT/proxy.sock

# A test authority, and a certificate for the provider's host signed by it.
T=$OUT/tls
if [ ! -s "$T/chain.crt" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$T/ca.key" -out "$T/ca.crt" -days 30 \
    -subj "/CN=Test authority" -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -keyout "$T/leaf.key" -out "$T/leaf.csr" -subj "/CN=api.anthropic.com" 2>/dev/null
  printf "subjectAltName=DNS:api.anthropic.com,DNS:*.anthropic.com\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nauthorityKeyIdentifier=keyid\nsubjectKeyIdentifier=hash\n" > "$T/ext.cnf"
  openssl x509 -req -in "$T/leaf.csr" -CA "$T/ca.crt" -CAkey "$T/ca.key" -CAcreateserial \
    -out "$T/leaf.crt" -days 30 -extfile "$T/ext.cnf" 2>/dev/null
  cat "$T/leaf.crt" "$T/ca.crt" > "$T/chain.crt"
fi
[ -s "$FAKE/short.json" ] || echo '{"seed": 1, "main_steps": 3, "subagents": 2, "spawn_at": 1, "sub_steps": 2, "think": [200, 800], "tps": 2000}' > "$FAKE/short.json"

PORT=18443 TLS_CERT=$T/chain.crt TLS_KEY=$T/leaf.key OUT=$OUT/fake SCENARIO=$FAKE/short.json \
  python3 "$FAKE/provider.py" > "$OUT/fake.out" 2>&1 & FAKEPID=$!
SOCK=$SOCK LOG=$OUT/proxy.jsonl \
  ALLOW="api.anthropic.com,github.com,registry.npmjs.org,pypi.org,files.pythonhosted.org" \
  MAP='{"api.anthropic.com:443":"127.0.0.1:18443"}' \
  python3 "$H/proxy.py" & PROXY=$!
sleep 1
unshare -rnm env -i PATH="$PATH" TERM=dumb CA=$T/ca.crt SOCK=$SOCK RES=$OUT/results.tsv READY=$OUT/ready \
  bash "$H/inside.sh"
kill $FAKEPID $PROXY 2>/dev/null || true
cat "$OUT/results.tsv"; echo; cat "$OUT/proxy.jsonl"
