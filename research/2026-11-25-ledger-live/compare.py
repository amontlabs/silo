"""The live runs (out/live.jsonl) beside the simulator of #388 run with the same settings, 20 seeds
each, and the Git measurements (out/git.jsonl). Writes out/tables.md.

python3 compare.py
"""
import json, os, statistics, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "2026-11-25-team-ledger", "sim"))
sys.path.insert(0, HERE)
import sim  # noqa: E402
from experiments import GROUPS  # noqa: E402

OUT = os.path.join(HERE, "out")
TEAM = ["heavy", "heavy", "medium", "light", "idle"]
SEEDS = range(1, 21)


def load(path):
    try:
        with open(path) as f:
            return [json.loads(l) for l in f if l.strip()]
    except OSError:
        return []


def batch_numbers(rs):
    """The batch's bound as the boundary priced it, and what the provider billed for it."""
    bounds, costs = [], []
    for r in rs:
        d = r["dir"]
        for f in os.listdir(d):
            if f.startswith("mac") and f.endswith(".jsonl"):
                for line in open(os.path.join(d, f)):
                    o = json.loads(line)
                    if "bound" in o:
                        bounds.append(o["bound"])
                    if "batch" in o and "cost" in o:
                        costs.append(o["cost"])
    return (statistics.mean(bounds) if bounds else None), (statistics.mean(costs) if costs else None)


def sim_of(cfg, rs):
    """The simulator's run for a live setting, or None where it has no model of it."""
    if cfg.get("sleep") or (cfg.get("partition") and len(cfg["partition"][2]) < 4):
        return None
    way = {"local": "exact", "central": "central", "lease": "lease"}[cfg["way"]]
    kw = {}
    if way != "exact":
        kw["rtt"] = 0.03
    span = cfg.get("kill") or (cfg.get("partition") or [None, None])[:2]
    if span[0] is not None:
        kw["down"] = [tuple(span)]
    if cfg.get("on_down") == "deny":
        kw["on_down"] = "deny"
    batch = None
    if cfg.get("batch"):
        bound, cost = batch_numbers(rs)
        mac, at, _, _, wait = cfg["batch"]
        batch = {"mac": 2, "at": at, "bound": bound or 4.0, "cost": cost or 1.5, "result_at": at + wait + 2}
    out = []
    for s in SEEDS:
        out.append(sim.run_one(way, TEAM, cfg.get("budget", 15.0), cfg["policy"], s, cfg.get("tps", 500.0), 7200,
                               cut_calls=cfg.get("cut", False), batch=batch, **kw))
    return out


def pct(x):
    return f"{x:+.1f}%"


def early_cuts(rs):
    """Under leases: cuts a Mac made because no answer came in time, not because the ledger said
    nothing was left. Per run."""
    n = 0
    for r in rs:
        d = r["dir"]
        if not os.path.isdir(d):
            return None
        for f in os.listdir(d):
            if f.startswith("mac") and f.endswith(".jsonl"):
                for line in open(os.path.join(d, f)):
                    o = json.loads(line)
                    n += o.get("cut") == "no answer"
    return n / len(rs)


def row_live(rs):
    sb = [r["billed"] / r["budget"] for r in rs]
    over = [r["over_pct"] for r in rs]
    return {"n": len(rs), "spent": statistics.mean(sb), "worst": max(over), "under": min(over),
            "cut": statistics.mean(r["cut_paid"] for r in rs), "calls": statistics.mean(r["calls_done"] for r in rs),
            "w10": statistics.mean(r["waited_10s"] for r in rs), "wmax": max(r["wait_max_s"] for r in rs),
            "msgs": statistics.mean(r["messages_per_call"] for r in rs),
            "cerr": max(abs(r["count_err_pct"] or 0) for r in rs), "early": early_cuts(rs) if rs[0]["way"] == "lease" else None,
            "batch": (sum(1 for r in rs if r.get("batch_admitted")), len(rs)) if any(r.get("batch_admitted") is not None for r in rs) else None}


