"""#353's detector with two additions to body conditions, nothing else:

- "in": [null] matches a field sent as JSON null (Fly deletes a secret with {"values": {"K": null}},
  Vercel removes deployment protection with {"ssoProtection": null}). #353 skipped null fields.
- "present": true matches a field (globs allowed) sent with a non-empty value (Cloudflare's DNS
  batch deletes whatever is listed under "deletes").
"""
import fnmatch
import os
import sys

OLD = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..",
                                    "2026-11-24-destructive-calls", "proto", "detector"))
if OLD not in sys.path:
    sys.path.insert(0, OLD)
import detector as base  # noqa: E402


class Detector(base.Detector):
    def _body_cond(self, cond, ctx, vars_):
        if "present" not in cond and None not in cond.get("in", ()):
            return super()._body_cond(cond, ctx, vars_)
        j = ctx.json()
        fields = base.flatten_args(j) if isinstance(j, (dict, list)) else dict(ctx.form())
        pat = cond["field"]
        keys = [k for k in fields if fnmatch.fnmatchcase(k, pat)] if "*" in pat else [k for k in [pat] if k in fields]
        hit = False
        for k in keys:
            v = fields[k]
            if "present" in cond:
                ok = v not in (None, "", [], {})
            else:
                ok = v is None or str(v).lower() in [str(x).lower() for x in cond["in"] if x is not None]
            if ok:
                vars_.setdefault("matched", k)
                hit = True
        return hit


def load(path):
    return Detector.load(path)
