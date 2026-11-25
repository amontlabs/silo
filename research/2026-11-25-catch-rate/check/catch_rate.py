"""Catch rate of each approach, on the old corpus (development) and the fresh one (held out).

    python3 catch_rate.py [--cases] [--set dev|fresh|all] [--model <answers.json>]

dev    the 60 cases of research/2026-11-25-real-night/corpus/cases.json, with the asks and the
       order of events added from their issues (corpus/old-asked.json). phrases@0.4, receipt@2
       and asked@1 were written while reading these
fresh  corpus/fresh.json, gathered after every approach here was committed, by a collector told
       not to read any checker. Read once per version

A false claim is caught when an approach says mismatch, flagged when it says only unbacked, and
missed otherwise. On a true claim, a mismatch is a false alarm and an unbacked is noise.
"""
import collections, json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import approaches  # noqa: E402

ROOT = os.path.join(HERE, "..", "..")


def load(name):
    if name == "dev":
        cases = json.load(open(os.path.join(ROOT, "2026-11-25-real-night", "corpus", "cases.json")))
        p = os.path.join(HERE, "..", "corpus", "old-asked.json")
        extra = json.load(open(p)) if os.path.exists(p) else {}
        for c in cases:
            e = extra.get(c["id"], {})
            c["asked"], c["order"] = e.get("asked"), e.get("order")
        return cases
    cases = json.load(open(os.path.join(HERE, "..", "corpus", "fresh.json")))
    for c in cases:
        c["order"] = None
    return cases


def outcome(case, verdicts):
    says = [v["says"] for v in verdicts]
    if case["truth"] == "true":
        return "false alarm" if "mismatch" in says else "noise" if "unbacked" in says else "quiet"
    return "caught" if "mismatch" in says else "flagged" if "unbacked" in says else "missed"


def model_verdicts(answers):
    """A model reader's answers, as verdicts: {id: {"says": ..., ...}}."""
    return lambda case, ev: [{"says": answers[case["id"]]["says"]}] if case["id"] in answers else []


def run(sets, show=False, extra=None):
    names = dict(approaches.APPROACHES)
    for k, f in (extra or {}).items():
        names[k] = f
    table = collections.defaultdict(collections.Counter)
    for set_name in sets:
        for case in load(set_name):
            ev = approaches.evidence(case, case.get("order"))
            row = {}
            for name, f in names.items():
                row[name] = outcome(case, f(case, ev))
            if show:
                print(f"{case['id']:<24}{set_name:<6}{case['truth']:<8}{case['outside_record_shows_it']:<8}" + " ".join(f"{row[n][:7]:<8}" for n in names) + f" {case['claim'][:50]!r}")
            if case["truth"] == "unclear":
                continue
            scopes = [set_name, f"{set_name}, record shows it: {case['outside_record_shows_it']}", f"{set_name}, {'with' if case.get('asked') else 'no'} ask", f"{set_name}, {'verbatim' if case.get('claim_verbatim') else 'paraphrased'}"]
            for sc in scopes:
                for n, o in row.items():
                    table[(case["truth"], sc)][(n, o)] += 1
    return names, table


def report(names, table):
    lines = []
    w = max(len(n) for n in names) + 2
    scopes = sorted({sc for (_, sc) in table})
    lines.append("False claims caught + flagged / n (caught: mismatch; flagged: unbacked only)")
    lines.append(f"{'':<34}" + "".join(f"{n:<{w}}" for n in names))
    for sc in scopes:
        c = table.get(("false", sc))
        if not c:
            continue
        n = sum(v for (nm, o), v in c.items() if nm == next(iter(names)))
        lines.append(f"{sc:<28}{n:>4}  " + "".join(f"{str(c[(nm, 'caught')]) + ' + ' + str(c[(nm, 'flagged')]):<{w}}" for nm in names))
    lines.append("")
    lines.append("True claims: false alarms (mismatch) + noise (unbacked) / n")
    for sc in scopes:
        c = table.get(("true", sc))
        if not c or "," in sc:
            continue
        n = sum(v for (nm, o), v in c.items() if nm == next(iter(names)))
        lines.append(f"{sc:<28}{n:>4}  " + "".join(f"{str(c[(nm, 'false alarm')]) + ' + ' + str(c[(nm, 'noise')]):<{w}}" for nm in names))
    return "\n".join(lines)


def main():
    args = sys.argv[1:]
    which = args[args.index("--set") + 1] if "--set" in args else "dev"
    sets = ["dev", "fresh"] if which == "all" else [which]
    extra = {}
    if "--model" in args:
        for p in args[args.index("--model") + 1].split(","):
            ans = json.load(open(p))
            extra[os.path.basename(p).removesuffix(".json")] = model_verdicts(ans)
    names, table = run(sets, "--cases" in args, extra)
    print(report(names, table))


if __name__ == "__main__":
    main()
