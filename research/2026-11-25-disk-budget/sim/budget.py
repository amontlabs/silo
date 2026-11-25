#!/usr/bin/env python3
"""One disk budget for everything Silo keeps on the Mac, simulated night by night.

Checkpoints come from #389's model (research/2026-11-25-checkpoint-retention/sim/retention.py):
main's disk as 1 MiB blocks, a checkpoint every 5 minutes, thinning by age as #376 writes it.
This adds what else Silo keeps on the same volume:

- the record's lines, from #386's sizes; never removed (`size-limit-removes-no-line`);
- message text (blobs), from #386's sizes, removable oldest first;
- recordings of computer use, from measure/screen.js, removable oldest first;
- forks: live while they work (counted like main's live disk), then 7 days in the bin
  (`time-in-the-bin`), which no limit empties early unless a variant says so;
- a reserve file (#365), which the computers write into if the volume fills;
- what grows outside Silo's reach: main's own live disk, and the Mac's files (a download).

A policy says how large Silo's part may be, and in which order its kinds go when it is over.
When nothing more may go and free space falls under `low-disk`'s levels, Silo alerts, then pauses
every computer; while paused, nothing is written. A paused night resumes once free space is back
above the alert level.

Usage: budget.py [--part=k/n] [name-filter ...]     one JSON line per run
"""
import json, math, os, sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "2026-11-25-checkpoint-retention", "sim"))
import retention as R  # noqa: E402

MIN, HOUR, DAY, GIB = R.MIN, R.HOUR, R.DAY, R.GIB
MB = 1e6 / 2**30  # one MB in GiB


def working(mod):
    h = mod / 60.0
    return 9 <= h < 18 or h >= 22 or h < 6


class Chunks:
    """Bulky content in hourly pieces: (time, GiB). Removed oldest first."""

    def __init__(self):
        self.t, self.g, self.i = [], [], 0
        self.removed = 0.0

    def add(self, t, g):
        if g > 0:
            self.t.append(t); self.g.append(g)

    def total(self):
        return sum(self.g[self.i:])

    def oldest(self):
        return self.t[self.i] if self.i < len(self.t) else None

    def pop_older_than(self, cutoff):
        if self.i < len(self.t) and self.t[self.i] < cutoff:
            g = self.g[self.i]; self.removed += g; self.i += 1
            return g
        return 0.0


