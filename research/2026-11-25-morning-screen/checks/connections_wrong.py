"""Exits 1 when a connection is named for a conversation of a run other than the one whose process
opened it (the probe's owner report). Scored on runs, so a run with no conversation counts.

    python3 checks/connections_wrong.py <night> [--fixed]

Without --fixed: #399's attribute.py as it is. With --fixed: traffic_author.py's rule.
"""
import os, sys
sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, ".."))
sys.path.insert(0, os.path.join(HERE, "..", "..", "2026-11-25-attribution-without-processes"))
import attribute as A  # noqa: E402
import traffic_author as T  # noqa: E402

night = os.path.abspath(sys.argv[1])
v = A.View(os.path.join(night, "record"), now=None)
res, calls = A.attribute(v)
run_of_conv = {cv["id"]: cv["run"] for cv in v.convs.values()}
if "--fixed" in sys.argv:
    progs = {cv["id"]: A.program_of_conversation(cv) for cv in v.convs.values()}
    span = {cv["id"]: (A.R.ts_of(cv["calls"][0]["started"]), A.R.ts_of(cv["calls"][-1]["at"])) for cv in v.convs.values()}
    end = max(A.R.ts_of(l["at"]) for l in v.lines)
    fixed = T.connections(v, calls, progs, span, A.R.ts_of, end)
    for r in res:
        if r["kind"] == "connection":
            r["by"], r["how"], r["tool_call"] = fixed[r["id"]]
right = wrong = none = 0
for r in res:
    if r["kind"] != "connection":
        continue
    l = v.by_id[r["id"]]
    pid = v.owner.get(r["id"])
    run = v.run_of_pid(l["computer"], pid) if pid else None
    if run is None:
        continue
    named = run_of_conv.get(r["by"]) if r["by"] else None
    if named is None:
        none += 1
    elif named is run:
        right += 1
    else:
        wrong += 1
        print(f"WRONG {r['id']} {r['path']}: named {named['name']} ({r['how']}), opened by {run['name']}")
print(f"connections with a run by the probe: right {right}, wrong {wrong}, none {none}")
sys.exit(1 if wrong else 0)
