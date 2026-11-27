"""Every live run, three at a time, appended to out/live.jsonl; a run already there is not run again.

python3 experiments.py [group ...]      groups: calib, faults, sleep, batch, real
"""
import json, os, sys, threading
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run as live  # noqa: E402

OUT = os.path.join(live.OUT, "live.jsonl")
TC = {"policy": "reserve_typical", "cut": True}   # #385's typical-cut
CUT = {"policy": "finish_tps", "cut": True}       # #385's cut
KT = {"keep_typical": True}                       # leases: a recall keeps each running call's typical output
BATCH = [2, 20.0, 98, 4000, 150.0]                # the medium Mac, at 20 s, 98 requests of max 4,000 tokens, results 150 s later

GROUPS = {
    "calib": [(n, s, c) for s in range(1, 4) for n, c in [
        ("one boundary, typical-cut", {"way": "local", **TC}),
        ("central 30 ms, typical-cut", {"way": "central", **TC}),
        ("leases 30 ms, typical-cut", {"way": "lease", **TC, **KT}),
        ("leases 30 ms, typical-cut, a recall keeps $1", {"way": "lease", **TC}),
        ("one boundary, cut", {"way": "local", **CUT}),
        ("central 30 ms, cut", {"way": "central", **CUT}),
        ("leases 30 ms, cut", {"way": "lease", **CUT, **KT}),
        ("leases 30 ms, cut, a recall keeps $1", {"way": "lease", **CUT}),
        ("leases 30 ms, cut, #388's recall", {"way": "lease", **CUT, "keep": 0.0, "rearm": False}),
    ]],
    "faults": [(n, s, c) for s in range(1, 3) for n, c in [
        ("central, ledger killed 40-70 s", {"way": "central", **TC, "kill": [40, 70]}),
        ("leases, ledger killed 40-70 s", {"way": "lease", **KT, **TC, "kill": [40, 70]}),
        ("central, all Macs cut off 40-70 s", {"way": "central", **TC, "partition": [40, 70, [0, 1, 2, 3]]}),
        ("leases, all Macs cut off 40-70 s", {"way": "lease", **KT, **TC, "partition": [40, 70, [0, 1, 2, 3]]}),
        ("central, ledger killed 40-70 s, calls refused", {"way": "central", **TC, "kill": [40, 70], "on_down": "deny"}),
        ("central, ledger killed 40-160 s", {"way": "central", **TC, "kill": [40, 160]}),
        ("leases, ledger killed 40-160 s", {"way": "lease", **KT, **TC, "kill": [40, 160]}),
        ("leases, 2 Macs cut off 40-100 s", {"way": "lease", **KT, **TC, "partition": [40, 100, [0, 2]]}),
    ]],
    "sleep": [(n, s, c) for s in range(1, 3) for n, c in [
        ("leases, a heavy Mac asleep 30-150 s, no expiry", {"way": "lease", **KT, **TC, "sleep": [30, 150, 0]}),
        ("leases, a heavy Mac asleep 30-150 s, 20 s leases", {"way": "lease", **KT, **TC, "sleep": [30, 150, 0], "ttl": 20.0}),
    ]],
    "batch": [(n, s, c) for s in range(1, 3) for n, c in [
        ("one boundary, typical-cut, a batch", {"way": "local", **TC, "batch": [0] + BATCH[1:]}),
        ("central 30 ms, typical-cut, a batch", {"way": "central", **TC, "batch": BATCH}),
        ("leases 30 ms, typical-cut, a batch", {"way": "lease", **KT, **TC, "batch": BATCH}),
    ]],
    "real": [(n, s, c) for s in (1,) for n, c in [
        ("central 30 ms, typical-cut, 50 tokens/s", {"way": "central", **TC, "tps": 50.0, "until": 3600}),
        ("leases 30 ms, typical-cut, 50 tokens/s", {"way": "lease", **KT, **TC, "tps": 50.0, "until": 3600}),
    ]],
}


def done():
    try:
        with open(OUT) as f:
            return {(r["name"], r["seed"]) for r in map(json.loads, f)}
    except OSError:
        return set()


def main():
    groups = sys.argv[1:] or ["calib", "faults", "sleep", "batch"]
    have = done()
    todo = [(n, s, c) for g in groups for n, s, c in GROUPS[g] if (n, s) not in have]
    mu = threading.Lock()
    os.makedirs(os.path.join(live.OUT, "runs"), exist_ok=True)

    def one(item):
        n, s, c = item
        r = live.run(dict(c, name=n, seed=s))
        with mu, open(OUT, "a") as f:
            f.write(json.dumps(r) + "\n")
        print(f"{n} seed {s}: billed ${r['billed']} ({r['over_pct']:+}%), cut ${r['cut_paid']}, waits>10s {r['waited_10s']}", flush=True)
    print(f"{len(todo)} runs", flush=True)
    with ThreadPoolExecutor(int(os.environ.get("PARALLEL", "4"))) as ex:
        list(ex.map(one, todo))


if __name__ == "__main__":
    main()
