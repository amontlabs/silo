#!/usr/bin/env python3
"""Simulators per agent inside main: what fits in memory, and how long agents wait.

Two parts.

1. Memory. Apple's framework gives a macOS computer a fixed memory size when it starts,
   and whether a macOS computer gives any back through the traditional balloon is not
   known (`memory-given-back`). So main's memory is set before the night starts, and
   everything the agents run inside main must fit in it. For each Mac this computes
   the largest number of simulators booted at once that fits, for a booted simulator
   between 0.9 GB (one vendor's figure) and 3.4 GB (xcfs's figure).

2. Waiting. N agents each work in a loop: think and edit, build, then, on some turns,
   run the app in their own simulator. At most K simulators are booted at once. Two
   ways to share them:
   - "session": an agent keeps its simulator booted from its first use to the end of
     the night (a lease per agent);
   - "use": an agent holds a slot only while it runs the app; its simulator stays
     booted after, and is shut down only when another agent needs the slot and it is
     the one least recently used (a lease per use, kept warm).
   An agent whose simulator is not booted pays a boot. Prints how long agents wait.

Usage: simulators.py memory | simulators.py wait
"""
import heapq, json, sys
import numpy as np

# ---------------------------------------------------------------- memory (GB)

MACS = [16, 24, 32, 48, 64, 128]
HOST_KEEPS = {16: 6, 24: 7, 32: 8, 48: 8, 64: 10, 128: 12}   # macOS, browser, editor, Silo
BESIDE = 2.0             # one Linux computer beside main (a fork or a task), idle-ish
MAIN_BASE = 3.0          # a macOS computer with nobody on its screen, booted
PER_AGENT = 0.6          # an agent program (Node or Rust) with its tools, no build
LSP = 1.0                # sourcekit-lsp for one worktree, shared by agents on it
BUILD_PEAK = 4.0         # one Xcode build at its peak (xcfs: 21-26 GB to warm three)
APP = 0.3                # the app under test, inside its simulator
SIM = [0.9, 1.5, 3.4]    # one booted simulator: vendor figure, middle, xcfs


def memory():
    rows = []
    for mac in MACS:
        main = mac - HOST_KEEPS[mac] - BESIDE
        for agents, worktrees, builds in ((4, 2, 1), (8, 3, 2), (16, 4, 3)):
            fixed = MAIN_BASE + agents * PER_AGENT + worktrees * LSP + builds * BUILD_PEAK
            row = {"mac_gb": mac, "main_gb": main, "agents": agents,
                   "builds_at_once": builds, "fixed_gb": round(fixed, 1)}
            for s in SIM:
                row[f"sims_at_{s}gb"] = max(0, int((main - fixed) // (s + APP)))
            rows.append(row)
    return rows


# ---------------------------------------------------------------- waiting

def wait(N, K, lease, seed=1, hours=8.0, think=180.0, build=60.0, use=90.0,
         p_use=0.5, boot=25.0):
    """Event simulation of one night. Times in seconds."""
    rng = np.random.default_rng(seed)
    end = hours * 3600
    booted = {}          # agent -> last time its simulator was used (booted ones)
    busy = set()         # agents running their app now
    queue = []           # agents waiting for a slot, in order
    waits, boots = [], 0
    ev = []              # (time, kind, agent)
    for a in range(N):
        heapq.heappush(ev, (rng.exponential(think), "built", a))
    asked = {}

    def try_grant(t):
        nonlocal boots
        granted = []
        for a in list(queue):
            if a in booted:
                ok, cost = True, 0.0
            elif len(booted) < K:
                ok, cost = True, boot
            else:
                # a booted simulator nobody runs and whose owner may lose it
                idle = [b for b in booted if b not in busy and
                        (lease == "use")]
                if idle:
                    victim = min(idle, key=lambda b: booted[b])
                    del booted[victim]
                    ok, cost = True, boot
                else:
                    ok = False
            if ok:
                queue.remove(a)
                if cost:
                    boots += 1
                booted[a] = t
                busy.add(a)
                waits.append(t - asked[a] + cost)
                heapq.heappush(ev, (t + cost + rng.exponential(use), "done", a))
                granted.append(a)
        return granted

    while ev:
        t, kind, a = heapq.heappop(ev)
        if t > end:
            break
        if kind == "built":
            # the agent thought, edited and built; does it look at the app?
            if rng.random() < p_use:
                asked[a] = t
                queue.append(a)
                try_grant(t)
            else:
                heapq.heappush(ev, (t + rng.exponential(think) + rng.exponential(build),
                                    "built", a))
        elif kind == "done":
            busy.discard(a)
            booted[a] = t
            heapq.heappush(ev, (t + rng.exponential(think) + rng.exponential(build),
                                "built", a))
            try_grant(t)
    # an agent still waiting at the end of the night waited until then
    waits += [end - asked[a] for a in queue]
    w = np.array(waits) if waits else np.zeros(1)
    return {"agents": N, "booted_max": K, "lease": lease, "uses": len(waits),
            "wait_mean_s": round(float(w.mean()), 1),
            "wait_p95_s": round(float(np.percentile(w, 95)), 1),
            "wait_max_s": round(float(w.max()), 1),
            "boots": boots,
            "share_of_agent_time_waiting": round(float(w.sum()) / (N * end), 3),
            "starved_agents": sum(1 for a in range(N) if a in queue)}


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "memory"
    if what == "memory":
        for r in memory():
            print(json.dumps(r))
    else:
        for N in (4, 8, 16):
            for K in (1, 2, 3, 4, 6, 8):
                if K > N:
                    continue
                for lease in ("session", "use"):
                    print(json.dumps(wait(N, K, lease)))
        # sensitivity: agents that look at the app on every turn, and longer UI tests
        for kw in (dict(p_use=1.0), dict(use=300.0), dict(think=60.0)):
            for K in (2, 4, 6):
                r = wait(16, K, "use", **kw); r.update(kw); print(json.dumps(r))
