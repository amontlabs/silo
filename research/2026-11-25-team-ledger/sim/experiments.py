"""Runs every table of the README and writes them to ../out/tables.md and ../out/runs.jsonl.

python3 experiments.py [seeds]   (20 seeds by default)
"""
import json, os, statistics, sys

sys.path.insert(0, os.path.dirname(__file__))
from sim import run_one  # noqa: E402

SEEDS = range(1, 1 + (int(sys.argv[1]) if len(sys.argv) > 1 else 20))
OUT = os.path.join(os.path.dirname(__file__), "..", "out")
os.makedirs(OUT, exist_ok=True)
RUNS = open(os.path.join(OUT, "runs.jsonl"), "w")
TABLES = []

TEAM5 = ["heavy", "heavy", "medium", "light", "idle"]
TEAM12 = ["heavy"] * 4 + ["medium"] * 4 + ["light"] * 3 + ["idle"]


def runs(label, way, members, budget, policy, tps, **kw):
    rs = []
    for s in SEEDS:
        r = run_one(way, members, budget, policy, s, tps, 6 * 3600, **dict(kw))
        r["label"] = label
        RUNS.write(json.dumps(r) + "\n")
        rs.append(r)
    return rs


def row(label, rs):
    sp = [r["spent"] / r["budget"] for r in rs]
    over = max(r["over_pct"] for r in rs)
    under = min(r["over_pct"] for r in rs)
    return "| %s | %.2f | %+.1f%% | %+.1f%% | %.0f | %.0f | %.1f | %.1f | %.2f |" % (
        label, statistics.mean(sp), over, under, statistics.mean(r["calls"] for r in rs),
        max(r["wait_p99_ms"] for r in rs), max(r["wait_max_s"] for r in rs),
        statistics.mean(r["waited_10s"] for r in rs),
        statistics.mean(r["messages_per_call"] for r in rs))