def run(S):
    rng = np.random.default_rng(S["seed"])
    W, P, H, B = S["work"], S["policy"], S["host"], S["bulk"]
    disk, store = R.Disk(W, rng), R.Store()
    store.add(0.0, "auto")
    end = S["nights"] * DAY + 9 * HOUR
    vol = H["volume_gib"]
    mac_other = H["other_used_gib"]                     # macOS, apps, files, main's live disk
    host_events = {int(d * DAY + 14 * HOUR): g for d, g in H.get("events", [])}
    lines = 0.0
    text, rec = Chunks(), Chunks()
    forks_live, binned = [], []                         # (ends, GiB); (leaves the bin, GiB)
    fork_times = [int(x * HOUR) for x in W["fork_at_h"]]
    reserve = P["reserve_gib"]
    fixed_ceiling = None
    paused, paused_min, agent_paused_min, alert_min, full_min = False, 0, 0, 0, 0
    out = dict(removed={"ckpt": 0, "ckpt24": 0, "rec": 0.0, "text": 0.0, "bin": 0.0},
               max_keep=0.0, min_free=math.inf, max_bin=0.0, mornings=[])
    wrote_since = False
    restores = set(int(d * DAY + 2 * HOUR + 47) for d in S["restore_nights"])
    held_blocks = 0
    hour_lines = hour_text = hour_rec = 0.0

    def parts(held_blocks):
        ck = held_blocks / GIB
        b = sum(g for _, g in binned)
        live_forks = sum(g for _, g in forks_live)
        if P["kind"] == "separate":
            keep = ck + lines + text.total() + rec.total() + reserve   # what the plan's limits see
        else:
            keep = ck + lines + text.total() + rec.total() + b + reserve
        used = mac_other + live_forks + ck + lines + text.total() + rec.total() + b + reserve
        return ck, keep, vol - used

    for m in range(1, int(end)):
        t = float(m)
        mod = m % int(DAY)
        day = m // int(DAY)
        if m in host_events:
            mac_other += host_events[m]
        if not paused:
            # main's disk
            rate = R.schedule(mod, W)
            n = rng.poisson(rate) if rate > 0 else 0
            if n:
                disk.write(t - 0.5, n); wrote_since = True
            if day in S["big_rewrite_nights"] and mod == 23 * 60:
                disk.big_rewrite(t - 0.5, W["big_rewrite_gib"]); wrote_since = True
            if mod == 0:
                mac_other += W["live_growth_gib_d"]
            # the record, while agents work
            if working(mod):
                a = W["agents"]
                hour_lines += a * B["lines_mb_agent_h"] * MB / 60
                hour_text += a * B["text_mb_agent_h"] * MB / 60
            h = mod / 60.0
            if 10 <= h < 10 + W["computer_use_h"]:
                hour_rec += B["recording_gib_h"] / 60
            # forks: each works 6 hours, writing what it rebuilds, then goes to the bin
            if mod in fork_times:
                forks_live.append([t + 6 * HOUR, 0.0])
            for f in forks_live:
                if f[0] > t:
                    f[1] += W["fork_gib"] / (6 * HOUR)
        if m in restores:
            store.add(t, "save", t + 36 * HOUR)
        for f in [f for f in forks_live if f[0] <= t]:
            forks_live.remove(f); binned.append((t + P["bin_days"] * DAY, f[1]))
        binned = [b for b in binned if b[0] > t]
        if m % int(HOUR) == 0:
            lines += hour_lines; text.add(t, hour_text); rec.add(t, hour_rec)
            hour_lines = hour_text = hour_rec = 0.0
        if m % S["interval_min"] == 0 and wrote_since:
            store.add(t, "auto"); wrote_since = False

        if m % 15 == 0:
            store.absorb(disk)
            keep_ids = R.age_keep(store, t, P)
            for i in store.kept_ids():
                if i not in keep_ids and store.kind[i] in ("auto", "save", "alert"):
                    store.kept[i] = False
            ids, K, held_blocks, excl = store.account()
            ck, keep, free = parts(held_blocks)
            avail = free + keep
            if fixed_ceiling is None:
                fixed_ceiling = P.get("share", 1) * avail
            # the ceiling on Silo's part
            if P["kind"] == "none":
                C = math.inf
            elif P["kind"] == "fixed":
                C = fixed_ceiling
            elif P["kind"] == "floor":
                C = avail - P["floor_gib"]
            else:  # share, separate
                C = min(P["share"] * avail, avail - P["floor_gib"])

            def over():
                ck, keep, free = parts(held_blocks)
                if P["kind"] == "separate":
                    return ck - min(P["share"] * (free + ck), free + ck - P["floor_gib"]) > 0
                return keep > C

            # the record's own limit, under the separate policy
            if "record_limit_gib" in P:
                while rec.total() + text.total() > P["record_limit_gib"]:
                    g = rec.pop_older_than(t)
                    if g:
                        out["removed"]["rec"] += g; continue
                    g = text.pop_older_than(t)
                    if not g:
                        break
                    out["removed"]["text"] += g
            tiers = P["tiers"]
            for tier in tiers:
                while over():
                    if tier in ("ckpt", "ckpt24"):
                        cands = [j for j, i in enumerate(ids)
                                 if j != len(ids) - 1 and store.kind[i] == "auto"
                                 and store.protected[i] <= t
                                 and (tier == "ckpt24" or t - K[j] >= 24 * HOUR)]
                        if not cands:
                            break
                        store.kept[ids[cands[0]]] = False
                        out["removed"][tier] += 1
                        ids, K, held_blocks, excl = store.account()
                    elif tier in ("rec", "text"):
                        c = rec if tier == "rec" else text
                        g = c.pop_older_than(t - 24 * HOUR)
                        if not g:
                            break
                        out["removed"][tier] += g
                    elif tier == "bin":
                        if not binned:
                            break
                        binned.sort(key=lambda b: b[0])
                        out["removed"]["bin"] += binned.pop(0)[1]
            ck, keep, free = parts(held_blocks)
            out["max_keep"] = max(out["max_keep"], keep)
            out["min_free"] = min(out["min_free"], free)
            out["max_bin"] = max(out["max_bin"], sum(g for _, g in binned))
            # low-disk: alert, then pause; resume once back above the alert level
            if free < H["alert_gib"]:
                alert_min += 15
            if free <= 0:
                full_min += 15
            if not paused and free < H["pause_gib"]:
                paused = True
            elif paused and free >= H["alert_gib"]:
                paused = False
            if paused:
                paused_min += 15
                if working(mod):
                    agent_paused_min += 15
        if mod == 8 * 60 and day >= 1:
            autos = [store.t[i] for i in store.kept_ids() if store.kind[i] == "auto"]
            ck, keep, free = parts(held_blocks)
            o_text, o_rec = text.oldest(), rec.oldest()
            out["mornings"].append({
                "night": day, "ckpt": round(ck, 1), "keep": round(keep, 1), "free": round(free, 1),
                "reach_d": round((t - autos[0]) / DAY, 1),
                "text_d": None if o_text is None else round((t - o_text) / DAY, 1),
                "rec_d": None if o_rec is None else round((t - o_rec) / DAY, 1),
                "lines": round(lines, 2), "text": round(text.total(), 2), "rec": round(rec.total(), 1),
                "bin": round(sum(g for _, g in binned), 1),
            })
    last = out["mornings"][-1]
    return {
        "name": S["name"],
        "reach_d": last["reach_d"],
        "min_reach_d_after_10": min((x["reach_d"] for x in out["mornings"] if x["night"] >= 10), default=None),
        "text_kept_d": last["text_d"], "rec_kept_d": last["rec_d"],
        "last": {k: last[k] for k in ("ckpt", "lines", "text", "rec", "bin", "keep", "free")},
        "max_keep": round(out["max_keep"], 1), "max_bin": round(out["max_bin"], 1),
        "min_free": round(out["min_free"], 1),
        "alert_h": alert_min / 60, "paused_h": paused_min / 60,
        "agent_paused_h": agent_paused_min / 60, "full_h": full_min / 60,
        "removed": {k: (round(v, 2) if isinstance(v, float) else v) for k, v in out["removed"].items()},
        "mornings": out["mornings"],
    }


