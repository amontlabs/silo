#!/bin/bash
# #401's requests through the whole boundary, then what reached the provider against what the
# boundary counted, and the boundary's peak memory. BOUNDARY_BIN runs another build (#395 as
# drafted, to show each case failing first). usage: hostile.sh <out>
R=$(cd $(dirname $0); pwd)
O=$1
BIN=${BIN:-/root/e2e/bin2} GVPROXY_ARGS="-no-gateway-api -no-host-loopback -outbound-deny host -outbound-handoff $O/c2.handoff.sock -dns-upstream 127.0.0.1:5302" \
  SCENARIO=$R/night-scenario.json $R/up.sh $O > /dev/null 2>&1 || { echo "up failed"; exit 1; }
. $O/env
python3 $R/sample.py $O 400 > $O/samples.jsonl &
SP=$!
$R/in.sh $O c1 env CA=$TLS/silo-and-up.crt ANTH=sk-ant-api03-silo-placeholder-c1-0000 OPENAI=sk-proj-silo-placeholder-c1-0000 python3 $R/hostile.py
sleep 2; kill $SP
python3 - $O <<'PY'
import json, sys
O = sys.argv[1]
led = [json.loads(l) for l in open(f"{O}/ledger.jsonl") if '"input"' in l]
tr = [json.loads(l) for l in open(f"{O}/trace.jsonl")]
calls = [t for t in tr if t.get("kind") == "call"]
hosts = [json.loads(l) for l in open(f"{O}/hosts.log")]
s = [json.loads(l) for l in open(f"{O}/samples.jsonl")]
print(json.dumps({"model_calls_served": len(led), "counted": len(calls),
  "served_paths": sorted({h["path"] for h in hosts if h["host"] in ("api.anthropic.com", "api.openai.com")}),
  "hosts_saw": sorted({(h["host"], h["cred"]) for h in hosts}),
  "boundary_peak_mb": max(x["boundary"]["hwm_mb"] for x in s if x.get("boundary")),
  "boundary_alive": s[-1].get("boundary") is not None}))
PY
$R/down.sh $O
