#!/bin/bash
# usage: run.sh <out> <config.json> <scenario.json> <timeout s> <program...>
# Runs the stand-in providers, the boundary and the programs (in parallel) inside a private network
# and mount namespace: their DNS, their 443 and their hosts file are the experiment's. Nothing
# outside the namespace changes: no global trust store, environment or firewall.
set -u
R=$(cd $(dirname $0); pwd); E=$(dirname $R); TLS=$R/tls
OUT=$E/out/$1; CONF=$(realpath $2); SCEN=$(realpath $3); TO=$4; shift 4
rm -rf $OUT; mkdir -p $OUT
. $R/progs.sh
cp $CONF $OUT/config.json; cp $SCEN $OUT/scenario.json
cat > $OUT/.inner.sh <<IN
set -u
python3 -c '
import socket,fcntl,struct
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); i=struct.pack("16sh",b"lo",0)
f=struct.unpack("16sh",fcntl.ioctl(s,0x8913,i))[1]; fcntl.ioctl(s,0x8914,struct.pack("16sh",b"lo",f|1))'
printf 'nameserver 127.0.0.1\n' > $OUT/.resolv; mount --bind $OUT/.resolv /etc/resolv.conf
cd $OUT
env -i PATH=/usr/bin:/bin ${DUMP:+DUMP=1} OUT=$OUT SCENARIO=$SCEN TLS_CERT=$TLS/up-chain.crt TLS_KEY=$TLS/up-leaf.key PORT=8443 python3 $E/fake/provider.py 2> $OUT/fake.err &
FP=\$!
env -i PATH=/usr/bin:/bin SILO_CONFIG=$CONF RECORD=$OUT/record.jsonl CERT=$TLS/silo-chain.crt KEY=$TLS/silo-leaf.key UPSTREAM_CA=$TLS/up-ca.crt HOLD=0 \${SILO_TODAY:+SILO_TODAY=\$SILO_TODAY} $E/boundary/zig-out/bin/boundary 2> $OUT/boundary.err &
BP=\$!
sleep 1; kill -0 \$BP || { echo boundary did not start; cat $OUT/boundary.err; exit 1; }
IN
echo 'PIDS=""' >> $OUT/.inner.sh
for p in "$@"; do
  v=P_$p; spec=${!v}; addr=${spec%%|*}; rest=${spec#*|}; envs=${rest%%|*}; cmd=${rest#*|}; cmd=${cmd//Do_the_task./\"\$TASK\"}
  W=$OUT/$p; mkdir -p $W/home $W/work
  python3 -c "import sys;sys.stdout.write(('lorem ipsum dolor sit amet ' * 40000)[:${BIG:-400000}])" > $W/work/big.txt
  (cd $W/work && git init -q)
  printf '127.0.0.1 localhost\n' > $W/hosts
  for h in api.anthropic.com api.openai.com chatgpt.com generativelanguage.googleapis.com api.mistral.ai api.deepseek.com; do echo "$addr $h" >> $W/hosts; done
  cat > $W/run.sh <<PR
mount --bind $W/hosts /etc/hosts
cd $W/work
START=\$(date +%s.%N)
export TASK="${TASK:-Do the task.}"
env -i PATH=/opt/codex/bin:/opt/gemini/bin:/opt/node22/bin:/usr/local/bin:/usr/bin:/bin HOME=$W/home TERM=dumb USER=root TASK="\$TASK" $envs timeout $TO $cmd < /dev/null > $W/out.txt 2> $W/err.txt
echo "exit=\$? secs=\$(python3 -c "import time;print(round(time.time()-\$START,1))")" > $W/result.txt
PR
  echo "unshare -m --propagation private bash $W/run.sh & PIDS=\"\$PIDS \$!\"" >> $OUT/.inner.sh
done
cat >> $OUT/.inner.sh <<IN
wait \$PIDS
sleep 0.5; kill \$BP \$FP; wait 2>/dev/null
IN
unshare -n -m --propagation private bash $OUT/.inner.sh
for p in "$@"; do echo "$p: $(cat $OUT/$p/result.txt 2>/dev/null)"; done
