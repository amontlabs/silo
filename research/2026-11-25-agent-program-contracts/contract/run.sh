#!/bin/bash
# usage: run.sh <program> <version> <scenario> <trust> <out dir>
#   program   claude-code | claude-code-plan | codex | codex-openai-key | gemini-cli
#   scenario  task:   the stand-in model asks for one shell tool call, then ends
#             refuse: a budget too small for one call; the boundary refuses in the provider's shape
#   trust     env:    Silo's authority reaches the program through the setting its recipe writes
#             store:  only through the computer's trust store, with no setting in the program
#
# One run of one release inside a private network and mount namespace: a name server that points
# every name at the boundary and writes each lookup, #374's boundary on 443, the stand-in providers
# behind it on 8443. Nothing outside the namespace changes.
set -u
C=$(cd $(dirname $0); pwd); B=$C/.build; TLS=$B/tls
PROG=$1; VER=$2; SCEN=$3; TRUST=$4; OUT=$(realpath -m $5)
BOUNDARY=$B/research/2026-11-24-spend-budgets/boundary/zig-out/bin/boundary
PRICES=$B/research/2026-11-24-spend-budgets/prices.json
rm -rf $OUT; mkdir -p $OUT/home $OUT/work
BASE=${PROG%-plan}; BASE=${BASE%-openai-key}
BIN=$($C/install.sh $BASE $VER) || { echo "install failed" > $OUT/result.txt; exit 1; }
TASK="Run one shell command that writes contract-proof.txt, then say you are done."
NODE=$(dirname $(command -v node))

case $PROG in
  claude-code)      KEY=sk-ant-api03-contract-fake; CENV="ANTHROPIC_API_KEY=$KEY IS_SANDBOX=1 DISABLE_AUTOUPDATER=1"
                    CMD="$BIN -p \"\$TASK\" --dangerously-skip-permissions --output-format stream-json --verbose"; TRUSTENV="NODE_EXTRA_CA_CERTS=$TLS/silo-ca.crt" ;;
  claude-code-plan) KEY=sk-ant-oat01-contract-fake; CENV="CLAUDE_CODE_OAUTH_TOKEN=$KEY IS_SANDBOX=1 DISABLE_AUTOUPDATER=1"
                    CMD="$BIN -p \"\$TASK\" --dangerously-skip-permissions --output-format stream-json --verbose"; TRUSTENV="NODE_EXTRA_CA_CERTS=$TLS/silo-ca.crt" ;;
  codex)            KEY=sk-proj-contract-fake; CENV="OPENAI_API_KEY=$KEY CODEX_API_KEY=$KEY CODEX_HOME=$OUT/home/.codex"
                    CMD="$BIN exec --json --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check \"\$TASK\""; TRUSTENV="CODEX_CA_CERTIFICATE=$TLS/silo-ca.crt SSL_CERT_FILE=$TLS/store-with-silo.crt"
                    mkdir -p $OUT/home/.codex ;;
  codex-openai-key) KEY=sk-proj-contract-fake; CENV="OPENAI_API_KEY=$KEY CODEX_HOME=$OUT/home/.codex"
                    CMD="$BIN exec --json --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check \"\$TASK\""; TRUSTENV="CODEX_CA_CERTIFICATE=$TLS/silo-ca.crt SSL_CERT_FILE=$TLS/store-with-silo.crt"
                    mkdir -p $OUT/home/.codex ;;
  gemini-cli)       KEY=AIzaContractFakeKey0000000000000000000; CENV="GEMINI_API_KEY=$KEY GEMINI_CLI_TRUST_WORKSPACE=true"
                    CMD="$NODE/node $BIN -p \"\$TASK\" --yolo"; TRUSTENV="NODE_EXTRA_CA_CERTS=$TLS/silo-ca.crt" ;;
esac
[ $TRUST = store ] && TRUSTENV=""
[ $PROG = gemini-cli ] && [ $TRUST = store ] && TRUSTENV="NODE_USE_SYSTEM_CA=1"

LIMITS=""; POLICY=finish
[ $SCEN = refuse ] && { LIMITS=', "limits": {"total": 0.000001}'; POLICY=admit_input; }
cat > $OUT/boundary.json <<J
{"policy": "$POLICY", "prices": "$PRICES",
 "computers": [{"name": "c", "address": "127.0.0.2", "person": "p", "credentials": [{"value": "$KEY"}]$LIMITS}],
 "people": [{"name": "p"}]}
J
(cd $OUT/work && git init -q)
printf '127.0.0.1 localhost\n' > $OUT/hosts
printf 'nameserver 127.0.0.1\n' > $OUT/resolv.conf
echo "$PROG $VER $SCEN $TRUST" > $OUT/what.txt

cat > $OUT/inner.sh <<IN
set -u
python3 -I -c '
import socket,fcntl,struct
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); i=struct.pack("16sh",b"lo",0)
f=struct.unpack("16sh",fcntl.ioctl(s,0x8913,i))[1]; fcntl.ioctl(s,0x8914,struct.pack("16sh",b"lo",f|1))'
mount --bind $OUT/resolv.conf /etc/resolv.conf
mount --bind $OUT/hosts /etc/hosts
env -i PATH=/usr/bin:/bin OUT=$OUT ANSWER=127.0.0.2 python3 -I $C/dns.py 2> $OUT/dns.err & DP=\$!
env -i PATH=/usr/bin:/bin OUT=$OUT TLS_CERT=$TLS/up-chain.crt TLS_KEY=$TLS/up-leaf.key PORT=8443 WS=\${WS:-1} python3 -I $C/standin.py 2> $OUT/standin.err & SP=\$!
env -i PATH=/usr/bin:/bin SILO_CONFIG=$OUT/boundary.json RECORD=$OUT/record.jsonl CERT=$TLS/silo-chain.crt KEY=$TLS/silo-leaf.key UPSTREAM_CA=$TLS/up-ca.crt HOLD=0 $BOUNDARY 2> $OUT/boundary.err & BP=\$!
sleep 1
kill -0 \$BP || { echo "boundary did not start" > $OUT/result.txt; exit 1; }
cd $OUT/work
[ $TRUST = store ] && mount --bind $TLS/store-with-silo.crt /etc/ssl/certs/ca-certificates.crt
START=\$(date +%s.%N)
export TASK="$TASK"
env -i PATH=$NODE:/usr/local/bin:/usr/bin:/bin HOME=$OUT/home TERM=dumb USER=root TASK="\$TASK" $TRUSTENV $CENV timeout ${TIMEOUT:-120} $CMD < /dev/null > $OUT/out.txt 2> $OUT/err.txt
echo "exit=\$? secs=\$(python3 -I -c "import time;print(round(time.time()-\$START,1))")" > $OUT/result.txt
sleep 0.5; kill \$BP \$SP \$DP; wait 2>/dev/null
IN
unshare -n -m --propagation private bash $OUT/inner.sh
python3 -I $C/check.py $OUT > $OUT/contract.json 2> $OUT/check.err
python3 -I $C/check.py --text $OUT
