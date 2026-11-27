"""Print what reached the fake provider, and the boundary's record, for an output folder."""
import json, sys
o = sys.argv[1]
print("-- reached the provider")
for l in open(f"{o}/requests.jsonl"):
    r = json.loads(l); a = r["auth"]
    print(f"  {r['method']:4} {r['host']}{r['path'].split('?')[0]:28} {r['verdict']:58} {a[:22]+'…' if a else '-'}")
print("-- the boundary's record")
try:
    for l in open(f"{o}/record.jsonl"):
        r = json.loads(l); r.pop("t"); print("  " + json.dumps(r))
except FileNotFoundError: pass
