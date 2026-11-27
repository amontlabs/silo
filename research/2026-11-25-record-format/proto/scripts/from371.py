"""Converts the record of a night of #371's prototype (research/2026-11-24-morning-review) into
lines for `silorec import`, in the format of ../../format.md. Both computers go into one chain:
this measures size, not separation.

    python3 from371.py <night>/record full|delta > lines.jsonl
"""
import datetime, json, os, sys

root, form = sys.argv[1], sys.argv[2]
KIND = {"model call": "model-call", "owner": "connection"}
ENVELOPE = {"id", "kind", "line", "at", "computer", "source", "prev"}
lines = []
for comp in sorted(os.listdir(root)):
    for f in ("facts", "reports"):
        p = os.path.join(root, comp, f + ".jsonl")
        if os.path.exists(p):
            lines += [json.loads(x) for x in open(p) if x.strip()]
lines.sort(key=lambda o: o["at"])

counters, last_list = {}, {}
for seq, o in enumerate(lines, 1):
    t = datetime.datetime.fromisoformat(o["at"])
    body = {}
    kind = KIND.get(o["line"], o["line"])
    if kind == "control":
        by = str(o.get("by", "silo"))
        who = by.split(":")[0]
        who = {"rule": "rule", "precaution": "precaution", "you": "you", "silo": "silo"}.get(who, "silo")
        body["author"] = {"by": who, **({"person": "p1"} if who == "you" else {})}
        if ":" in by:
            body["rule"] = by.split(":", 1)[1]
    for k, v in o.items():
        if k not in ENVELOPE and k != "by":
            body[k] = v
    if kind == "model-call" and "messages_ref" in body:
        conv = body.get("conversation")
        full = body.pop("messages_ref")
        prev = last_list.get(conv)
        if form == "delta" and prev and full[: len(prev[1])] == prev[1]:
            body["sent"] = {"after": prev[0], "added": full[len(prev[1]):]}
        else:
            body["sent"] = full
        last_list[conv] = (seq, full)
    src = o["source"]
    counters[src] = counters.get(src, 0) + 1
    members = json.dumps(body, ensure_ascii=False, separators=(",", ":"))[1:-1]
    print(json.dumps({"time_ns": int(t.timestamp() * 1e9), "source": src, "count": src[0] + ":1",
                      "n": counters[src], "is": o["kind"], "kind": kind, "body": members}))
