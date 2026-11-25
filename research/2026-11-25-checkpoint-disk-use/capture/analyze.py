#!/usr/bin/env python3
"""What the captured checkpoint deltas would take under dedup, compression and exclusions."""
import json, sys, os
import numpy as np, zstandard, lz4.frame
sys.path.insert(0, os.path.dirname(__file__))
from ckcap import CATS, BS

D = sys.argv[1]
EXCL = {"node_modules", "npm-cache", "target", "cargo-home", "venv", "pip-cache", "other-cache"}
tags = json.load(open(f"{D}/state.json"))["tags"]
base = tags[1]  # the first checkpoint with the computer's system in it
deltas = [t for t in tags[2:] if os.path.exists(f"{D}/{t}.delta")]

def csize(data, frame, how):
    tot = 0
    for i in range(0, len(data), frame):
        f = data[i:i + frame]
        if how == "lz4":
            tot += len(lz4.frame.compress(f, compression_level=0))
        else:
            tot += len(zstandard.ZstdCompressor(level=how).compress(f))
    return tot

store = set(np.load(f"{D}/{base}.hash.npy").tolist()) - {0}
rows, T = [], {}
def add(k, v): T[k] = T.get(k, 0) + v
for t in deltas:
    z = np.load(f"{D}/{t}.delta.npz")
    data = open(f"{D}/{t}.delta", "rb").read()
    h, cat = z["oldhash"], z["cat"]
    n = len(h)
    raw = n * BS
    r = {"tag": t, "raw_mib": raw / 2**20}
    # by category
    r["by_cat_mib"] = {CATS[c]: int((cat == c).sum()) * BS / 2**20 for c in np.unique(cat)}
    ex = np.isin(cat, [CATS.index(c) for c in EXCL])
    r["excluded_mib"] = int(ex.sum()) * BS / 2**20
    # compression of the whole delta
    for name, frame, how in (("zstd1_4k", 4096, 1), ("zstd3_64k", 65536, 3), ("lz4_64k", 65536, "lz4"),
                             ("zstd3_1m", 1 << 20, 3)):
        r[name] = csize(data, frame, how) / 2**20
    # dedup: within the delta, then against everything the store already holds
    _, first = np.unique(h, return_index=True)
    r["dedup_within_mib"] = len(first) * BS / 2**20
    keep = np.array([i for i in sorted(first) if int(h[i]) not in store], dtype=np.int64)
    r["dedup_store_mib"] = len(keep) * BS / 2**20
    uniq = b"".join(data[i * BS:(i + 1) * BS] for i in keep)
    r["dedup_store_zstd3_64k"] = csize(uniq, 65536, 3) / 2**20
    # exclusions first, then dedup and compression of what is left
    keep_ex = np.array([i for i in keep if not ex[i]], dtype=np.int64)
    r["excl_dedup_zstd3_64k"] = csize(b"".join(data[i * BS:(i + 1) * BS] for i in keep_ex), 65536, 3) / 2**20
    r["excl_zstd3_64k"] = csize(b"".join(data[i * BS:(i + 1) * BS] for i in np.nonzero(~ex)[0]), 65536, 3) / 2**20
    store.update(int(x) for x in h)
    for k, v in r.items():
        if isinstance(v, float): add(k, v)
    for c, v in r["by_cat_mib"].items(): add("cat:" + c, v)
    rows.append(r)
    print(json.dumps({k: (round(v, 1) if isinstance(v, float) else v) for k, v in r.items()}), flush=True)
print(json.dumps({"TOTAL": {k: round(v, 1) for k, v in T.items()}}))
