#!/bin/bash
# usage: restart.sh OUT [BOUNDARY]
# Does a computer's budget hold across a restart of the boundary? Two cases, each in a private
# network and mount namespace, with #374's stand-in provider (as run/run.sh):
#   settled   calls until the budget refuses one, the boundary killed (SIGKILL, as a power cut
#             kills it), started again, one more call
#   inflight  one long call, the boundary killed while it streams, started again, one more call
# The budget holds when the call after the restart is refused. The config names a spend journal;
# a boundary that has none ignores the field. BOUNDARY runs another build, such as #374's.
set -u
H=$(cd $(dirname $0); pwd); E=$(cd $H/../../2026-11-24-spend-budgets; pwd); R=$E/run; TLS=$R/tls
OUT=$(realpath -m $1); B=${2:-$E/boundary/zig-out/bin/boundary}
[ -f $TLS/silo-chain.crt ] || bash $R/tls.sh > /dev/null
rm -rf $OUT; mkdir -p $OUT
for c in settled inflight; do
  D=$OUT/$c; mkdir -p $D
  cat > $D/config.json <<CONF
{"policy": "finish", "prices": "$E/prices.json", "output_tps": 2000, "mid_stream": "finish",
 "journal": "$D/spent.journal",
 "computers": [{"name": "script", "address": "127.0.0.7", "credentials": [{"value": "sk-ant-api03-script-fake"}], "limits": {"total": 1}}]}
CONF
  cat > $D/inner.sh <<IN
set -u
python3 -c '
import socket,fcntl,struct
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); i=struct.pack("16sh",b"lo",0)
f=struct.unpack("16sh",fcntl.ioctl(s,0x8913,i))[1]; fcntl.ioctl(s,0x8914,struct.pack("16sh",b"lo",f|1))'
printf 'nameserver 127.0.0.1\n' > $D/.resolv; mount --bind $D/.resolv /etc/resolv.conf
cd $D
echo '{"seed": 1, "tps": 2000, "think": [500, 2000]}' > scenario.json
env -i PATH=/usr/bin:/bin OUT=$D SCENARIO=$D/scenario.json TLS_CERT=$TLS/up-chain.crt TLS_KEY=$TLS/up-leaf.key PORT=8443 python3 $E/fake/provider.py 2> fake.err &
FP=\$!
start() { env -i PATH=/usr/bin:/bin SILO_CONFIG=$D/config.json RECORD=$D/record.jsonl CERT=$TLS/silo-chain.crt KEY=$TLS/silo-leaf.key UPSTREAM_CA=$TLS/up-ca.crt HOLD=0 $B 2>> boundary.err & BP=\$!; sleep 1; }
cl() { env -i PATH=/usr/bin:/bin SSL_CERT_FILE=$TLS/store-with-silo.crt python3 $H/client.py "\$@"; }
start
if [ $c = settled ]; then
  cl until-refused 60 | sed 's/^/{"phase":"before",/;s/,{/,/' > before.jsonl
else
  cl long > before.jsonl & LP=\$!; sleep 2
fi
kill -9 \$BP; wait \$BP 2>/dev/null; [ $c = inflight ] && wait \$LP
start
cl one > after.jsonl
kill \$BP \$FP; wait 2>/dev/null
IN
  unshare -n -m --propagation private bash $D/inner.sh
  after=$(cat $D/after.jsonl)
  st=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["status"])' "$after")
  n=$(grep -c '"status": 200' $D/before.jsonl)
  verdict=$([ "$st" = 200 ] && echo "admitted: the budget was forgotten" || echo "refused: the budget held")
  echo "{\"case\": \"$c\", \"calls_before\": $n, \"after_restart\": $after, \"verdict\": \"$verdict\"}"
done | tee $OUT/restart.jsonl