HEAD = ("| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |\n"
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|")


def table(title, lines):
    TABLES.append("### " + title + "\n\n" + HEAD + "\n" + "\n".join(lines) + "\n")
    print(TABLES[-1], flush=True)


# 1. The simulator against #374's measured run: one Mac, Claude Code with 8 subagents, $3 on Opus.
lines = []
for p in ["finish", "finish_tps", "admit_input", "reserve_typical", "reserve_max"]:
    lines.append(row(p, runs("calibration", "exact", ["heavy"], 3.0, p, 500, )))
table("One Mac, $3, stand-in speed (500 tokens/s): the simulator against #374", lines)

WAYS = [
    ("exact: one boundary sees all", "exact", {}),
    ("central ledger, 1 ms (a Linux host on the LAN)", "central", {"rtt": 0.001}),
    ("central ledger, 30 ms (a host over a VPN, or one region)", "central", {"rtt": 0.03}),
    ("central ledger, 150 ms (another continent)", "central", {"rtt": 0.15}),
    ("central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms", "central", {"rtt": [0, 0, 0.03, 0.03, 0.03]}),
    ("leases, 30 ms", "lease", {"rtt": 0.03}),
    ("leases, 30 ms, no recall", "lease", {"rtt": 0.03, "recall": False}),
    ("leases, 2 s (a Git push as the ledger)", "lease", {"rtt": 2.0}),
    ("gossip every 1 s", "gossip", {"every": 1.0}),
    ("gossip every 10 s", "gossip", {"every": 10.0}),
    ("static fifths", "static", {}),
]

for tps in (500, 50):
    for policy in ("finish_tps", "reserve_typical", "finish"):
        lines = [row(label, runs(f"team5-{tps}-{policy}", w, TEAM5, 15.0, policy, tps, **kw)) for label, w, kw in WAYS]
        table(f"Team of 5 (2 heavy, 1 medium, 1 light, 1 idle), $15, {policy}, {tps} tokens/s", lines)

WAYS12 = [(l, w, kw) for l, w, kw in WAYS if not isinstance(kw.get("rtt"), list)]
for policy in ("finish_tps", "reserve_typical"):
    lines = [row(label, runs(f"team12-50-{policy}", w, TEAM12, 36.0, policy, 50, **kw)) for label, w, kw in WAYS12]
    table(f"Team of 12 (4 heavy, 4 medium, 3 light, 1 idle), $36, {policy}, 50 tokens/s", lines)

# Faults, at the real speed: the ledger cannot be reached for 10 minutes from the tenth minute, around when the budget runs out;
# or, with no ledger, the team splits in two for 10 minutes.
DOWN = [(600.0, 1200.0)]
FAULTS = [
    ("exact (no fault possible)", "exact", {}),
    ("central, calls wait for the ledger", "central", {"rtt": 0.03, "down": DOWN, "on_down": "hold"}),
    ("central, calls refused while it is down", "central", {"rtt": 0.03, "down": DOWN, "on_down": "deny"}),
    ("leases, the ledger down", "lease", {"rtt": 0.03, "down": DOWN}),
    ("gossip every 1 s, team cut in two", "gossip", {"every": 1.0, "cut": (600.0, 1200.0, {0, 2})}),
    ("gossip every 1 s, cut, escrow on silence", "gossip", {"every": 1.0, "cut": (600.0, 1200.0, {0, 2}), "split": True}),
]
for policy in ("finish_tps", "reserve_typical"):
    lines = [row(label, runs(f"fault-{policy}", w, TEAM5, 15.0, policy, 50, **kw)) for label, w, kw in FAULTS]
    EARLY = [(200.0, 800.0)]
    for label, w, kw in [("central, calls wait, ledger down from 200 s", "central", {"rtt": 0.03, "down": EARLY}),
                         ("leases, ledger down from 200 s", "lease", {"rtt": 0.03, "down": EARLY}),
                         ("central, calls wait, ledger down 1 minute from 300 s", "central", {"rtt": 0.03, "down": [(300.0, 360.0)]}),
                         ("leases, ledger down 1 minute from 300 s", "lease", {"rtt": 0.03, "down": [(300.0, 360.0)]}),
                         ("central, calls wait, ledger down 2 minutes from 300 s", "central", {"rtt": 0.03, "down": [(300.0, 420.0)]}),
                         ("leases, ledger down 2 minutes from 300 s", "lease", {"rtt": 0.03, "down": [(300.0, 420.0)]})]:
        lines.append(row(label, runs(f"fault-early-{policy}", w, TEAM5, 15.0, policy, 50, **kw)))
    table(f"Faults, team of 5, $15, {policy}, 50 tokens/s: 10 minutes without the ledger, or the team cut in two", lines)

# #385's counting: a call under way is cut once the budget is passed (`cut`), alone or behind the
# typical hold (`typical-cut`); and work out of sight is held at its whole bound.
CUTHEAD = ("| Way | Spent / budget, mean | Over, worst | Over, best | Paid for cut answers, mean | Calls done, mean | Messages per call |\n"
           "|---|---:|---:|---:|---:|---:|---:|")


def cutrow(label, rs):
    sp = [r["spent"] / r["budget"] for r in rs]
    return "| %s | %.2f | %+.1f%% | %+.1f%% | $%.2f | %.0f | %.2f |" % (
        label, statistics.mean(sp), max(r["over_pct"] for r in rs), min(r["over_pct"] for r in rs),
        statistics.mean(r["cut_paid"] for r in rs), statistics.mean(r["calls"] for r in rs),
        statistics.mean(r["messages_per_call"] for r in rs))


def cuttable(title, lines):
    TABLES.append("### " + title + "\n\n" + CUTHEAD + "\n" + "\n".join(lines) + "\n")
    print(TABLES[-1], flush=True)


lines = [cutrow(p + " and cut", runs("cal-cut", "exact", ["heavy"], 3.0, p, 500, cut_calls=True)) for p in ("finish_tps", "reserve_typical")]
cuttable("One Mac, $3, stand-in speed, with #385's cut: the simulator against #385", lines)

CUTWAYS = [w for w in WAYS if w[1] != "lease" or "recall" not in w[2]] + [
    ("gossip every 1 s, team cut in two", "gossip", {"every": 1.0, "cut": (600.0, 1200.0, {0, 2})}),
    ("gossip every 1 s, cut in two, escrow on silence", "gossip", {"every": 1.0, "cut": (600.0, 1200.0, {0, 2}), "split": True}),
    ("central, ledger down 10 minutes from 600 s, calls wait", "central", {"rtt": 0.03, "down": DOWN}),
    ("leases, ledger down 10 minutes from 600 s", "lease", {"rtt": 0.03, "down": DOWN}),
]
for policy, name in (("finish_tps", "cut"), ("reserve_typical", "typical-cut")):
    lines = [cutrow(label, runs(f"cut-{policy}", w, TEAM5, 15.0, policy, 50, cut_calls=True, **kw)) for label, w, kw in CUTWAYS]
    cuttable(f"Team of 5, $15, 50 tokens/s, #385's `{name}`: calls under way are cut once the budget is passed", lines)

# A batch: the medium member sends one at 120 s whose bound is $4; it costs $1.50 and its results
# come back at 1,500 s, after the budget has run out.
BATCH = {"mac": 2, "at": 120.0, "bound": 4.0, "cost": 1.5, "result_at": 1500.0}
BHEAD = ("| Way | Batch admitted, share of runs | Spent / budget, mean | Over, worst | Over, best | Calls done, mean |\n"
         "|---|---:|---:|---:|---:|---:|")
blines = []
for label, w, kw in [("one boundary", "exact", {}), ("central ledger, 30 ms", "central", {"rtt": 0.03}),
                     ("leases, 30 ms", "lease", {"rtt": 0.03}), ("gossip every 1 s", "gossip", {"every": 1.0}),
                     ("fixed fifths ($3 each)", "static", {})]:
    rs = runs("batch", w, TEAM5, 15.0, "reserve_typical", 50, cut_calls=True, batch=BATCH, **kw)
    sp = [r["spent"] / r["budget"] for r in rs]
    blines.append("| %s | %.0f%% | %.2f | %+.1f%% | %+.1f%% | %.0f |" % (
        label, 100 * statistics.mean(1 if r["batch_ok"] else 0 for r in rs), statistics.mean(sp),
        max(r["over_pct"] for r in rs), min(r["over_pct"] for r in rs), statistics.mean(r["calls"] for r in rs)))
TABLES.append("### Team of 5, $15, `typical-cut`, and one batch with a $4 bound held until its results (#385)\n\n" + BHEAD + "\n" + "\n".join(blines) + "\n")
print(TABLES[-1], flush=True)

open(os.path.join(OUT, "tables.md"), "w").write("\n".join(TABLES))