def row_sim(rs):
    return {"spent": statistics.mean(r["spent"] / r["budget"] for r in rs), "worst": max(r["over_pct"] for r in rs),
            "under": min(r["over_pct"] for r in rs), "cut": statistics.mean(r["cut_paid"] for r in rs),
            "calls": statistics.mean(r["calls"] for r in rs), "w10": statistics.mean(r["waited_10s"] for r in rs),
            "wmax": max(r["wait_max_s"] for r in rs), "msgs": statistics.mean(r["messages_per_call"] for r in rs),
            "batch": (sum(1 for r in rs if r.get("batch_ok")), len(rs)) if any(r.get("batch_ok") is not None for r in rs) else None}


def main():
    live = load(os.path.join(OUT, "live.jsonl"))
    by = {}
    for r in live:
        by.setdefault(r["name"], []).append(r)
    cfgs = {}
    for g, items in GROUPS.items():
        for n, s, c in items:
            cfgs.setdefault(n, (g, c))
    lines = []
    for group, title in (("calib", "A team of five, $15, the stand-in's 500 tokens/s: live against the simulator"),
                         ("faults", "The ledger killed, Macs cut off from it: live against the simulator (typical-cut)"),
                         ("sleep", "A Mac that sleeps holding a slice (typical-cut); the simulator has no model of it"),
                         ("batch", "A batch held at its bound (typical-cut)"),
                         ("real", "A real model's speed, 50 tokens/s")):
        names = [n for n, (g, _) in cfgs.items() if g == group and n in by]
        if not names:
            continue
        lines.append(f"### {title}\n")
        lines.append("| Setting | Runs | Spent / budget | Over, worst | Under, worst | Paid for cut answers | Calls done | Calls that waited over 10 s | Longest wait, s | Messages per call | Cuts with no answer, per run | Sim: spent / budget | Sim: over, worst | Sim: under, worst | Sim: paid for cut answers | Sim: calls done | Sim: waited over 10 s | Sim: messages per call |")
        lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
        for n in names:
            rs = by[n]
            L = row_live(rs)
            S = sim_of(cfgs[n][1], rs)
            S = row_sim(S) if S else None
            b = f" (batch admitted {L['batch'][0]} of {L['batch'][1]}; sim {S['batch'][0]} of {S['batch'][1]})" if L["batch"] and S and S["batch"] else ""
            early = "" if L["early"] is None else f"{L['early']:.1f}"
            sim_cells = (f"{S['spent']:.3f} | {pct(S['worst'])} | {pct(S['under'])} | ${S['cut']:.2f} | {S['calls']:.0f} | {S['w10']:.1f} | {S['msgs']:.2f}"
                         if S else "n/a | | | | | |")
            lines.append(f"| {n}{b} | {L['n']} | {L['spent']:.3f} | {pct(L['worst'])} | {pct(L['under'])} | ${L['cut']:.2f} | {L['calls']:.0f} | "
                         f"{L['w10']:.1f} | {L['wmax']:.1f} | {L['msgs']:.2f} | {early} | {sim_cells} |")
        lines.append("")
    cerr = max((abs(r["count_err_pct"] or 0) for r in live if "batch" not in r["name"]), default=0)
    lines.append(f"Largest gap between what the boundaries counted and what the provider billed, in any run without a batch: {cerr:.2f}%. "
                 "In the batch runs the Macs' call totals leave the batch out; each batch was settled at what the provider billed for it, to the cent.\n")
    git = load(os.path.join(OUT, "git.jsonl"))
    if git:
        lines.append("### Leases through a Git branch\n")
        lines.append("| Where | Macs | Pace between asks | Asks | Pushes tried | Refused | Ask p50, ms | Ask p90, ms | Ask max, ms | Read p50, ms | Push p50, ms | Refused push p50, ms | Never past the budget |")
        lines.append("|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|")
        for g in git:
            pace = f"0 to {2 * g['pace_s']:.0f} s" if g["pace_s"] else "all at once"
            lines.append(f"| {g['where']} | {g['macs']} | {pace} | {g['asks']} | {g['pushes_tried']} | {g['pushes_refused']} | {g['ask_p50_ms']} | "
                         f"{g['ask_p90_ms']} | {g['ask_max_ms']} | {g.get('read_p50_ms')} | {g.get('push_ok_p50_ms')} | {g.get('push_refused_p50_ms')} | "
                         f"{'yes' if g['ok'] else 'NO'} |")
        lines.append("")
    with open(os.path.join(OUT, "tables.md"), "w") as f:
        f.write("\n".join(lines))
    print("\n".join(lines))


if __name__ == "__main__":
    main()
