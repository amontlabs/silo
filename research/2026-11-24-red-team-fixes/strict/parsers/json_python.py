"""json.loads: what a Python host sees of a body. usage: json_python.py <file>"""
import json, sys
o = {"ok": False, "web_fetch": False, "max_tokens": None}
try:
    j = json.loads(open(sys.argv[1], "rb").read())
    o = {"ok": True, "web_fetch": "web_fetch" in json.dumps(j.get("tools", [])), "max_tokens": str(j.get("max_tokens"))}
except Exception: pass
print(json.dumps(o))
