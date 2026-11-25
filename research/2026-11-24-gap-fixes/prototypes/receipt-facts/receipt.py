"""A plain-text receipt excerpt from facts.json: the test runs, what weakens checks, and the
agent's claim beside what the record shows. Each line says how Silo knows it. The look of the
receipt is another thread's work; this shows only what it holds.
Usage: python3 receipt.py <facts.json>
"""
import json, sys

TAG = {"fact": "seen", "report": "told by the probe", "opinion": "Silo's classifier thinks"}


def tag(x):
    return "[" + TAG[x["kind"]] + "]"


def main(path):
    d = json.load(open(path))
    out = []
    w = d["weakening"]["value"]
    if w:
        out.append("Changes that weaken checks " + tag(d["weakening"]))
        for x in w:
            where = x.get("path") or " ".join(x.get("argv", [])) or x.get("command", "")
            extra = f": {x['line']}" if x.get("line") else (f": {x['executed_per_run']}" if x.get("executed_per_run") else "")
            out.append(f"  - {x['what']}" + (f" ({where})" if where else "") + extra)
        out.append("")
    out.append("Test runs")
    tests = [t for c in d.get("commands", []) for t in c.get("tests", [])] or d.get("tests", [])
    cmds = {t["pid"]: c["command"]["value"] for c in d.get("commands", []) for t in c.get("tests", [])}
    for t in tests:
        cmd = cmds.get(t["pid"]) or t.get("command")
        out.append(f"  $ {cmd}   [seen: the command the model asked for]")
        if t["runner_exit"].get("value") is not None:
            line = f"    {t['runner']} ({' '.join(t['argv'])}): {t['outcome']['value']}, exit {t['runner_exit']['value']}"
            if t["masked"]["value"]:
                line += f"; the pipeline exited {t['pipeline_exit_without_pipefail']['value']}, so the agent saw no error"
            out.append(line + "   " + tag(t["runner_exit"]))
        else:
            out.append(f"    {t['outcome']['value']}   [seen: the output shown to the agent]")
            if t["masked"]["value"]:
                out.append("    the output says tests failed; the agent program reported the command as not failed   [seen]")
        if t["counts_shown"]["value"]:
            c = ", ".join(f"{v} {k}" for k, v in t["counts_shown"]["value"].items())
            out.append(f"    the output shown to the agent says: {c}   [seen; who printed it is not]")
        elif t["runner_exit"].get("value") is not None or True:
            out.append("    no test count in the output shown to the agent")
        if t["narrowed"]["value"]:
            sel = ", ".join(f"{s['selector']} {s.get('value', '')}".strip() + f" ({s['means']})" for s in t["narrowed"]["selectors"])
            out.append(f"    narrowed: {sel}   {tag(t['narrowed'])}")
        if t["passed"]:
            ch = t["changed_after"]["value"]
            if ch:
                files = ", ".join(c["path"] for c in ch)
                out.append(f"    STALE: changed after this pass: {files}   [seen: checkpoints {t['changed_after']['between'][0]} and {t['changed_after']['between'][1]}]")
                if t.get("changed_after_and_read", {}).get("value"):
                    out.append(f"    of these, the run read: {', '.join(t['changed_after_and_read']['value'])}   [told by the probe]")
            else:
                out.append("    nothing in the checkout changed after this pass   [seen]")
    out.append("")
    out.append("The agent said (its own words, beside the receipt):")
    out.append(f"  \"{d['claim']['value']}\"")
    m = d["claim_against_record"]
    out.append(f"Against the record: {m['value'].upper()}   {tag(m)}")
    for r in m.get("reasons", []):
        out.append(f"  - {r}")
    print("\n".join(out))


if __name__ == "__main__":
    main(sys.argv[1])
