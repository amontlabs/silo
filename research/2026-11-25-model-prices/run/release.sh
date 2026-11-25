#!/bin/bash
# usage: release.sh
# Two runs through #374's boundary with the stand-in providers (research/2026-11-24-spend-budgets):
#   prices_release  the table is replaced while calls go on: by hand, by a signed release, by an
#                   older signed release; and one call asks for US-only inference.
#   unknown_bound   a model with no row, under a $1 budget, with unknown_model "bound".
# Then checks the boundary's record (check.py). Needs the boundary built and run/tls.sh run there.
set -eu
H=$(cd $(dirname $0); pwd); E=$(cd $H/../../2026-11-24-spend-budgets; pwd)
W=$E/out/prices-work; rm -rf $W; mkdir -p $W
KEY=$(python3 $H/../update/sign.py keygen $W test-2026)
python3 - "$E/prices.json" "$W" <<'PY'
import json, sys
base, w = json.load(open(sys.argv[1])), sys.argv[2]
mult = [{"provider": "anthropic", "models": ["claude-opus-5-5*", "claude-sonnet-5-5*", "claude-fable-5-2*", "claude-haiku-5-5*"], "when": "inference_geo=us", "factor": 1.1}]
v1 = {"format": 2, "version": 2026112501, "published": "2026-11-25", "check_by": "2026-12-09", "rows": base["rows"], "multipliers": mult}
new = {"provider": "anthropic", "model": "claude-opus-6-0", "input": 6, "output": 30, "cache_read": 0.3, "cache_write_short": 7.5, "cache_write_long": 12}
v2 = dict(v1, version=2026112601, rows=base["rows"] + [new])
tampered = json.loads(json.dumps(v1)); tampered["version"] = 2026119999
for r in tampered["rows"]:
    if r["model"] == "claude-opus-5-5" and r.get("tier", "standard") == "standard": r["input"], r["output"] = 0.04, 0.2
for n, v in [("v1.json", v1), ("v2.json", v2), ("tampered.json", tampered)]: json.dump(v, open(f"{w}/{n}", "w"), indent=1)
json.dump({"rows": v2["rows"]}, open(f"{w}/truth.json", "w"))
PY
cp $W/v1.json $W/prices.json
python3 $H/../update/sign.py sign $W test-2026 $W/prices.json > /dev/null
for c in prices_release unknown_bound; do
  python3 - "$W" "$KEY" "$c" <<'PY'
import json, sys
w, key, case = sys.argv[1], json.loads(sys.argv[2]), sys.argv[3]
cfg = {"policy": "finish", "prices": f"{w}/prices.json", "truth_prices": f"{w}/truth.json", "provider_time_scale": 0.001,
       "computers": [{"name": "script", "address": "127.0.0.7", "credentials": [{"value": "sk-ant-api03-script-fake"}], "limits": {"total": 100}}]}
if case == "prices_release":
    cfg["price_keys"] = [key]
else:
    cfg["prices"] = f"{w}/v1.json"; cfg["unknown_model"] = "bound"; cfg["unknown_margin"] = 2
    cfg["computers"][0]["limits"]["total"] = 1
json.dump(cfg, open(f"{w}/{case}.json", "w"), indent=1)
PY
  rm -f $W/prices.json.seen
  cp $W/v1.json $W/prices.json; python3 $H/../update/sign.py sign $W test-2026 $W/prices.json > /dev/null
  CASE=$c PRICES_DIR=$W $E/run/run.sh prices-$c $W/$c.json $E/run/scenarios/edges/base.json 240 edge > /dev/null
  python3 $H/check.py $E/out/prices-$c $c
done
rm -f $W/test-2026.key
