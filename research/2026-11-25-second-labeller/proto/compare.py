"""Compares #383's labels of 1,085 documented writes (A) with a second, blind labelling (B) and,
when present, the settled labels (S), then scores #353's, #383's blind and #383's tuned rules
against each label set.

Inputs: #383's proto (../../2026-11-25-destructive-calls-platforms/proto, which needs #353's
detector, both on the integration branch), labels-b/*.jsonl, and settled.jsonl if present.
Writes results/compare.json and results/tables.md. Run: python3 compare.py
"""
import collections, glob, json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
P383 = os.path.normpath(os.path.join(HERE, "..", "..", "2026-11-25-destructive-calls-platforms", "proto"))
sys.path.insert(0, P383)
sys.path.insert(0, os.path.join(P383, "surface"))
import surface  # noqa: E402
import score  # noqa: E402

COARSE = lambda l: l.split(":")[0]


def load_jsonl(paths):
    out = {}
    for p in paths:
        for line in open(p):
            if line.strip():
                r = json.loads(line)
                out[r["id"]] = r
    return out


def kappa(pairs):
    n = len(pairs)
    po = sum(a == b for a, b in pairs) / n
    ca, cb = collections.Counter(a for a, _ in pairs), collections.Counter(b for _, b in pairs)
    pe = sum(ca[k] * cb[k] for k in set(ca) | set(cb)) / (n * n)
    return po, (po - pe) / (1 - pe)


def rule_scores(rows_by_set, labels):
    """For each rule set: held of hold, held of allow, held of either and deploy, under `labels`."""
    out = {}
    for name, rows in rows_by_set.items():
        c = collections.Counter()
        for r in rows:
            lab = COARSE(labels[r["id"]])
            c[lab + "_n"] += 1
            c[lab + "_held"] += r["got"] == "hold"
        out[name] = dict(c)
    return out


def main():
    cases = surface.cases()
    A = {c["id"]: c["label"] for c in cases}
    Awhy = {c["id"]: c["why"] for c in cases}
    Brec = load_jsonl(sorted(glob.glob(os.path.join(HERE, "labels-b", "*.jsonl"))))
    assert set(Brec) == set(A), (len(set(A) - set(Brec)), len(set(Brec) - set(A)))
    B = {k: v["label"] for k, v in Brec.items()}
    spath = os.path.join(HERE, "settled.jsonl")
    S = {k: v["label"] for k, v in load_jsonl([spath]).items()} if os.path.exists(spath) else None
    ppath = os.path.join(HERE, "settled-proposed.jsonl")
    SP = {k: v["label"] for k, v in load_jsonl([ppath]).items()} if os.path.exists(ppath) else None

    res = {"n": len(A)}
    ids = sorted(A)
    res["exact"], res["kappa_exact"] = kappa([(A[i], B[i]) for i in ids])
    res["coarse"], res["kappa_coarse"] = kappa([(COARSE(A[i]), COARSE(B[i])) for i in ids])
    hb = lambda l: "hold" if l.startswith("hold") else "not held"
    res["held_or_not"], res["kappa_held_or_not"] = kappa([(hb(A[i]), hb(B[i])) for i in ids])
    res["confusion"] = collections.Counter("%s -> %s" % (A[i], B[i]) for i in ids if A[i] != B[i]).most_common()
    res["counts_a"] = collections.Counter(A.values()).most_common()
    res["counts_b"] = collections.Counter(B.values()).most_common()
    res["named_b"] = sum(1 for v in Brec.values() if v.get("named"))
    res["exposed_a_named_b"] = sum(1 for i in ids if A[i] == "hold:exposed" and Brec[i].get("named"))
    res["exposed_a_exposed_b"] = sum(1 for i in ids if A[i] == "hold:exposed" and B[i] == "hold:exposed")
    if S:
        res["a_vs_settled"] = kappa([(A[i], S[i]) for i in ids])[0]
        res["b_vs_settled"] = kappa([(B[i], S[i]) for i in ids])[0]
        res["counts_s"] = collections.Counter(S.values()).most_common()

    rows = {}
    for name, path in (("#353", score.OLD_RULES), ("blind", score.BLIND_RULES), ("tuned", score.NEW_RULES)):
        _, rows[name] = score.score(score.load(path), cases)
    res["rules_under_a"] = rule_scores(rows, A)
    res["rules_under_b"] = rule_scores(rows, B)
    if S:
        res["rules_under_s"] = rule_scores(rows, S)
    if SP:
        res["rules_under_s_proposed"] = rule_scores(rows, SP)
        res["counts_s_proposed"] = collections.Counter(SP.values()).most_common()
        res["a_vs_settled_proposed"] = kappa([(A[i], SP[i]) for i in ids])[0]
        res["b_vs_settled_proposed"] = kappa([(B[i], SP[i]) for i in ids])[0]
    # disagreements that change whether a call is held
    res["held_differs"] = sum(1 for i in ids if hb(A[i]) != hb(B[i]))
    res["held_differs_kinds"] = collections.Counter("%s -> %s" % (A[i], B[i]) for i in ids if hb(A[i]) != hb(B[i])).most_common()
    if S:
        res["held_differs_a_vs_s"] = collections.Counter("%s -> %s" % (A[i], S[i]) for i in ids if hb(A[i]) != hb(S[i])).most_common()
    got = {r["id"]: r["got"] for r in rows["tuned"]}

    dis = []
    for i in ids:
        if A[i] != B[i]:
            dis.append({"id": i, "a": A[i], "a_why": Awhy[i], "b": B[i], "b_why": Brec[i].get("why", ""),
                        "b_named": Brec[i].get("named"), "tuned_rules": got[i],
                        "settled": S[i] if S else None})
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    json.dump(res, open(os.path.join(HERE, "results", "compare.json"), "w"), indent=1)
    with open(os.path.join(HERE, "results", "disagreements.jsonl"), "w") as f:
        for d in dis:
            f.write(json.dumps(d) + "\n")
    print(json.dumps({k: v for k, v in res.items() if k not in ("confusion",)}, indent=1))
    print("disagreements:", len(dis))
    for k, v in res["confusion"][:40]:
        print(v, k)


if __name__ == "__main__":
    main()
