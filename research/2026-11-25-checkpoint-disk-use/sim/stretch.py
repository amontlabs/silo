#!/usr/bin/env python3
"""Days back gained by each way to stretch checkpoints, on #389's simulation.

Runs `retention.py` from research/2026-11-25-checkpoint-retention/sim unchanged, with three
knobs added from outside:

- store_ratio: bytes the store keeps on the Mac for each byte a checkpoint holds (compression
  and dedup of the delta files; 1.0 is #389).
- excluded: share of the computer's rewrites that land in paths left out of checkpoints (they
  go to a disk no checkpoint holds), applied to the steady writes and to the reinstalls.
- external_gib: room on an external disk for the checkpoints the Mac's limit would remove,
  in GiB the store keeps there (after store_ratio). The local limit is unchanged.

Usage: stretch.py [--part=k/n] [name-filter ...]   Prints one JSON line per run.
"""
import copy, json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../2026-11-25-checkpoint-retention/sim"))
import retention as R

GIB = R.GIB
_account = R.Store.account
_limit = R.limit_blocks
KNOBS = {"store_ratio": 1.0, "external_gib": 0.0}


def account(self):
    ids, K, held, excl = _account(self)
    return ids, K, int(held * KNOBS["store_ratio"]), excl


def limit_blocks(P, free, held):
    # free space on the Mac is unchanged by what sits on the external disk: the
    # store spills the oldest checkpoints there instead of removing them.
    ext = KNOBS["external_gib"] * GIB
    local_held = max(held - ext, 0)
    return _limit(P, free + (held - local_held), local_held) + ext


R.Store.account = account
R.limit_blocks = limit_blocks


def run(work, host, policy, store_ratio=1.0, excluded=0.0, external_gib=0.0, label=""):
    W = copy.deepcopy(R.WORK[work])
    for k in ("day_gib_h", "night_gib_h", "idle_gib_h", "big_rewrite_gib"):
        W[k] = W[k] * (1 - excluded)
    S = R.scenario(work, host, policy)
    S["work"] = W
    KNOBS.update(store_ratio=store_ratio, external_gib=external_gib)
    out = R.summarize(S, R.run(S))
    out["name"] = f"{work}/{host}/{policy}/{label}"
    out.update(store_ratio=store_ratio, excluded=excluded, external_gib=external_gib)
    return out


# Filled from results/capture-*.json (see README): measured on the Linux workload.
OPTIONS = json.load(open(os.path.join(HERE, "options.json")))
CASES = [("light", "mac-512"), ("medium", "mac-512"), ("heavy", "mac-512"),
         ("macos-no-trim", "mac-512"), ("medium", "mac-1tb")]


def grid():
    for work, host in CASES:
        for label, o in OPTIONS.items():
            yield work, host, "30d-half-oldest", o, label


if __name__ == "__main__":
    want = [a for a in sys.argv[1:] if not a.startswith("--part=")]
    part = [a for a in sys.argv[1:] if a.startswith("--part=")]
    k, n = map(int, part[0][7:].split("/")) if part else (0, 1)
    for i, (work, host, pol, o, label) in enumerate(grid()):
        name = f"{work}/{host}/{pol}/{label}"
        if i % n != k or (want and not any(w in name for w in want)):
            continue
        print(json.dumps(run(work, host, pol, label=label, **o)), flush=True)
