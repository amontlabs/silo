"""Checks one run of run.sh: what the boundary read, passed or refused on each computer, against
what the stand-in provider served. Exits 1 if any expectation fails."""
import collections, json, pathlib, sys

out = pathlib.Path(sys.argv[1])
rec = [json.loads(l) for l in open(out / "record.jsonl")]
served = sum(1 for _ in open(out / "ledger.jsonl"))
comp_of = {r["conn"]: r.get("computer") for r in rec if r.get("kind") == "open"}
per = collections.defaultdict(collections.Counter)
for r in rec:
    c = r.get("computer") or comp_of.get(r.get("conn"))
    if c:
        per[c][r["kind"] + ("/" + r["why"] if "why" in r else "")] += 1
exit_of = {p: (out / p / "result.txt").read_text().split()[0] for p in ("kk", "kp", "pp", "pk", "rp")}
said = (out / "kp" / "out.txt").read_text()
raw = (out / "record.jsonl").read_text()
checks = [
    ("key, signed in with a key: read, calls recorded", per["key-signed-key"]["call"] > 0 and exit_of["kk"] == "exit=0"),
    ("key, signed in with a plan: refused, nothing read", per["key-signed-plan"]["call"] == 0 and per["key-signed-plan"]["refused/plan-in-a-computer-set-for-a-key"] > 0 and exit_of["kp"] == "exit=1"),
    ("key, signed in with a plan: Claude Code prints Silo's sentence", "set for Claude with an API key" in said),
    ("plan, signed in with a plan: works, passed unread", per["plan-signed-plan"]["call"] == 0 and per["plan-signed-plan"]["connection"] > 0 and exit_of["pp"] == "exit=0"),
    ("plan, signed in with a key: works, passed unread", per["plan-signed-key"]["call"] == 0 and per["plan-signed-key"]["connection"] > 0 and exit_of["pk"] == "exit=0"),
    ("read (today): a plan is read", per["read-signed-plan"]["call"] > 0 and exit_of["rp"] == "exit=0"),
    ("no sign-in value in the record", "sk-ant-oat" not in raw and "sk-ant-api" not in raw),
    ("the provider served calls Silo never read", served > per["key-signed-key"]["call"] + per["read-signed-plan"]["call"]),
]
for c, v in per.items():
    print(f"{c:18} {dict(sorted(v.items()))}")
read = per["key-signed-key"]["call"] + per["read-signed-plan"]["call"]
print(f"stand-in provider served {served} calls; Silo read {read}; {served - read} passed unread")
bad = 0
for name, ok in checks:
    print(("ok   " if ok else "FAIL ") + name)
    bad += not ok
sys.exit(1 if bad else 0)
