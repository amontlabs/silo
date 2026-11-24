"""Copies the fake Anthropic API of research/2026-11-24-experiments into <dir> and adds two knobs:
HOLD_S=<seconds> holds the first request that long before answering, as a hold at the boundary
would; REFUSE=<n> answers the first n requests with a 403 and Silo's words, as a hold that ended
without your answer would.

Run: python3 patch_fake.py <dir>; then HOLD_S=150 ./run_cc.sh <out> <dir>/fake_anthropic.py
"""
import os, shutil, sys

SRC = "/mnt/project-files/research/2026-11-24-experiments"
d = sys.argv[1]
os.makedirs(d, exist_ok=True)
for f in ("fake_anthropic.py", "probe.py"):
    shutil.copy(os.path.join(SRC, f), d)
p = os.path.join(d, "fake_anthropic.py")
s = open(p).read()
anchor = '        if "count_tokens" in self.path:'
add = '''        state.setdefault("n", 0); state["n"] += 1
        if os.environ.get("HOLD_S") and state["n"] == 1:
            log("holding first request %s s" % os.environ["HOLD_S"]); time.sleep(float(os.environ["HOLD_S"])); log("hold over")
        if os.environ.get("REFUSE") and state["n"] <= int(os.environ["REFUSE"]):
            log("refusing request %d" % state["n"])
            return self.json({"type": "error", "error": {"type": "permission_error", "message": "Silo held this request: it carries a string with GitHub's token format and checksum. Answer the ask in Silo."}}, 403)
'''
assert anchor in s
open(p, "w").write(s.replace(anchor, add + anchor, 1))
print("patched", p)