# ---------------------------------------------------------------- the grid

def work(base, **kw):
    w = dict(R.WORK[base]); w.update(kw); return w


WORK = {
    # agents, computer use and forks are guesses, chosen to go with #389's write rates
    "light": work("light", agents=1, computer_use_h=0, fork_at_h=[], fork_gib=0, live_growth_gib_d=0.2),
    "medium": work("medium", agents=3, computer_use_h=1, fork_at_h=[10, 23], fork_gib=3, live_growth_gib_d=0.5),
    "heavy": work("heavy", agents=8, computer_use_h=3, fork_at_h=[10, 13, 23, 2], fork_gib=6, live_growth_gib_d=1.0),
    "macos-no-trim": work("macos-no-trim", agents=3, computer_use_h=1, fork_at_h=[10, 23], fork_gib=3,
                          live_growth_gib_d=0.5),
}

HOSTS = {
    # as #389: 200 GiB free on a 512 GB Mac, 500 GiB on a 1 TB one, a 40 GiB download on day 12
    # removed on day 20; low-disk's normal preset (alert under 20 GB, pause under 5 GB)
    "mac-512": dict(volume_gib=460, other_used_gib=260, alert_gib=20, pause_gib=5, events=[(12, 40), (20, -40)]),
    "mac-1tb": dict(volume_gib=920, other_used_gib=420, alert_gib=20, pause_gib=5, events=[(12, 40), (20, -40)]),
}

# #386: on the modelled heavy night (3 agents, 8 hours) lines take 34.9 MB, 7.0 MB with zstd as
# closed segments are; message text 13.1 MB with each request listing what it added. With full
# lists of hashes, model-call lines take 44.1 MB more; with each request's messages kept whole,
# text takes 935 MB.
BULK = {
    "base": dict(lines_mb_agent_h=7.0 / 24, text_mb_agent_h=13.1 / 24, recording_gib_h=0.75),
    "by-quality": dict(lines_mb_agent_h=7.0 / 24, text_mb_agent_h=13.1 / 24, recording_gib_h=0.2),
    "full-lists": dict(lines_mb_agent_h=(7.0 + 44.1) / 24, text_mb_agent_h=13.1 / 24, recording_gib_h=0.75),
    "not-once": dict(lines_mb_agent_h=7.0 / 24, text_mb_agent_h=935 / 24, recording_gib_h=0.75),
}

