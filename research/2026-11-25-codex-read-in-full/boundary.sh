#!/bin/bash
# Codex 0.172.0 through #385's Zig boundary, which ends TLS for api.openai.com and chatgpt.com and
# counts model calls against budgets. Run as: boundary.sh <out> <case> [codex args...]
#
# Inside a network namespace with only its loopback: provider.py on 127.0.0.1:8443 (the hosts,
# behind a test authority only the boundary trusts), the boundary on :443, and Codex whose
# /etc/hosts points OpenAI's and ChatGPT's hosts at 127.0.0.3, its computer's address. The case
# picks the sign-in and the config (cases below). BOUNDARY is the boundary's binary.
#
# Cases:
#   gpt          ChatGPT sign-in, the computer has a budget of $0.0001 (each call here costs $0.00016, so the second is over it)
#   gpt-nobudget ChatGPT sign-in, no budget
#   key          API key, the same budget
#   gpt-http     as gpt, but the provider refuses the WebSocket, so Codex falls back to plain requests
#   gpt-refresh  ChatGPT sign-in whose access token has expired, so Codex refreshes it first
#   gpt-rotated  ChatGPT sign-in, the config holds the access token from before a refresh: what
#                the boundary sees once Codex has refreshed its sign-in (it does so at least every
#                8 days, and on a 401)
# Every key and token is a fake one; nothing leaves the machine.
set -eu
H=$(cd "$(dirname "$0")" && pwd)
OUT=$(realpath -m "$1"); CASE=$2; shift 2
B=${BOUNDARY:?the boundary binary}
PRICES=${PRICES:?prices.json of #374}
T=$OUT/tls; mkdir -p "$T"
if [ ! -s "$T/silo-chain.crt" ]; then
  SAN="DNS:api.openai.com,DNS:chatgpt.com,DNS:ab.chatgpt.com,DNS:auth.openai.com"
  for who in silo up; do
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $T/$who-ca.key -out $T/$who-ca.crt -days 30 \
      -subj "/CN=$who test authority" -addext "basicConstraints=critical,CA:true" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
    openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $T/$who-leaf.key -out $T/$who-leaf.csr -subj "/CN=providers" 2>/dev/null
    printf "subjectAltName=%s\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:false\n" "$SAN" > $T/ext.cnf
    openssl x509 -req -in $T/$who-leaf.csr -CA $T/$who-ca.crt -CAkey $T/$who-ca.key -CAcreateserial -out $T/$who-leaf.crt -days 30 -extfile $T/ext.cnf 2>/dev/null
    cat $T/$who-leaf.crt $T/$who-ca.crt > $T/$who-chain.crt
  done
  cat /etc/ssl/certs/ca-certificates.crt $T/silo-ca.crt > $T/store-with-silo.crt
fi
C=$OUT/$CASE; rm -rf "$C"; mkdir -p "$C/home/codex" "$C/work" "$C/bodies"
export CODEX_HOME=$C/home/codex
case $CASE in
  gpt*) EXPIRED=$([ "$CASE" = gpt-refresh ] && echo 1 || echo 0) CODEX_HOME=$CODEX_HOME "$H/chatgpt.sh" true
        CRED=$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['tokens']['access_token'])" "$CODEX_HOME/auth.json"); KIND=plan; ENVK="" ;;
  key)  CRED=sk-proj-codex-fake; KIND=key; ENVK="OPENAI_API_KEY=$CRED" ;;
esac
[ "$CASE" = gpt-rotated ] && CRED="$CRED.before-refresh"
ACCT=""; case $CASE in gpt*) ACCT=", \"account\": \"acct-test\"" ;; esac
LIMITS='"limits": {"total": 0.0001},'; case $CASE in gpt-nobudget|gpt-rotated) LIMITS="" ;; esac
cat > "$C/config.json" <<CONF
{"policy": "finish", "prices": "$PRICES",
 "computers": [{"name": "codex", "address": "127.0.0.3", "person": "polar", $LIMITS
                "credentials": [{"value": "$CRED", "kind": "$KIND"$ACCT}]}],
 "people": [{"name": "polar"}]}
CONF
printf '127.0.0.1 localhost\n' > "$C/hosts"
for h in api.openai.com chatgpt.com ab.chatgpt.com auth.openai.com; do echo "127.0.0.3 $h" >> "$C/hosts"; done
echo "$CASE" > "$C/case"
cat > "$C/inner.sh" <<IN
set -u
ip link set lo up 2>/dev/null || python3 -c '
import socket,fcntl,struct
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); i=struct.pack("16sh",b"lo",0)
f=struct.unpack("16sh",fcntl.ioctl(s,0x8913,i))[1]; fcntl.ioctl(s,0x8914,struct.pack("16sh",b"lo",f|1))'
WS=$([ "$CASE" = gpt-http ] && echo 0 || echo 1) PORT=8443 TLS_CERT=$T/up-chain.crt TLS_KEY=$T/up-leaf.key LOG=$C/provider.jsonl BODIES=$C/bodies CASE_FILE=$C/case \
  SCRIPT=$H/night.js python3 $H/provider.py 2> $C/provider.err & PP=\$!
SILO_CONFIG=$C/config.json RECORD=$C/record.jsonl CERT=$T/silo-chain.crt KEY=$T/silo-leaf.key UPSTREAM_CA=$T/up-ca.crt HOLD=0 \
  $B 2> $C/boundary.err & BP=\$!
sleep 1
mount --bind $C/hosts /etc/hosts
cd $C/work
env -i PATH=\$PATH HOME=$C/home CODEX_HOME=$CODEX_HOME TERM=dumb SSL_CERT_FILE=$T/store-with-silo.crt $ENVK \
  timeout ${LIMIT:-90} $CODEX exec --json --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox $* Do_the_task. < /dev/null > $C/codex.out 2>&1
echo "exit=\$?" > $C/result.txt
sleep 1; kill \$PP \$BP
IN
CODEX=${CODEX:-$(command -v codex)} unshare -rnm bash -c "CODEX=\${CODEX} bash $C/inner.sh" 
echo "$CASE $(cat $C/result.txt) files: $(ls $C/work | tr '\n' ' ')"
tail -1 $C/codex.out | cut -c1-200
