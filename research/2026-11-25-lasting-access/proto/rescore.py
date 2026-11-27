"""Rescores #383's and #404's rules under three wordings of "exposed", with revocations as labelled
and with revocations passing, on both documented surfaces and on both sets of CLI captures.

Wordings:
  in-force  "readable by anyone" (plus a protection turned off, as the answer says)
  wide      #383's: "gives access or data to someone who did not have it"
  lasting   #405's: readable by anyone, or lasting access to someone who did not have it
Revocations: "labelled" keeps each set's label (#404 and #383 call most of them outage, #405's
settling calls members removed allow); "pass" makes every revocation allow (gone stays gone).

Label sources: #383's surface uses #405's settled labels (settled.jsonl is the in-force wording,
settled-proposed.jsonl the wide one); #404's surface uses #404's labels, which follow the wide
wording, and access.py's classes to derive the other two. Captures use their own labels and the
same classes.

Rules: #404's tuned rules ("tuned"), which decide #383's surface exactly as #383's rules do, and
the same rules with an overlay that passes the operations access.py classes as run, self or
revocation ("tuned+lasting"), to show what the narrower wording asks of a rule set.

Run from this folder: python3 -B rescore.py. Writes results/rescore.json and results/tables.md.
"""
import collections, json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
R = os.path.normpath(os.path.join(HERE, "..", ".."))
C404 = os.path.join(R, "2026-11-25-destructive-calls-clouds", "proto")
S405 = os.path.join(R, "2026-11-25-second-labeller", "proto")
sys.path.insert(0, HERE)
sys.path.insert(0, C404)
import access  # noqa: E402
import measure  # noqa: E402  #404's loaders
import score  # noqa: E402  #404's scorer and rule sets

GIVE = ("lasting",)          # held under the lasting wording besides anyone and protection
PASS_KEEP = ("hold:gone", "hold:money", "hold:recoverable", "hold:exposed")


def jl(path):
    return {r["id"]: r for r in (json.loads(l) for l in open(path) if l.strip())}


# ---------------------------------------------------------------- labels
def labels_p383(cases):
    S, SP = jl(os.path.join(S405, "settled.jsonl")), jl(os.path.join(S405, "settled-proposed.jsonl"))
    out = {}
    for c in cases:
        i = c["id"]
        l0, l1 = S[i]["label"], SP[i]["label"]
        cls = access.P_NAMED.get(i, (None,))[0]
        if l0 != l1:
            assert cls, i
        l2 = l1 if cls in GIVE else l0
        out[i] = {"in-force": l0, "wide": l1, "lasting": l2, "revoke": access.is_revoke_p(i), "class": cls}
    return out


def labels_c404(cases):
    out = {}
    for c in cases:
        lab, cls = c["label"], None
        l0 = l2 = lab
        if lab == "hold:exposed":
            cls = access.class_c(c["provider"], c["op"], c["why"])[0]
            if cls in ("lasting", "run"):
                l0 = "allow"
            if cls == "run":
                l2 = "allow"
        out[c["id"]] = {"in-force": l0, "wide": lab, "lasting": l2, "revoke": access.is_revoke_c(c["provider"], c["op"]), "class": cls}
    return out


# Captures: the commands whose label depends on the wording or is a revocation.
CAPTURE_CLASS = {
    "k-token": "run", "gh-collaborator-admin": "lasting", "gc-project-owner": "lasting",
    "fly-tokens-create": "lasting", "fly-orgs-invite": "lasting", "vc-teams-invite": "lasting", "vc-tokens-add": "lasting",
    "gh-member-remove": "revoke", "gh-deploy-key-delete": "revoke",
}


def labels_captures(cases):
    out = {}
    for c in cases:
        lab = c["label"]
        cls = CAPTURE_CLASS.get(c["id"].split(" ")[0]) if lab.startswith("hold") else None
        l0 = "allow" if cls in ("run", "lasting") and lab == "hold:exposed" else lab
        l2 = "allow" if cls == "run" else lab
        out[c["id"]] = {"in-force": l0, "wide": lab, "lasting": l2, "revoke": cls == "revoke",
                        "class": None if cls == "revoke" else cls}
    return out


def with_revocations(lab, how):
    l = lab["lasting"] if how is None else lab[how]
    return l


def final(lab, wording, revoke):
    l = lab[wording]
    if revoke == "pass" and lab["revoke"] and l not in PASS_KEEP:
        l = "allow"
    return l


# ---------------------------------------------------------------- the overlay
def template_re(path):
    p = re.sub(r"\{[^}]+\}", "[^/]+", re.escape(path).replace(r"\{", "{").replace(r"\}", "}"))
    p = re.sub(r"^/v\d+/", "/(v\\\\d+/)?", p)
    return re.compile("^" + p + r"(\?.*)?$")


