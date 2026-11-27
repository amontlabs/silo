"""Measures #353's rules and this folder's rules on (1) every documented operation of the five
APIs (surface/), (2) the new CLI captures (capture/), (3) #353's own corpus, so nothing regresses.
Writes results/*.json and prints the tables.

Run: python3 measure.py [surface|captures|corpus ...]
"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import score  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))


def dump(name, obj):
    with open(os.path.join(HERE, "results", name + ".json"), "w") as f:
        json.dump(obj, f, indent=1, default=str)


def surface_cases():
    sys.path.insert(0, os.path.join(HERE, "surface"))
    import surface
    return surface.cases()


def main(which):
    rulesets = {"353": score.OLD_RULES, "blind": score.BLIND_RULES, "tuned": score.NEW_RULES}
    for w in which:
        if w == "surface":
            cases = surface_cases()
        elif w == "captures":
            sys.path.insert(0, os.path.join(HERE, "capture"))
            import labels as caplabels
            cases = caplabels.cases()
        elif w == "corpus":
            sys.path.insert(0, score.OLD)
            import corpus_build, capture_labels, base64
            cases = []
            for c in corpus_build.CASES + capture_labels.cases():  # CASES, not main(): main() rewrites #353's corpus.jsonl
                req = dict(c["req"])
                req["body"] = base64.b64decode(req.pop("body_b64", ""))
                cases.append({"id": c["id"], "provider": c["family"] + "/" + c["kind"], "label": c["label"], "req": req})
        out = {}
        for name, path in rulesets.items():
            if not os.path.exists(path):
                continue
            agg, rows = score.score(score.load(path), cases)
            out[name] = {"summary": agg, "rows": rows}
            print(score.table(agg, "\n%s, rules of %s" % (w, name)))
        dump(w, out)


if __name__ == "__main__":
    main(sys.argv[1:] or ["surface", "captures", "corpus"])
