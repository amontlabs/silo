"""Writes ../details/captures.md from results/captures.json and the captures: each command, its
label, the writes it sent and what the blind rules, blind rules with the fallback, and tuned rules decided.
Run after measure.py captures."""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "capture"))
import labels  # noqa: E402

res = json.load(open(os.path.join(HERE, "results", "captures.json")))
got = {k: {r["id"]: r for r in res[k]["rows"]} for k in ("fallback", "blind", "blind+fb", "tuned")}
cmds = labels.commands()
byc = {}
for c in labels.cases():
    if c["step"]:
        byc.setdefault(c["command"], []).append(c)
out = ["# The CLI captures, command by command", "",
       "Each command of `proto/capture/commands.tsv`, its label, each write it sent (reads are left out)",
       "and the decision of the fallback alone, the blind rules, the blind rules with the fallback, and the",
       "tuned rules. **hold** is a hold; a dash is allow. A mark after a decision: `!` it disagrees with the label.", ""]
for cli in ("aws", "gcloud", "az", "gh", "kubectl"):
    out += ["## " + cli, "", "| Command | Label | Write | fallback | blind | blind+fb | tuned |", "|---|---|---|---|---|---|---|"]
    for name, c in cmds.items():
        if c["cli"] != cli:
            continue
        ws = byc.get(name)
        if not ws:
            why = labels.UNREACHED.get(name, "a read only")
            out.append("| `%s` | %s | (%s) | | | | |" % (name, c["label"], why))
            continue
        for w in ws:
            r = w["req"]
            cells = []
            for k in ("fallback", "blind", "blind+fb", "tuned"):
                g = got[k][w["id"]]["got"]
                bad = (w["label"].startswith("hold") and g != "hold") or (w["label"] == "allow" and g == "hold")
                cells.append(("**hold**" if g == "hold" else "-") + (" !" if bad else ""))
            out.append("| `%s` | %s | `%s %s%s` | %s |" % (name, w["label"], r["method"], r["host"].split(".")[0],
                                                          r["target"].split("?")[0][:70], " | ".join(cells)))
    out.append("")
open(os.path.join(HERE, "..", "details", "captures.md"), "w").write("\n".join(out))
print("ok")