class Overlay:
    """The rules, then: a request that matches an operation classed run, self or revocation passes."""

    def __init__(self, det, ids, rest, gql):
        self.det, self.ids, self.rest, self.gql = det, ids, rest, gql

    def check(self, req, cid=None):
        r = self.det.check(dict(req))
        if r["decision"] != "hold":
            return r
        hit = cid in self.ids if cid else False
        if not hit and req["method"] != "GET":
            body = req.get("body") or b""
            m = re.search(rb'mutation[^{]*\{\s*(\w+)', body)
            if m and m.group(1).decode() in self.gql:
                hit = True
            for meth, rx in self.rest:
                if meth == req["method"] and rx.match(req["target"]):
                    hit = True
        if hit:
            return {"decision": "pass", "verdict": "overlay", "says": ["passes: run-scoped, self or revocation"], "matches": []}
        return r


def overlay(det, cases_by_set, labels_by_set):
    ids, rest, gql = set(), [], set()
    for name, cases in cases_by_set.items():
        for c in cases:
            lab = labels_by_set[name][c["id"]]
            if lab["class"] in ("run", "self") or lab["revoke"]:
                ids.add(c["id"])
                op = c.get("op", "")
                if " " in op and op.split(" ")[0] in ("POST", "PUT", "PATCH", "DELETE"):
                    meth, path = op.split(" ", 1)
                    rest.append((meth, template_re(path)))
                elif c["req"]["target"].endswith("graphql") or "/graphql" in c["req"]["target"]:
                    gql.add(op.split(" ")[-1])
    return Overlay(det, ids, rest, gql)


# ---------------------------------------------------------------- scoring
def run(det, cases, labels, wording, revoke, by_id=False):
    c = collections.Counter()
    rows = []
    for x in cases:
        lab = final(labels[x["id"]], wording, revoke)
        got = (det.check(x["req"], x["id"]) if by_id else det.check(dict(x["req"])))["decision"]
        k = lab.split(":")[0]
        c[k + "_n"] += 1
        c[k + "_held"] += got == "hold"
        if (k == "hold" and got != "hold") or (k == "allow" and got == "hold"):
            rows.append({"id": x["id"], "label": lab, "got": got, "class": labels[x["id"]]["class"], "revoke": labels[x["id"]]["revoke"]})
    return dict(c), rows


def pct(a, b):
    return "%d of %d (%.0f%%)" % (a, b, 100.0 * a / b) if b else "-"


def main():
    c404 = measure.cases("surface")  # first: both folders name their module surface.py
    sets = {
        "#383 surface": measure.cases("s383"),
        "#404 surface": c404,
        "#383 captures": measure.cases("c383"),
        "#404 captures": measure.cases("captures"),
    }
    labels = {
        "#383 surface": labels_p383(sets["#383 surface"]),
        "#404 surface": labels_c404(sets["#404 surface"]),
        "#383 captures": labels_captures(sets["#383 captures"]),
        "#404 captures": labels_captures(sets["#404 captures"]),
    }
    tuned = score.load("tuned")
    ov = overlay(tuned, {k: v for k, v in sets.items() if "surface" in k}, labels)
    res = {"classes": {}, "scores": {}, "misses": {}}
    for name in sets:
        res["classes"][name] = {
            "access": collections.Counter(l["class"] for l in labels[name].values() if l["class"]),
            "revocations": collections.Counter(final(l, "wide", "labelled") for l in labels[name].values() if l["revoke"]),
        }
    lines = []
    for name, cases in sets.items():
        lines += ["", "### " + name, "",
                  "| Wording | Revocations | Rules | Should be held | Held | Should pass | Held wrongly |",
                  "|---|---|---|---|---|---|---|"]
        for wording in ("in-force", "wide", "lasting"):
            for revoke in ("labelled", "pass"):
                for rname, det, by_id in (("tuned", tuned, False), ("tuned+lasting", ov, True)):
                    if rname == "tuned+lasting" and (wording, revoke) != ("lasting", "pass"):
                        continue
                    if by_id:
                        cnt, rows = run(det, cases, labels[name], wording, revoke, by_id="surface" in name)
                    else:
                        cnt, rows = run(det, cases, labels[name], wording, revoke)
                    key = "%s|%s|%s|%s" % (name, wording, revoke, rname)
                    res["scores"][key] = cnt
                    res["misses"][key] = rows
                    lines.append("| %s | %s | %s | %d | %s | %d | %s |" % (
                        wording, revoke, rname, cnt.get("hold_n", 0), pct(cnt.get("hold_held", 0), cnt.get("hold_n", 0)),
                        cnt.get("allow_n", 0), pct(cnt.get("allow_held", 0), cnt.get("allow_n", 0))))
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    with open(os.path.join(HERE, "results", "rescore.json"), "w") as f:
        json.dump(res, f, indent=1, default=dict)
    with open(os.path.join(HERE, "results", "tables.md"), "w") as f:
        f.write("# Rescored tables (written by rescore.py)\n" + "\n".join(lines) + "\n")
    print("\n".join(lines))
    print(json.dumps(res["classes"], default=dict, indent=1))


if __name__ == "__main__":
    main()
