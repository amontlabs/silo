#!/bin/bash
# The bench: directly to the stand-ins, then from c1 and from c2 through the boundary.
# usage: bench.sh <out> [n]     (a run started by up.sh). Prints JSON lines.
R=$(cd $(dirname $0); pwd); O=$1; N=${2:-200}; . $O/env
[ -z "${ONLY:-}" ] && CA=$TLS/up-ca.crt ANTH=sk-ant-api03-REAL-anthropic-key-9f8e7d6c GCP=ya29.REAL-google-access-token-aabbccdd python3 $R/bench.py direct $N
for c in ${ONLY:-c1 c2}; do
  $R/in.sh $O $c env CA=$TLS/silo-and-up.crt ANTH=sk-ant-api03-silo-placeholder-$c-0000 GCP=ya29.silo-placeholder-$c python3 $R/bench.py $c $N
done
