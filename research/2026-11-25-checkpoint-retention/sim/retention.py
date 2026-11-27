#!/usr/bin/env python3
"""Checkpoint retention under a disk limit, simulated night by night.

One computer (main) on a Mac. Its disk image is a grid of 1 MiB blocks. Each write
ends the block's previous version; a checkpoint holds every version that was live at
its moment. The space checkpoints take beyond the live disk is the number of ended
versions that at least one kept checkpoint holds (as with clones that share blocks,
#365's `full`, or chained deltas, `delta`: both keep exactly these blocks).

Thinning by age follows the answer of `thinning-checkpoints` as #376 writes it:
every checkpoint of the last 24 hours, the first of each hour for 2 days, the first
of each day for 30 days, and, whatever their age, the save before a restore and a
checkpoint an open alert points to. Then a disk policy removes more while
checkpoints take more than the limit.

Usage: retention.py [--part=k/n] [name-filter ...]   (no filter runs the whole grid)
Prints one JSON line per run.
"""
import json, math, sys
import numpy as np

MIN = 1.0
HOUR = 60.0
DAY = 1440.0
GIB = 1024  # blocks of 1 MiB


# ---------------------------------------------------------------- workload

def schedule(minute_of_day, w):
    """Write rate (blocks a minute) at a minute of the day."""
    h = minute_of_day / 60.0
    if 9 <= h < 18:
        return w["day_gib_h"] * GIB / 60.0
    if h >= 22 or h < 6:
        return w["night_gib_h"] * GIB / 60.0
    return w["idle_gib_h"] * GIB / 60.0


class Disk:
    """The computer's disk: where each write lands, and the versions it ends."""

    def __init__(self, w, rng):
        self.w, self.rng = w, rng
        self.nblocks = w["disk_gib"] * GIB
        self.used = w["used_gib"] * GIB          # blocks the guest's files occupy
        self.ws = w["working_set_gib"] * GIB      # the hot part of those files
        self.last = np.full(self.nblocks, -1.0)  # when each block's version began
        self.last[: self.used] = 0.0
        self.alloc = self.used                   # macOS without TRIM: next free place
        self.free_region = self.nblocks - self.used
        self.ended_s, self.ended_e = [], []

    def targets(self, n):
        w, rng = self.w, self.rng
        if w["guest"] == "linux-trim":
            hot = rng.random(n) < 0.8
            b = np.where(hot, rng.integers(0, self.ws, n), rng.integers(0, self.used, n))
            return b
        # macOS guest whose TRIM does not reach the image (#365 Mac test 6): APFS
        # writes new data to free places, so a rewrite lands on the next free block
        # in turn and the old place keeps its old bytes in the image.
        b = (self.alloc - self.used + np.arange(n)) % self.free_region + self.used
        self.alloc = (self.alloc + n - self.used) % self.free_region + self.used
        return b

    def write(self, t, n):
        if n <= 0:
            return
        b = np.unique(self.targets(n))
        prev = self.last[b]
        live = prev >= 0
        self.ended_s.append(prev[live])
        self.ended_e.append(np.full(int(live.sum()), t))
        self.last[b] = t

    def big_rewrite(self, t, gib):
        """A dependency reinstall or a clean build: gib of the working set rewritten."""
        n = int(gib * GIB)
        if self.w["guest"] == "linux-trim":
            start = self.rng.integers(0, max(1, self.ws - n))
            b = np.arange(start, start + n) % self.used
            prev = self.last[b]
            self.ended_s.append(prev.copy())
            self.ended_e.append(np.full(n, t))
            self.last[b] = t
        else:
            self.write(t, n)


# ---------------------------------------------------------------- the store

