#!/bin/bash
# #374's boundary tells a computer by the address its connection arrives on. A connection that
# arrives on an address no computer has gets no computer, and #374 then skips both of its checks:
# the key the computer was given, and the budget. This runs three calls, in a private network
# namespace, against #374's boundary as written and with unknown-computer.patch:
#   main, a key it was not given             refused by both
#   main, its own key, a second call over its budget   refused by both
#   127.0.0.9, no computer, any key          #374: reaches the provider; patched: refused
# usage: test.sh before|after     (as root, after #374's run/tls.sh)
set -u
H=$(cd $(dirname $0); pwd); S=$H/../../2026-11-24-spend-budgets; TLS=$S/run/tls
W=$(mktemp -d); cp -r $S/boundary $W/; rm -rf "${W:?}"/boundary/.zig-cache "${W:?}"/boundary/zig-out
[ "$1" = after ] && (cd $W && patch -s -p1 < $H/unknown-computer.patch)
(cd $W/boundary && zig build -Doptimize=ReleaseSafe) || exit 2
sed "s#PRICES#$S/prices.json#" $H/config.json > $W/config.json
cat > $W/inner.sh <<IN
ip link set lo up
cd $W
OUT=$W SCENARIO=$S/run/scenarios/short.json TLS_CERT=$TLS/up-chain.crt TLS_KEY=$TLS/up-leaf.key PORT=8443 python3 -I $S/fake/provider.py 2> $W/fake.err &
SILO_CONFIG=$W/config.json RECORD=$W/record.jsonl CERT=$TLS/silo-chain.crt KEY=$TLS/silo-leaf.key UPSTREAM_CA=$TLS/up-ca.crt HOLD=0 $W/boundary/zig-out/bin/boundary 2> $W/boundary.err &
sleep 1
call() { # from, key
  curl -s -m 10 -o /dev/null -w '%{http_code}' --cacert $TLS/silo-ca.crt --resolve api.anthropic.com:443:\$1 \
    https://api.anthropic.com/v1/messages -H "x-api-key: \$2" -H 'anthropic-version: 2023-06-01' -H 'content-type: application/json' \
    -d '{"model":"claude-opus-5-5","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'; }
a=\$(call 127.0.0.2 sk-ant-api03-pasted-real-key); call 127.0.0.2 sk-ant-api03-main-fake > /dev/null; b=\$(call 127.0.0.2 sk-ant-api03-main-fake); c=\$(call 127.0.0.9 sk-ant-api03-pasted-real-key)
echo "main, a key it was not given:   \$a \$([ \$a = 400 ] && echo PASS || echo FAIL)"
echo "main, over its budget, again:   \$b \$([ \$b = 400 ] && echo PASS || echo FAIL)"
echo "no computer (127.0.0.9), a key: \$c \$([ \$c = 400 ] || [ \$c = 000 ] && echo PASS || echo FAIL)"
kill %1 %2; wait 2>/dev/null
IN
unshare -n bash $W/inner.sh | tee $W/out.txt
echo "provider saw: $(grep -c . $W/ledger.jsonl 2>/dev/null || echo 0) calls"
! grep -q FAIL $W/out.txt; rc=$?; rm -rf "${W:?}"; exit $rc
