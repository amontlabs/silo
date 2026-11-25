"""Makes a capture small enough to commit: long bodies dropped, long strings inside JSON bodies
cut to 200 characters (the rule reads their shape, not the loot), and gsutil's built-in API key
masked.

    python3 slim.py capture.jsonl > data/capture.jsonl
"""
import base64
import json
import re
import sys


def cut(v):
    if isinstance(v, str):
        return v if len(v) <= 200 else v[:200] + f"...[{len(v)} characters]"
    if isinstance(v, dict):
        return {k: cut(x) for k, x in v.items()}
    if isinstance(v, list):
        return [cut(x) for x in v]
    return v


for line in open(sys.argv[1]):
    r = json.loads(line)
    if r["case"] in ("setup", "done"):
        continue
    b = r.get("body_b64")
    if b and len(b) > 8000:
        raw = base64.b64decode(b)
        try:
            r["body_b64"] = base64.b64encode(json.dumps(cut(json.loads(raw))).encode()).decode()
            r["body_cut"] = True
        except ValueError:
            r["body_b64"], r["body_dropped"] = None, True
    s = re.sub(r"AIza[0-9A-Za-z_\-]{20,}", "AIza-REDACTED-gsutil-built-in-key", json.dumps(r))
    print(s)
