"""Measures rule sets and the fallback on:
  surface      every documented write of AWS (20 services), Google Cloud (16 APIs), Azure (14 providers),
               GitHub (REST and GraphQL) and Kubernetes, labelled here (surface/)
  captures     what aws, gcloud, az, gh and kubectl sent for the commands of capture/commands.tsv
  s383         #383's surface of Railway, Fly, Vercel, Supabase and Cloudflare (labelled by #383)
  c383         #383's CLI captures
  corpus       #353's corpus and captures
Writes results/<set>.json and prints the tables. For the surface, only the mistakes are kept as rows.

Run: python3 measure.py [set ...] [--rules name,name]
"""
import json, os, sys, base64
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import score  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_RULES = ["353", "383", "fallback", "383+fb", "blind", "blind+fb", "tuned", "tuned+fb"]


def cases(which):
    if which == "surface":
        sys.path.insert(0, os.path.join(HERE, "surface"))
        import surface
        return surface.cases()
    if which == "captures":
        sys.path.insert(0, os.path.join(HERE, "capture"))
        import labels
        return labels.cases()
    if which in ("s383", "c383"):
        p = os.path.join(score.P383)
        if which == "s383":
            sys.path.insert(0, os.path.join(p, "surface"))
            import importlib
            surf = importlib.import_module("surface")
            if not hasattr(surf, "PROVIDERS") or "railway" not in surf.PROVIDERS:
                import importlib.util
                spec = importlib.util.spec_from_file_location("surface383", os.path.join(p, "surface", "surface.py"))
                surf = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(surf)
            return surf.cases()
        import importlib.util
        spec = importlib.util.spec_from_file_location("labels383", os.path.join(p, "capture", "labels.py"))
        m = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(m)
        return m.cases()
    if which == "corpus":
        sys.path.insert(0, score.OLD)
        import corpus_build, capture_labels
        out = []
        for c in corpus_build.CASES + capture_labels.cases():
            req = dict(c["req"])
            req["body"] = base64.b64decode(req.pop("body_b64", ""))
            out.append({"id": c["id"], "provider": c["family"] + "/" + c["kind"], "label": c["label"], "req": req})
        return out
    raise SystemExit("unknown set " + which)


def main(argv):
    rules = DEFAULT_RULES
    if "--rules" in argv:
        i = argv.index("--rules")
        rules = argv[i + 1].split(",")
        argv = argv[:i] + argv[i + 2:]
    for w in argv or ["surface", "captures", "s383", "c383", "corpus"]:
        cs = cases(w)
        out = {}
        for name in rules:
            if name.split("+")[0] in score.RULES and not os.path.exists(score.RULES[name.split("+")[0]]):
                continue
            agg, rows = score.score(score.load(name), cs)
            if w == "surface":   # 5,546 rows per rule set: keep only the mistakes
                rows = [r for r in rows if (r["label"].startswith("hold") and r["got"] != "hold")
                        or (r["label"] == "allow" and r["got"] == "hold")]
            out[name] = {"summary": agg, "rows": rows}
            print(score.table(agg, "\n%s, %s" % (w, name)))
        with open(os.path.join(HERE, "results", w + ".json"), "w") as f:
            json.dump(out, f, indent=1, default=str)


if __name__ == "__main__":
    main(sys.argv[1:])
