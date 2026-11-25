#!/bin/bash
# usage: overshoot.sh <limit $> <model> <seeds...>
# Runs Claude Code with 8 subagents under a budget on its computer, once per policy and seed, the
# policies of one seed at the same time, each run in its own namespace.
R=$(cd $(dirname $0); pwd); E=$(dirname $R); LIMIT=$1; MODEL=$2; shift 2
mkdir -p $E/out/cfg
POLICIES=(
  'finish|"policy": "finish"'
  'finish-time|"policy": "finish", "output_tps": 500'
  'admit-input|"policy": "admit_input", "output_tps": 500'
  'reserve-typical|"policy": "reserve_typical", "output_tps": 500'
  'reserve-max|"policy": "reserve_max"'
  'cut|"policy": "finish", "output_tps": 500, "mid_stream": "cut"'
  'typical-cut|"policy": "reserve_typical", "output_tps": 500, "mid_stream": "cut"'
)
for seed in "$@"; do
  for pol in "${POLICIES[@]}"; do
    name=${pol%%|*}; frag=${pol#*|}; n=over-$MODEL-$name-$seed
    # ONLY="finish-time cut" runs those policies alone.
    [ -n "${ONLY:-}" ] && [[ " $ONLY " != *" $name "* ]] && continue
    python3 - $E/run/scenarios/overshoot.json $seed > $E/out/cfg/$n.scen.json <<'P'
import json,sys; s=json.load(open(sys.argv[1])); s["seed"]=int(sys.argv[2]); print(json.dumps(s))
P
    cat > $E/out/cfg/$n.json <<C
{$frag, "prices": "$E/prices.json",
 "computers": [{"name": "main", "address": "127.0.0.2", "credentials": [{"value": "sk-ant-api03-main-fake"}], "limits": {"total": $LIMIT}}]}
C
    MODEL=$MODEL $R/run.sh $n $E/out/cfg/$n.json $E/out/cfg/$n.scen.json 300 cc > /dev/null 2>&1 &
  done
  wait
done
