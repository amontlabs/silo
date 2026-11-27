#!/bin/bash
# program -> boundary spike (443, test authority in the system store) -> fake provider (8443)
# usage: e2e2.sh <prog> <scenario.json> <out> ; env DENY (substring of a call input to deny)
S=${SILO_EXP:?set SILO_EXP to the folder that holds w/, npm/ and dl/}; W=$S/w; P=$1; SC=$2; O=$3
rm -rf $O; mkdir -p $O/home; rm -rf /work/site; mkdir -p /work/site; echo "old line" > /work/site/existing.txt
PATHS=$S/npm/bin:$S/dl/goose:$S/dl/cursor/dist-package:/root/.local/bin:/opt/node22/bin:/usr/local/bin:/usr/bin:/bin
line=$(grep "^$P|" $W/e2e_progs.sh); envs=$(echo "$line" | cut -d'|' -f2); cmd=$(echo "$line" | cut -d'|' -f3-)
PORT=8443 TLS_CERT=$W/tls/up-chain.crt TLS_KEY=$W/tls/up-leaf.key OUT=$O SCENARIO=$SC python3 $W/fake_multi.py 2>$O/fake.err & SP=$!
mkfifo $O/ctl $O/ack; exec 9<>$O/ctl; exec 8<>$O/ack
for a in $O/home/ /dev/ /tmp/ /proc/; do echo "allow $a"; done > $O/ctl
LISTEN_PORT=443 RECORD=$O/record.jsonl CERT=$W/tls/chain.crt KEY=$W/tls/leaf.key UPSTREAM_CA=$W/tls/up-ca.crt DENY_SUBSTR="$DENY" PROBE_CTL=$O/ctl PROBE_ACK=$O/ack $W/boundary 2> $O/boundary.err & BP=$!
sleep 0.8
cd /work/site; git init -q 2>/dev/null; [ -n "$SETUP" ] && HOME=$O/home bash -c "$SETUP"
$W/nsrun env -i PATH=$PATHS HOME=$O/home TERM=dumb SPROBE_ACK=$O/ack $envs timeout ${TO:-90} $W/exp/sc/sprobe $O/ctl $O/probe.txt -- bash -c "$cmd" < /dev/null > $O/out.txt 2>&1; echo "exit=$?" >> $O/out.txt
kill $SP $BP 2>/dev/null; ls -la /work/site > $O/site.txt; cat /work/site/existing.txt >> $O/site.txt 2>/dev/null
