"""#377's claim checker marks a false «All tests pass.» as matched when the narrowed run is npm's.

    python3 claims_npm_narrowed.py <research folder of #377's branch>

The night of #373: the whole suite fails, then the agent runs the auth tests alone and says the
tests pass. With `node --test test/auth.test.js` as the narrowed run, claims@0.3 says mismatch.
With `npm test -- test/auth`, the usual npm way to narrow a run, it says matched. Exits 1 while a
false claim comes out matched.
"""
import os, sys

sys.path.insert(0, os.path.join(os.path.abspath(sys.argv[1]), "2026-11-24-morning-review", "proto"))
import claims

CLAIMS = ["All tests pass.", "The test suite is green."]
NARROWED = ["node --test test/auth.test.js", "npm test -- test/auth"]
wrong = 0
for narrowed in NARROWED:
    ev = {"commands": [{"command": "npm test", "exit": 1, "t": 0}, {"command": narrowed, "exit": 0, "t": 2}],
          "edits": [{"path": "src/session.js", "change": "changed", "inside": True, "t": None}],
          "left_running": [], "connections": [], "cwd": "the repo"}
    for claim in CLAIMS:
        says = claims.check_03(claim, ev)[0]["says"]
        ok = says == "mismatch"
        wrong += not ok
        print(f"{'ok   ' if ok else 'WRONG'}  {narrowed:<32} {claim:<26} -> {says}")
sys.exit(1 if wrong else 0)
