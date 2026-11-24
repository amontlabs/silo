"""Summarises results/runs.json into the tables of details/git-guard.md."""
import json, os, statistics, sys

HERE = os.path.dirname(os.path.abspath(__file__))
R = json.load(open(os.path.join(HERE, "results", sys.argv[1] if len(sys.argv) > 1 else "runs.json")))


def med(xs):
    xs = [x for x in xs if x is not None]
    return round(statistics.median(xs), 1) if xs else None


def by(scen, mode):
    return [r for r in R if r["scenario"] == scen and r["mode"] == mode]


scens = []
for r in R:
    if r["scenario"] not in scens:
        scens.append(r["scenario"])

print("## Classification (observe mode, all reps)\n")
print("| scenario | ref | expected | decision (c) | pack alone (a) | server alone (b) | server asks (c) | probe POST | pack objects / commits / deltified commits / unresolved |")
print("|---|---|---|---|---|---|---|---|---|")
for s in scens:
    obs = by(s, "observe") or by(s, "backup")
    if not obs:
        continue
    exp = obs[0]["expected"]
    for ref in exp:
        dec, a, b, c = set(), set(), set(), set()
        probe, packs = False, set()
        for r in obs:
            for g in r["guard"]:
                if g.get("probe"):
                    probe = True
                if ref in g.get("refs", {}):
                    i = g["refs"][ref]
                    dec.add(i["class"])
                    if "a_pack" in i:
                        a.add(i["a_pack"]["result"]); c.add(i["c_hybrid"]["server_asks"])
                    if "b_server" in i:
                        b.add(i["b_server"]["result"])
                    p = g.get("pack")
                    if p:
                        packs.add(f'{p["objects"]}/{p["commits"]}/{p["deltified_commits"]}/{p["unresolved"]}')
        ok = "yes" if dec == {exp[ref]} else f"NO {sorted(dec)}"
        print(f"| {s} | `{ref}` | {exp[ref]} | {','.join(sorted(dec)) or '(not seen)'} {'' if ok == 'yes' else ok} | {','.join(sorted(a)) or '-'} | {','.join(sorted(b)) or '-'} | {','.join(map(str, sorted(c))) or '-'} | {'yes' if probe else 'no'} | {','.join(sorted(packs)) or 'no pack'} |")

print("\n## Time (median over reps, ms)\n")
print("| scenario | wall pass | wall observe | wall backup | guard work in backup (parse+classify+backup) | classify | backup refs | pack MB |")
print("|---|---|---|---|---|---|---|---|")
for s in scens:
    def gw(mode, key):
        return med([g.get(key) for r in by(s, mode) for g in r["guard"] if not g.get("probe")])
    pb = med([g["body_len"] / 1e6 for r in by(s, "pass") for g in r["guard"] if not g.get("probe")])
    print(f"| {s} | {med([r['wall_ms'] for r in by(s, 'pass')])} | {med([r['wall_ms'] for r in by(s, 'observe')])} | "
          f"{med([r['wall_ms'] for r in by(s, 'backup')])} | {gw('backup', 'guard_ms')} | {gw('backup', 'classify_ms')} | "
          f"{gw('backup', 'backup_ms')} | {pb} |")

print("\n## Per-method time inside observe (median ms per force/ff ref)\n")
print("| scenario | ref | (a) pack | (b) server | (c) hybrid |")
print("|---|---|---|---|---|")
for s in scens:
    rows = {}
    for r in by(s, "observe"):
        for g in r["guard"]:
            for ref, i in g.get("refs", {}).items():
                if "a_pack" in i:
                    rows.setdefault(ref, []).append((i["a_pack"]["ms"], i["b_server"]["ms"], i["c_hybrid"]["ms"]))
    for ref, v in rows.items():
        print(f"| {s} | `{ref}` | {med([x[0] for x in v])} | {med([x[1] for x in v])} | {med([x[2] for x in v])} |")

print("\n## What the agent saw (refuse, split, hold) and what the server got\n")
for s in scens:
    for mode in ("refuse", "split", "hold", "backup"):
        for r in by(s, mode)[:1]:
            same = None
            g = [x for x in r["guard"] if not x.get("probe")]
            b = [x for x in r["backend"] if x["body_len"] > 4]
            if g and b and g[-1].get("answer") == "passed":
                same = g[-1]["received_sha256"] == b[-1]["body_sha256"]
            print(f"### {s} / {mode}: exit {r['exit']}, {r['wall_ms']} ms, answer {[x.get('answer') for x in g]}, "
                  f"byte-for-byte {same}, server refs changed {sorted(r['changed'])}")
            if mode in ("backup", "hold"):
                print(f"backups survive gc: {r.get('old_commit_survives_gc')}; mirror clone sees refs/silo: "
                      f"{r.get('mirror_clone_sees_silo_refs')}; ls-remote shows: {r.get('ls_remote_shows_silo_refs')}")
            print("```")
            print("\n".join(r["stderr"]))
            print("```")