class Store:
    def __init__(self):
        self.t = []          # checkpoint moments
        self.kind = []       # auto | hand | save | alert
        self.protected = []  # until when it may not be removed (inf = until set aside)
        self.kept = []
        self.vs = np.empty(0)
        self.ve = np.empty(0)

    def add(self, t, kind, protected_until=-1.0):
        self.t.append(t); self.kind.append(kind)
        self.protected.append(protected_until); self.kept.append(True)
        return len(self.t) - 1

    def absorb(self, disk):
        if disk.ended_s:
            self.vs = np.concatenate([self.vs] + disk.ended_s)
            self.ve = np.concatenate([self.ve] + disk.ended_e)
            disk.ended_s.clear(); disk.ended_e.clear()

    def kept_ids(self):
        return [i for i, k in enumerate(self.kept) if k]

    def account(self):
        """Blocks held, and blocks held by one checkpoint only, for each kept one."""
        ids = self.kept_ids()
        K = np.array([self.t[i] for i in ids])
        lo = np.searchsorted(K, self.vs, "right")   # first checkpoint after start
        hi = np.searchsorted(K, self.ve, "left")    # first checkpoint at or after end
        cnt = hi - lo
        held = cnt > 0
        # versions no kept checkpoint holds can never be held again: forget them
        self.vs, self.ve = self.vs[held], self.ve[held]
        lo, cnt = lo[held], cnt[held]
        excl = np.bincount(lo[cnt == 1], minlength=len(ids))
        return ids, K, int(held.sum()), excl


# ---------------------------------------------------------------- policies

