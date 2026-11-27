"""The ways to judge a whole-document access write, side by side on the same traffic.

One Judge per computer. It sees every request and its answer in order (observe), and for each write
of an access document gives every option's decision (decide). fetch(reqs) sends Silo's own reads and
returns the answers; it is called only by the options that read.

  A  today: #404's tuned rules with its fallback behind, on the request alone
  B  powerful anywhere: hold when the document sent holds any public or powerful grant
  C  every write: hold every whole-document access write
  D  the read relayed: compare with the last copy of the document the boundary relayed to the
     computer; with none, B
  E  Silo reads first: Silo sends its own read before every such write and compares; if it cannot
     read, the write is held as unknown
  F  relayed if provably current, else Silo reads: D when the write carries the etag of the copy
     relayed (Google), E otherwise; Google writes with no etag get the etag of Silo's read added,
     so the provider refuses them if the policy changed in between; if Silo cannot read, B

Frozen before any run (see README).
"""
import os
import sys

import docs
import grants

HERE = os.path.dirname(os.path.abspath(__file__))
P404 = os.path.normpath(os.path.join(HERE, "..", "..", "2026-11-25-destructive-calls-clouds", "proto"))
OPTIONS = ["A", "B", "C", "D", "E", "F"]


def _today():
    sys.path.insert(0, P404)
    import score  # #404's scorer, which loads #353's, #383's and #404's engines
    return score.load("tuned+fb")


class Judge:
    def __init__(self, aws_key_at_boundary=True):
        self.cache = {}          # key -> (doc, etag) relayed to the computer
        self.today = _today()
        self.aws_key_at_boundary = aws_key_at_boundary

    # -------------------------------------------------------------- what the boundary relays
    def observe(self, req, resp):
        r = docs.read_of(req, resp)
        if r:
            key, doc, etag = r
            self.cache[key] = (doc, etag)
            return
        w = docs.write_of(req)
        if w and resp["status"] < 300 and w.get("readable"):
            # A write that went through: what it set is now the document, as far as the computer knows.
            before = self.cache.get(w["key"], (None, None))[0]
            self.cache[w["key"]] = (docs.after_doc(w, before), None)

    # -------------------------------------------------------------- Silo's own read
    def own(self, w, req, fetch):
        if docs.needs_signing(w) and not self.aws_key_at_boundary:
            return None, 0, "Silo cannot sign a read without the key"
        plan = docs.own_read(w, req)
        sent = 0
        out = None
        for step in plan:
            if "then" in step:  # a managed policy: GetPolicy gave the default version id
                ver = docs.xml_text(last["body"], "DefaultVersionId")
                step = {"method": "POST", "host": "iam.amazonaws.com", "target": "/",
                        "headers": plan[0]["headers"], "body": ("Action=GetPolicyVersion&Version=2010-05-08&PolicyArn=%s&VersionId=%s" % (step["arn"], ver)).encode()}
            last = fetch(step)
            sent += 1
            if last["status"] != 200:
                return None, sent, "Silo's read was refused (%d)" % last["status"]
            out = docs.read_of(step, last)
        if not out:
            return None, sent, "Silo's read gave no document"
        return (out[1], out[2]), sent, None

    # -------------------------------------------------------------- the options
    def decide(self, req, fetch):
        w = docs.write_of(req)
        if not w:
            return None
        res = {"kind": w["kind"], "key": list(w["key"])}
        t = self.today.check(dict(req))
        res["A"] = {"hold": t["decision"] == "hold", "why": t.get("says") or [], "reads": 0}
        if not w.get("readable"):
            for o in "BCDEF":
                res[o] = {"hold": True, "why": ["the body cannot be read"], "reads": 0}
            return res
        kind = w["kind"]
        alone = docs.after_doc(w, None)
        hb, wb = grants.judge_whole(kind, alone) if alone is not None else (True, ["unknown: a JSON Patch needs the document it patches"])
        res["B"] = {"hold": hb, "why": wb, "reads": 0}
        res["C"] = {"hold": True, "why": ["a whole-document access write"], "reads": 0}
        cached = self.cache.get(w["key"])

        def diff(before, label):
            h, why = grants.judge_diff(kind, before, docs.after_doc(w, before))
            return {"hold": h, "why": why, "reads": 0, "before": label}

        if w.get("incremental"):
            for o in "DEF":
                res[o] = diff(None, "nothing: it creates")
            return res
        # D
        res["D"] = diff(cached[0], "relayed") if cached else dict(res["B"], before="none: as B")
        # E, and the read F may use
        got, sent, err = self.own(w, req, fetch)
        res["E"] = dict(diff(got[0], "Silo's read"), reads=sent) if got else {"hold": True, "why": ["unknown: " + err], "reads": sent}
        # F
        if kind == "gcp-iam" and cached and w.get("etag") and grants.etag_bytes(w["etag"]) == grants.etag_bytes(cached[1]):
            res["F"] = diff(cached[0], "relayed, same etag")
        elif got:
            f = dict(diff(got[0], "Silo's read"), reads=sent)
            if kind == "gcp-iam":
                if not w.get("etag"):
                    f["pin"] = got[1]
                elif grants.etag_bytes(w["etag"]) != grants.etag_bytes(got[1]):
                    f = {"hold": False, "why": ["the provider refuses it: its etag is not the policy's"], "reads": sent, "before": "Silo's read"}
            res["F"] = f
        else:
            res["F"] = dict(res["B"], reads=sent, before="none: as B (%s)" % err)
        return res