AGE = dict(whole_h=24, hourly_days=2, daily_days=30, day_starts_at_h=0, limit_takes_hand=False)
POLICIES = {
    "no-limit": dict(kind="none", tiers=[]),
    # the plan's entries read side by side: checkpoints at half the free space with 50 GiB kept
    # free (#389), the record's own limit of 20 GB (record-size-limit's case), the bin uncounted
    "separate": dict(kind="separate", share=0.5, floor_gib=50, record_limit_gib=20, tiers=["ckpt"]),
    # one part for everything Silo keeps, half of what is free to it, 50 GiB kept free
    "one-share": dict(kind="share", share=0.5, floor_gib=50, tiers=["ckpt", "rec", "text"]),
    # the same part, with the record's own limit in force (`size-limit-on-the-record`): the part
    # counts the record but removes only checkpoints; the record's limit alone removes its content
    "one-share-own-record": dict(kind="share", share=0.5, floor_gib=50, record_limit_gib=20, tiers=["ckpt"]),
    # and the bin's oldest goes after old checkpoints, before anything pauses
    "one-share-own-record-bin": dict(kind="share", share=0.5, floor_gib=50, record_limit_gib=20,
                                     tiers=["ckpt", "bin"]),
    "one-share-record-first": dict(kind="share", share=0.5, floor_gib=50, tiers=["rec", "text", "ckpt"]),
    "one-share-bin-early": dict(kind="share", share=0.5, floor_gib=50, tiers=["ckpt", "rec", "bin", "text"]),
    # fixed when Silo is installed, at half of what was free then
    "one-fixed": dict(kind="fixed", share=0.5, tiers=["ckpt", "rec", "text"]),
    # everything but 50 GiB
    "one-floor": dict(kind="floor", floor_gib=50, tiers=["ckpt", "rec", "text"]),
}
for p in POLICIES.values():
    p.update(AGE); p.setdefault("reserve_gib", 4); p.setdefault("bin_days", 7)


def scenario(work, host, policy, bulk="base", **kw):
    S = dict(seed=1, nights=30, interval_min=5, work=WORK[work], host=HOSTS[host],
             policy=POLICIES[policy], bulk=BULK[bulk], restore_nights=[7, 19],
             big_rewrite_nights=[4, 11, 18, 25])
    S.update(kw)
    S["name"] = f"{work}/{host}/{policy}/{bulk}" + "".join(f"/{k}={v}" for k, v in kw.items())
    return S


def grid():
    for host in HOSTS:
        for w in WORK:
            for pol in ("no-limit", "separate", "one-share", "one-share-record-first", "one-fixed", "one-floor"):
                yield scenario(w, host, pol)
    for host in HOSTS:
        yield scenario("heavy", host, "one-share-bin-early")
        yield scenario("medium", host, "one-share-bin-early")
        for bulk in ("by-quality", "full-lists", "not-once"):
            yield scenario("heavy", host, "one-share", bulk)
    yield scenario("medium", "mac-512", "one-share", seed=2)
    yield scenario("medium", "mac-512", "one-fixed", seed=2)


def grid2():
    """The policy recommended once `size-limit-on-the-record` (taken) is read in, and its variants."""
    for host in HOSTS:
        for w in WORK:
            yield scenario(w, host, "one-share-own-record")
            yield scenario(w, host, "one-share-own-record-bin")
        for bulk in ("by-quality", "full-lists", "not-once"):
            yield scenario("heavy", host, "one-share-own-record", bulk)
    yield scenario("medium", "mac-512", "one-share-own-record", seed=2)
    yield scenario("medium", "mac-512", "one-share-own-record", seed=3)


if __name__ == "__main__":
    want = [a for a in sys.argv[1:] if not a.startswith("--part=")]
    part = [a for a in sys.argv[1:] if a.startswith("--part=")]
    k, n = map(int, part[0][7:].split("/")) if part else (0, 1)
    g = grid2() if "--grid2" in sys.argv else grid()
    want = [a for a in want if a != "--grid2"]
    for idx, S in enumerate(g):
        if idx % n != k or (want and not any(w in S["name"] for w in want)):
            continue
        print(json.dumps(run(S)), flush=True)