def age_keep(store, now, P):
    """Ids the age rule keeps; the rest of the automatic ones go."""
    keep = set()
    hour_seen, day_seen = set(), set()
    off = P["day_starts_at_h"] * HOUR
    for i in store.kept_ids():
        t, kind = store.t[i], store.kind[i]
        age = now - t
        if kind != "auto" or store.protected[i] > now:
            keep.add(i); continue
        if age < P["whole_h"] * HOUR:
            keep.add(i); continue
        hb = int(t // HOUR)
        if age < P["hourly_days"] * DAY and hb not in hour_seen:
            hour_seen.add(hb); keep.add(i); continue
        db = int((t - off) // DAY)
        if age < P["daily_days"] * DAY and db not in day_seen:
            day_seen.add(db); keep.add(i); continue
    return keep


def removable(store, i, now, P):
    if store.protected[i] > now:
        return False
    if store.kind[i] == "hand" and not P["limit_takes_hand"]:
        return False
    # never the newest checkpoint
    return True


def pick_victim(store, ids, K, excl, now, P):
    cands = [j for j, i in enumerate(ids) if j != len(ids) - 1 and removable(store, i, now, P)]
    # the limit never reaches into the last whole window if anything older can go
    older = [j for j in cands if now - K[j] >= P["whole_h"] * HOUR]
    if older:
        cands = older
    if not cands:
        return None
    if P["order"] == "oldest":
        return cands[0]
    # "even": remove the checkpoint whose removal leaves the smallest gap relative
    # to its age, so the kept ones thin out evenly on a log scale of age and the
    # reach is kept as long as the disk allows.
    best, bscore = None, math.inf
    for j in cands:
        if j == 0:
            score = math.inf if P["order"] == "even" else 0  # the oldest sets the reach
            if P["order"] == "even":
                continue
        else:
            gap = K[j + 1] - K[j - 1]
            score = gap / max(now - K[j], HOUR)
            if P["order"] == "cheapest":
                score = score / max(excl[j], 1)
        if score < bscore:
            best, bscore = j, score
    if best is None:
        best = cands[0]
    return best


def limit_blocks(P, host_free_blocks, held):
    """The limit on what checkpoints hold, from the space not used by anything else."""
    avail = host_free_blocks + held     # what checkpoints could have
    L = math.inf
    if P.get("limit_gib") is not None:
        L = min(L, P["limit_gib"] * GIB)
    if P.get("limit_share") is not None:
        L = min(L, P["limit_share"] * avail)
    if P.get("floor_gib") is not None:
        L = min(L, avail - P["floor_gib"] * GIB)
    return max(L, 0)


# ---------------------------------------------------------------- one run

def run(S):
    rng = np.random.default_rng(S["seed"])
    W, P, H = S["work"], S["policy"], S["host"]
    disk, store = Disk(W, rng), Store()
    store.add(0.0, "auto")
    nights = S["nights"]
    end = nights * DAY + 9 * HOUR                     # the morning after the last night
    interval = S["interval_min"]
    host_other = H["other_used_gib"] * GIB           # the Mac's own files, the live image
    host_size = H["volume_gib"] * GIB
    # what happens to the Mac besides Silo: one large download that stays for a while
    host_events = {int(d * DAY + 14 * HOUR): g * GIB for d, g in H.get("events", [])}
    restores = set(int(d * DAY + 2 * HOUR + 47) for d in S.get("restore_nights", []))
    alert_nights = set(S.get("alert_nights", []))
    big = set(S.get("big_rewrite_nights", []))
    out = {"removed_by_limit": 0, "removed_young": 0, "max_removed_hour": 0,
           "min_free_gib": math.inf, "alert_floor_hits": 0, "pause_floor_hits": 0,
           "mornings": [], "max_held_gib": 0.0}
    removed_this_hour, hour = 0, 0
    last_ckpt_write = 0.0
    wrote_since = False
    for m in range(1, int(end)):
        t = float(m)
        mod = m % int(DAY)
        if m in host_events:
            host_other += host_events[m]
        # writes in this minute happen at t - 0.5
        rate = schedule(mod, W)
        n = rng.poisson(rate) if rate > 0 else 0
        if n:
            disk.write(t - 0.5, n); wrote_since = True
        day = m // int(DAY)
        if day in big and mod == 23 * 60:
            disk.big_rewrite(t - 0.5, W["big_rewrite_gib"]); wrote_since = True
        if m in restores:
            # a restore at 02:47: Silo saves first; the save stays until set aside
            store.add(t, "save", math.inf if S["saves_set_aside_h"] is None
                      else t + S["saves_set_aside_h"] * HOUR)
        if day in alert_nights and mod == 3 * 60 + 12:
            # an alert points to the checkpoint before a flagged call; set aside at
            # 10:00, or forgotten
            ttl = math.inf if rng.random() < S["alerts_forgotten"] else (DAY - mod + 10 * HOUR)
            store.add(t, "alert", t + ttl)
        if mod == 22 * 60 and S["hand_before_bed"]:
            store.add(t, "hand")
        if m % interval == 0 and wrote_since:
            store.add(t, "auto"); wrote_since = False
        if m % S["thin_every_min"] == 0:
            store.absorb(disk)
            # protected ones whose time ran out go back to the age rule
            keep = age_keep(store, t, P)
            for i in store.kept_ids():
                if i not in keep and store.kind[i] in ("auto", "save", "alert"):
                    store.kept[i] = False
                    removed_this_hour += 1
            ids, K, held, excl = store.account()
            # the disk policy
            while True:
                free = host_size - host_other - held
                L = limit_blocks(P, free, held)
                if held <= L:
                    break
                j = pick_victim(store, ids, K, excl, t, P)
                if j is None:
                    break
                i = ids[j]
                store.kept[i] = False
                out["removed_by_limit"] += 1
                removed_this_hour += 1
                if t - K[j] < P["daily_days"] * DAY:
                    out["removed_young"] += 1
                ids, K, held, excl = store.account()
            free = host_size - host_other - held
            out["min_free_gib"] = min(out["min_free_gib"], free / GIB)
            if free < H["alert_gib"] * GIB:
                out["alert_floor_hits"] += 1
            if free < H["pause_gib"] * GIB:
                out["pause_floor_hits"] += 1
            out["max_held_gib"] = max(out["max_held_gib"], held / GIB)
        if m // int(HOUR) != hour:
            out["max_removed_hour"] = max(out["max_removed_hour"], removed_this_hour)
            removed_this_hour, hour = 0, m // int(HOUR)
        if mod == 8 * 60 and day >= 1:
            ids, K, held, _ = store.account()
            night_start, night_end = t - 10 * HOUR, t - 2 * HOUR
            ever = [x for x in store.t if night_start <= x <= night_end]
            kept = [x for x in K if night_start <= x <= night_end]
            autos = [store.t[i] for i in ids if store.kind[i] == "auto"]
            reach = (t - autos[0]) / DAY
            held_back = sum(1 for i in ids if store.kind[i] != "auto")
            # how far the nearest kept checkpoint at or before a moment is from it
            A = np.array(autos)
            def gap(ago_d):
                target = t - ago_d * DAY
                k = A[A <= target]
                return None if len(k) == 0 else round((target - k[-1]) / HOUR, 1)
            out["mornings"].append({
                "night": int(day), "held_gib": round(held / GIB, 1),
                "kept": len(K), "reach_d": round(reach, 1), "not_auto_kept": held_back,
                "night_whole": len(kept) == len(ever),
                "gap_h": {str(a): gap(a) for a in (1, 3, 7, 14, 28)},
                "free_gib": round((host_size - host_other - held) / GIB, 1),
            })
    out["min_free_gib"] = round(out["min_free_gib"], 1)
    out["max_held_gib"] = round(out["max_held_gib"], 1)
    return out


# ---------------------------------------------------------------- the grid

WORK = {
    "light": dict(day_gib_h=0.5, night_gib_h=0.5, idle_gib_h=0.01, working_set_gib=4,
                  guest="linux-trim", big_rewrite_gib=2),
    "medium": dict(day_gib_h=2, night_gib_h=2, idle_gib_h=0.02, working_set_gib=8,
                   guest="linux-trim", big_rewrite_gib=6),
    "heavy": dict(day_gib_h=5, night_gib_h=5, idle_gib_h=0.05, working_set_gib=16,
                  guest="linux-trim", big_rewrite_gib=12),
    "macos-no-trim": dict(day_gib_h=2, night_gib_h=2, idle_gib_h=0.02, working_set_gib=8,
                          guest="macos-no-trim", big_rewrite_gib=6),
}
for w in WORK.values():
    w.update(disk_gib=96, used_gib=40)

HOSTS = {
    # a 512 GB MacBook: 460 GiB volume; macOS, apps, the user's files and main's
    # live image take 260 GiB, so 200 GiB are free before any checkpoint.
    "mac-512": dict(volume_gib=460, other_used_gib=260, alert_gib=20, pause_gib=5,
                    events=[(12, 40), (20, -40)]),
    "mac-1tb": dict(volume_gib=920, other_used_gib=420, alert_gib=20, pause_gib=5,
                    events=[(12, 40), (20, -40)]),
}

BASE_POLICY = dict(whole_h=24, hourly_days=2, daily_days=30, day_starts_at_h=0,
                   limit_takes_hand=False, order="oldest",
                   limit_gib=None, limit_share=None, floor_gib=None)

POLICIES = {
    # the plan before #376: 2 hours whole, no limit
    "earlier-2h-no-limit": dict(whole_h=2),
    # #376's answer with no limit set: what 30 days cost
    "30d-no-limit": dict(),
    # #370's spec: 7 days of dailies, no limit
    "7d-no-limit": dict(daily_days=7),
    # #376's answer with a limit: oldest first
    "30d-half-oldest": dict(limit_share=0.5, floor_gib=50),
    # the same limit, removing so the kept ones thin evenly by age
    "30d-half-even": dict(limit_share=0.5, floor_gib=50, order="even"),
    # the same limit, removing what frees most for the least precision lost
    "30d-half-cheapest": dict(limit_share=0.5, floor_gib=50, order="cheapest"),
    # a fixed 100 GiB limit, oldest first
    "30d-100g-oldest": dict(limit_gib=100),
    # only a floor: checkpoints may take everything above 50 GiB free
    "30d-floor-only-oldest": dict(floor_gib=50),
    "30d-floor-only-even": dict(floor_gib=50, order="even"),
    # dailies taken at the first checkpoint after 07:00, the end of the night
    "30d-half-even-morning": dict(limit_share=0.5, floor_gib=50, order="even",
                                  day_starts_at_h=7),
}


def scenario(work, host, policy, **kw):
    P = dict(BASE_POLICY); P.update(POLICIES[policy])
    S = dict(seed=1, nights=30, interval_min=5, thin_every_min=15,
             work=WORK[work], host=HOSTS[host], policy=P,
             restore_nights=[7, 19], saves_set_aside_h=36,
             alert_nights=[3, 5, 9, 14, 16, 22, 25, 28], alerts_forgotten=0.0,
             big_rewrite_nights=[4, 11, 18, 25], hand_before_bed=False)
    S.update(kw)
    S["name"] = f"{work}/{host}/{policy}" + "".join(f"/{k}={v}" for k, v in kw.items())
    return S


def grid():
    for host in HOSTS:
        for work in WORK:
            for pol in POLICIES:
                yield scenario(work, host, pol)
    # checkpoints at each turn: one a minute while agents work
    for pol in ("30d-half-oldest", "30d-half-even", "30d-no-limit"):
        yield scenario("medium", "mac-512", pol, interval_min=1)
    # a checkpoint by hand before bed, every night, never thinned by age
    for pol in ("30d-half-oldest", "30d-half-even"):
        yield scenario("heavy", "mac-512", pol, hand_before_bed=True)
    # checkpoints kept whatever their age and never set aside: the two saves before a
    # restore, and one alert in four forgotten
    for work in ("light", "medium"):
        for pol in ("30d-half-oldest", "30d-no-limit"):
            yield scenario(work, "mac-1tb", pol, saves_set_aside_h=None, alerts_forgotten=0.25)
    # 90 nights: the steady state past the 30-day reach
    for pol in ("30d-half-oldest", "30d-half-even", "30d-no-limit"):
        yield scenario("medium", "mac-1tb", pol, nights=90)
    # other seeds, to see the spread
    for seed in (2, 3):
        for pol in ("30d-half-oldest", "30d-half-even"):
            yield scenario("medium", "mac-512", pol, seed=seed)


def summarize(S, out):
    M = out["mornings"]
    last = M[-1]
    return {
        "name": S["name"],
        "max_held_gib": out["max_held_gib"],
        "min_free_gib": out["min_free_gib"],
        "alert_floor_minutes": out["alert_floor_hits"] * S["thin_every_min"],
        "pause_floor_minutes": out["pause_floor_hits"] * S["thin_every_min"],
        "reach_last_morning_d": last["reach_d"],
        "min_reach_d_after_night_10": min((x["reach_d"] for x in M if x["night"] >= 10), default=None),
        "nights_whole_at_morning": sum(x["night_whole"] for x in M),
        "mornings": len(M),
        "kept_last": last["kept"],
        "not_auto_kept_last": last["not_auto_kept"],
        "gap_h_last": last["gap_h"],
        "removed_by_limit": out["removed_by_limit"],
        "removed_younger_than_reach": out["removed_young"],
        "max_removed_in_an_hour": out["max_removed_hour"],
        "held_by_morning": [x["held_gib"] for x in M],
        "reach_by_morning": [x["reach_d"] for x in M],
    }


if __name__ == "__main__":
    want = [a for a in sys.argv[1:] if not a.startswith("--part=")]
    part = [a for a in sys.argv[1:] if a.startswith("--part=")]
    k, n = map(int, part[0][7:].split("/")) if part else (0, 1)
    for idx, S in enumerate(grid()):
        if idx % n != k or (want and not any(w in S["name"] for w in want)):
            continue
        print(json.dumps(summarize(S, run(S))), flush=True)
