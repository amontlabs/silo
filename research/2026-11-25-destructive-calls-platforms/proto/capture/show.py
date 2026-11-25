#!/usr/bin/env python3
"""show.py <cli> <command name>: the requests a command sent, from captures/<cli>.jsonl."""
import json, sys
cli, name = sys.argv[1], sys.argv[2]
on = False
for line in open("captures/%s.jsonl" % cli):
    e = json.loads(line)
    if e.get("event") == "command_start":
        on = e["name"] == name
    elif on and e.get("event") == "request":
        b = (e.get("body") or "").replace("\\n", " ").replace("\\t", " ")
        print(e["method"], e["host"], e["target"], b[:int(sys.argv[3]) if len(sys.argv) > 3 else 300])
