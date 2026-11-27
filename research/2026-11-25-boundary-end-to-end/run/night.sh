#!/bin/bash
# One heavy night, time compressed: up.sh with the platforms and the precaution, then night.py in
# both computers at once, the processes sampled each second, and an optional stall of the recorder.
# usage: night.sh <out> <loops per computer> <seconds> [extra boundary JSON]
# Environment: STALL_AT, STALL_FOR (seconds: SIGSTOP the recorder then), ANSWER, ANSWER_MS (the
# person, as precaution.py), SCENARIO, THINK_MS, DESTRUCTIVE, HOSTILE, BIN, GVPROXY_ARGS.
R=$(cd $(dirname $0); pwd); E=$(dirname $R)
O=$1; L=$2; S=$3; X=${4:-{\}}
BIN=${BIN:-/root/e2e/bin2} GVPROXY_ARGS=${GVPROXY_ARGS:-"-no-gateway-api -no-host-loopback -outbound-deny host -ec2-metadata-access -outbound-handoff $O/c2.handoff.sock -dns-upstream 127.0.0.1:5302"} \
  PLATFORMS=1 PRECAUTION=1 ANSWER=${ANSWER:-yes} ANSWER_MS=${ANSWER_MS:-2000} \
  SCENARIO=${SCENARIO:-$E/run/night-scenario.json} $R/up.sh $O finish "" "$X" > /dev/null 2>&1 || { echo "up failed"; exit 1; }
. $O/env
python3 $R/sample.py $O $((S + 15)) > $O/samples.jsonl &
SP=$!
for c in c1 c2; do
  $R/in.sh $O $c env CA=$TLS/silo-and-up.crt ANTH=sk-ant-api03-silo-placeholder-$c-0000 OPENAI=sk-proj-silo-placeholder-$c-0000 GCP=ya29.silo-placeholder-$c \
    THINK_MS=${THINK_MS:-200,1500} DESTRUCTIVE=${DESTRUCTIVE:-0.01} HOSTILE=${HOSTILE:-0.02} SEED=${SEED:-1} \
    python3 $R/night.py $c $L $S > $O/ops-$c.jsonl 2> $O/night-$c.err &
  echo $! > $O/night-$c.pid
done
T0=$(date +%s.%N); echo "{\"start\": $T0}" > $O/marks.jsonl
if [ -n "${STALL_AT:-}" ]; then
  sleep $STALL_AT; kill -STOP $(cat $O/recorder.pid); echo "{\"stall\": $(date +%s.%N)}" >> $O/marks.jsonl
  sleep $STALL_FOR; kill -CONT $(cat $O/recorder.pid); echo "{\"resume\": $(date +%s.%N)}" >> $O/marks.jsonl
fi
wait $(cat $O/night-c1.pid) $(cat $O/night-c2.pid)
echo "{\"end\": $(date +%s.%N)}" >> $O/marks.jsonl
sleep 3; kill $SP 2>/dev/null
cp $O/config.json $O/config.used.json
$R/down.sh $O
