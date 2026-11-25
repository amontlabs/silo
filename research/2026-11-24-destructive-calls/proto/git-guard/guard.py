"""A prototype of Silo's boundary for git push over smart HTTP.

Ends TLS, reads POST .../git-receive-pack, classifies each ref update (create, delete,
fast-forward, force, tag-move), and per --mode:
  pass     forward untouched, read nothing (baseline)
  observe  classify with all three ancestry methods, then forward
  backup   before a delete or force passes, create refs/silo/backup/... at the old commit
  refuse   a delete or force: answer 200 with report-status "ng <ref> <reason>"
  hold     wait --hold seconds (the ask), then back up and forward byte for byte
  split    refuse only the delete/force refs, forward the rest (refuse all if atomic)
Any command on refs/silo/ is refused in every mode except pass: that is where backups live.
"""
import argparse, hashlib, http.client, json, os, ssl, subprocess, time
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import receivepack as rp
from backend import read_body

SENT_HOLD = "Held by Silo: {what}. Ask the user in the Silo app."


class LocalServer:
    """Questions Silo would ask the host. Here: the bare repo on disk.
    For GitHub: GET /repos/{o}/{r}/compare/{old}...{x} and POST /repos/{o}/{r}/git/refs."""

    def __init__(self, path):
        self.path = path
        self.calls = 0

    def git(self, *a):
        self.calls += 1
        return subprocess.run(["git", "-C", self.path, *a], capture_output=True)

    def has(self, sha):
        return self.git("cat-file", "-e", sha + "^{commit}").returncode == 0

    def is_ancestor(self, a, b):
        r = self.git("merge-base", "--is-ancestor", a, b).returncode
        return {0: True, 1: False}.get(r)          # None: error (b unknown)

    def create_ref(self, name, sha):
        r = self.git("update-ref", name, sha, "0" * 40)   # only if it does not exist
        return r.returncode == 0


def ms(t0):
    return round((time.perf_counter() - t0) * 1000, 2)


def classify(push, body, server, methods):
    """Returns {ref: info}. info["class"] is the decision (hybrid method)."""
    out = {}
    commits, stats, pack_ms = {}, None, 0.0
    if push.pack_offset is not None and any(o != rp.ZERO and n != rp.ZERO
                                            for o, n, _ in push.commands):
        t0 = time.perf_counter()
        commits, stats = rp.read_pack(body, push.pack_offset)
        pack_ms = ms(t0)
    for old, new, ref in push.commands:
        info = {"old": old, "new": new}
        if old == rp.ZERO:
            info["class"] = "create"
        elif new == rp.ZERO:
            info["class"] = "delete"
        else:
            # (a) pack only
            t0 = time.perf_counter()
            a, boundary = rp.walk(old, new, commits)
            info["a_pack"] = {"result": a, "boundary": len(boundary),
                              "ms": round(ms(t0) + pack_ms, 2)}
            # (b) server only: works only when new is already on the server
            if "b" in methods:
                t0 = time.perf_counter()
                anc = server.is_ancestor(old, new) if server.has(new) else None
                info["b_server"] = {"result": {True: "ff", False: "force", None: "n/a: new not on server"}[anc],
                                    "ms": ms(t0)}
            # (c) hybrid: pack walk, then ask the server about each boundary commit
            t0 = time.perf_counter()
            res, asks = a, 0
            if a == "unknown":
                res = "force"
                for b in boundary:
                    asks += 1
                    if server.is_ancestor(old, b):
                        res = "ff"
                        break
            info["c_hybrid"] = {"result": res, "server_asks": asks,
                                "ms": round(ms(t0) + info["a_pack"]["ms"], 2)}
            info["class"] = res
            if ref.startswith("refs/tags/"):
                info["class"] = "tag-move"
        out[ref] = info
    return out, stats


def destructive(c):
    return c in ("delete", "force", "tag-move")


