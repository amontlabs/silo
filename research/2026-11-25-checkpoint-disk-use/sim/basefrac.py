#!/usr/bin/env python3
"""Share of what checkpoints hold that sits in the oldest one, the base clone of #365's `delta`.

With forward deltas the oldest checkpoint is a clone of the disk image, so its blocks cannot be
compressed; only the deltas after it can. This prints, each morning, the share of held blocks
whose first holder is the oldest kept checkpoint.
"""
import json, os, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../2026-11-25-checkpoint-retention/sim"))
import retention as R

shares = []
_acc = R.Store.account
def account(self):
    ids, K, held, excl = _acc(self)
    if len(K) and held:
        lo = np.searchsorted(K, self.vs, "right")
        shares.append(float((lo == 0).sum()) / held)
    return ids, K, held, excl
R.Store.account = account
for work, host in (("medium", "mac-512"), ("heavy", "mac-512"), ("medium", "mac-1tb"), ("macos-no-trim", "mac-512")):
    shares.clear()
    S = R.scenario(work, host, "30d-half-oldest")
    R.run(S)
    tail = shares[len(shares) // 2:]
    print(json.dumps({"case": f"{work}/{host}", "base_share_mean_2nd_half": round(sum(tail) / len(tail), 3),
                      "base_share_last": round(shares[-1], 3)}), flush=True)
