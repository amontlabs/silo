#!/bin/bash
# usage: suite.sh <results dir> <program>:<version>[,<version>...] ...
# Runs the contract on each release in the order given, each one against the one before it as its
# baseline, then the newest also with the trust store only and with a refusal. Keeps the reports,
# fingerprints and differences in <results dir>; the full captures stay in $RUNS (default: a
# temporary folder), as they hold the programs' prompts.
set -u
C=$(cd $(dirname $0); pwd); R=$(realpath -m $1); shift; RUNS=${RUNS:-$(mktemp -d)}; mkdir -p $R
keep() { local o=$RUNS/$1 d=$R/$1; mkdir -p $d; cp $o/contract.json $o/fingerprint.json $o/result.txt $o/lookups.txt $d/ 2>/dev/null; cp $o/shape-diff.json $d/ 2>/dev/null; true; }
for spec in "$@"; do
  P=${spec%%:*}; prev=""
  for V in ${spec#*:}; do V=${V//,/ }; for v in $V; do
    n=$P-$v-task-env; $C/run.sh $P $v task env $RUNS/$n > /dev/null 2>&1
    [ -n "$prev" ] && python3 -I $C/check.py $RUNS/$n $RUNS/$prev/fingerprint.json > $RUNS/$n/contract.json 2> /dev/null
    keep $n; echo "$n: $(python3 -I -c "import json;r=json.load(open('$RUNS/$n/contract.json'));print('PASS' if r['passed'] else 'FAIL', [c['id'] for c in r['checks'] if c['result']=='fail'])")"
    prev=$n
  done; done
  v=${prev#$P-}; v=${v%-task-env}
  for x in "task store" "refuse env"; do set -- $x; n=$P-$v-$1-$2; $C/run.sh $P $v $1 $2 $RUNS/$n > /dev/null 2>&1; keep $n
    echo "$n: $(python3 -I -c "import json;r=json.load(open('$RUNS/$n/contract.json'));print('PASS' if r['passed'] else 'FAIL', [c['id'] for c in r['checks'] if c['result']=='fail'])")"; done
done
echo "captures in $RUNS"