class Guard(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def forward(self, body):
        c = http.client.HTTPConnection("127.0.0.1", A.upstream)
        hdrs = {k: v for k, v in self.headers.items()
                if k.lower() not in ("host", "transfer-encoding", "content-length", "connection")}
        hdrs["Content-Length"] = str(len(body))
        c.request(self.command, self.path, body=body if self.command == "POST" else None, headers=hdrs)
        r = c.getresponse()
        return r.status, [(k, v) for k, v in r.getheaders()
                          if k.lower() not in ("transfer-encoding", "content-length", "connection")], r.read()

    def reply(self, status, hdrs, payload):
        self.send_response(status)
        for k, v in hdrs:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self):
        self.reply(*self.forward(b""))

    def do_POST(self):
        t_all = time.perf_counter()
        body = read_body(self)
        rec = {"t": time.time(), "mode": A.mode, "path": self.path, "body_len": len(body),
               "chunked": self.headers.get("Transfer-Encoding", "") == "chunked",
               "received_sha256": hashlib.sha256(body).hexdigest()}
        if A.mode == "pass" or not self.path.endswith("/git-receive-pack"):
            st, h, p = self.forward(body)
            rec.update(forwarded_sha256=hashlib.sha256(body).hexdigest(), total_ms=ms(t_all))
            self.log(rec)
            return self.reply(st, h, p)
        t0 = time.perf_counter()
        push = rp.parse_request(body)
        rec["parse_ms"] = ms(t0)
        if push.probe:
            rec["probe"] = True
            st, h, p = self.forward(body)
            rec.update(forwarded_sha256=hashlib.sha256(body).hexdigest(), total_ms=ms(t_all))
            self.log(rec)
            return self.reply(st, h, p)
        if push.unreadable:
            rec.update(answer="refused: " + push.unreadable, total_ms=ms(t_all))
            self.log(rec)
            return self.reply(200, [("Content-Type", "application/x-git-receive-pack-result")],
                              rp.refusal(push, {}, default=f"Silo cannot read this push ({push.unreadable})"))
        repo = os.path.join(A.root, self.path.split("/git-receive-pack")[0].lstrip("/"))
        server = LocalServer(repo)
        t0 = time.perf_counter()
        refs, stats = classify(push, body, server, methods="b" if A.mode == "observe" else "")
        rec.update(classify_ms=ms(t0), pack=stats, caps=push.caps, signed=push.signed,
                   push_options=push.push_options, refs=refs)
        own = {r for r in refs if r.startswith("refs/silo/")}
        bad = {r for r, i in refs.items() if destructive(i["class"])} | own
        hdr = [("Content-Type", "application/x-git-receive-pack-result"), ("Cache-Control", "no-cache")]
        refuse_all = None
        if own:
            refuse_all = {r: "refs/silo/ is kept by Silo and cannot be changed from the computer" for r in own}
        if A.mode == "refuse" and bad or A.mode == "split" and bad and "atomic" in push.caps:
            refuse_all = {r: self.sentence(r, refs[r]) for r in bad - own} | (refuse_all or {})
        if refuse_all:
            rec["answer"] = "refused"
            why = "atomic push: refused with the held refs" if "atomic" in push.caps else "refused with the held refs of this push"
            payload = rp.refusal(push, refuse_all, default=why)
            rec["total_ms"] = ms(t_all)
            self.log(rec)
            return self.reply(200, hdr, payload)
        if A.mode == "split" and bad and push.signed:
            # rebuilding the commands would break the signature: refuse the whole push
            rec.update(answer="refused", total_ms=ms(t_all))
            self.log(rec)
            return self.reply(200, hdr, rp.refusal(push, {r: self.sentence(r, refs[r]) for r in bad},
                                                   default="signed push: refused with the held refs"))
        if A.mode == "split" and bad:
            keep = [r for r in refs if r not in bad]
            if not keep:
                payload = rp.refusal(push, {r: self.sentence(r, refs[r]) for r in bad})
                rec.update(answer="refused", total_ms=ms(t_all))
                self.log(rec)
                return self.reply(200, hdr, payload)
            fwd = rp.rebuild_request(body, push, set(keep))
            t0 = time.perf_counter()
            st, h, p = self.forward(fwd)
            rec["upstream_ms"] = ms(t0)
            p = rp.splice(p, push, {r: self.sentence(r, refs[r]) for r in bad})
            rec.update(answer="split", passed=keep, refused=sorted(bad),
                       forwarded_sha256=hashlib.sha256(fwd).hexdigest(), total_ms=ms(t_all))
            self.log(rec)
            return self.reply(st, h, p)
        if A.mode == "hold" and bad:
            t0 = time.perf_counter()
            time.sleep(A.hold)          # stands for the ask; the answer here is always yes
            rec["hold_ms"] = ms(t0)
        if A.mode in ("backup", "hold") and bad:
            t0 = time.perf_counter()
            stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%f")
            made = []
            for r in bad:
                name = f"refs/silo/backup/{r[len('refs/'):]}/{stamp}"
                ok = server.create_ref(name, refs[r]["old"])
                made.append({"ref": name, "sha": refs[r]["old"], "ok": ok})
                if not ok:
                    rec.update(answer="refused: backup failed", backups=made, total_ms=ms(t_all))
                    self.log(rec)
                    return self.reply(200, hdr, rp.refusal(push, {x: "Silo could not keep the old commit" for x in bad}))
            rec.update(backups=made, backup_ms=ms(t0))
        t0 = time.perf_counter()
        st, h, p = self.forward(body)
        rec.update(upstream_ms=ms(t0), answer="passed",
                   forwarded_sha256=hashlib.sha256(body).hexdigest(), total_ms=ms(t_all))
        rec["guard_ms"] = round(rec["total_ms"] - rec["upstream_ms"] - rec.get("hold_ms", 0), 2)
        self.log(rec)
        self.reply(st, h, p)

    @staticmethod
    def sentence(ref, info):
        what = {"delete": f"deletes {ref}", "force": f"rewrites {ref} (not a fast-forward)",
                "tag-move": f"moves tag {ref}"}.get(info["class"], f"changes {ref}")
        return SENT_HOLD.format(what=what)

    def log(self, rec):
        with open(A.log, "a") as f:
            f.write(json.dumps(rec) + "\n")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--upstream", type=int, default=9000)
    ap.add_argument("--root", required=True, help="where the bare repos are (local server)")
    ap.add_argument("--mode", required=True,
                    choices=["pass", "observe", "backup", "refuse", "hold", "split"])
    ap.add_argument("--hold", type=float, default=2.0)
    ap.add_argument("--cert", required=True)
    ap.add_argument("--key", required=True)
    ap.add_argument("--log", required=True)
    A = ap.parse_args()
    srv = ThreadingHTTPServer(("127.0.0.1", A.port), Guard)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(A.cert, A.key)
    ctx.set_alpn_protocols(["http/1.1"])
    srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
    srv.serve_forever()
