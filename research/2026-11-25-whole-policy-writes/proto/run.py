#!/usr/bin/env python3
"""Runs the scenarios through a boundary that judges every whole-document access write six ways.

The boundary listens on 127.0.0.30:443 (rig.sh points the CLIs' hosts there), ends TLS with the
test CA, gives each write to options.Judge, and passes every request to mock.handle, the provider.
By default it holds nothing (each option's decision is only written down), so every scenario runs
to its end. With --enforce F it applies option F: it holds what F holds, and adds the etag of
Silo's read to a Google write that has none.

Usage: run.py [--enforce F] [--out results/run.json] [scenario ...]
"""
import argparse
import json
import os
import shlex
import socket
import socketserver
import ssl
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import docs  # noqa: E402
import mock  # noqa: E402
import options  # noqa: E402
import scenarios  # noqa: E402

W = "/opt/wpw"
ENV = {
    "gcloud": ["CLOUDSDK_AUTH_ACCESS_TOKEN_FILE=%s/gc-token" % W, "CLOUDSDK_CORE_PROJECT=proj1",
               "CLOUDSDK_CORE_CUSTOM_CA_CERTS_FILE=%s/pki/bundle.crt" % W, "CLOUDSDK_CORE_DISABLE_PROMPTS=1",
               "CLOUDSDK_COMPONENT_MANAGER_DISABLE_UPDATE_CHECK=1", "CLOUDSDK_CORE_DISABLE_USAGE_REPORTING=1",
               "CLOUDSDK_CONFIG=%s/home/gcloud/gcloud" % W],
    "aws": ["AWS_ACCESS_KEY_ID=TESTKEY", "AWS_SECRET_ACCESS_KEY=testsecret", "AWS_DEFAULT_REGION=us-east-1",
            "AWS_EC2_METADATA_DISABLED=true", "AWS_PAGER=", "AWS_MAX_ATTEMPTS=1"],
    "az": ["IDENTITY_ENDPOINT=https://identity.capture.test/token", "IDENTITY_HEADER=made-up", "AZURE_CORE_COLLECT_TELEMETRY=0",
           "AZURE_CORE_ONLY_SHOW_ERRORS=1", "AZURE_CORE_NO_COLOR=1", "AZURE_EXTENSION_USE_DYNAMIC_INSTALL=no",
           "AZURE_CONFIG_DIR=%s/home/az/.azure" % W],
    "kubectl": ["KUBECONFIG=%s/kubeconfig" % W],
    "curl": [],
}


# ---------------------------------------------------------------- the boundary
class Boundary:
    def __init__(self):
        self.enforce = None
        self.start()

    def start(self, scenario=None):
        self.judges = {"key": options.Judge(aws_key_at_boundary=True), "nokey": options.Judge(aws_key_at_boundary=False)}
        self.log = []
        self.writes = []

    def fetch(self, req):
        resp = mock.handle(req)
        self.log.append({"by": "silo", "method": req["method"], "host": req["host"], "target": req["target"], "status": resp["status"]})
        return resp

    def serve(self, req):
        decisions = {}
        for name, jdg in self.judges.items():
            d = jdg.decide(req, self.fetch)
            if d:
                decisions[name] = d
        entry = {"by": "computer", "method": req["method"], "host": req["host"], "target": req["target"][:200]}
        held = False
        if decisions:
            d = decisions["key"]
            if self.enforce:
                f = d[self.enforce]
                if f["hold"]:
                    held = True
                elif f.get("pin"):
                    b = json.loads(req["body"])
                    b["policy"]["etag"] = f["pin"]
                    req = dict(req, body=json.dumps(b).encode())
                    entry["pinned"] = f["pin"]
        if held:
            resp = {"status": 403, "headers": [["Content-Type", "application/json"]],
                    "body": json.dumps({"error": {"code": 403, "status": "PERMISSION_DENIED", "message": "Silo held this request."}}).encode()}
        else:
            resp = mock.handle(req)
        for jdg in self.judges.values():
            jdg.observe(req, resp)
        entry["status"] = resp["status"]
        self.log.append(entry)
        if decisions:
            self.writes.append({"request": entry, "body": req["body"].decode("utf-8", "replace")[:4000], "decisions": decisions, "held": held})
        return resp


B = None


def read_request(f, sock):
    line = f.readline(65536)
    if not line or not line.strip():
        line = f.readline(65536) if line else line
        if not line:
            return None
    method, target, _ = line.decode("latin-1").rstrip("\r\n").split(" ", 2)
    headers = []
    while True:
        h = f.readline(65536).decode("latin-1")
        if h in ("\r\n", "\n", ""):
            break
        k, _, v = h.partition(":")
        headers.append([k.strip(), v.strip()])
    if (docs.hget(headers, "expect") or "").lower() == "100-continue":
        sock.sendall(b"HTTP/1.1 100 Continue\r\n\r\n")
    body = b""
    if "chunked" in (docs.hget(headers, "transfer-encoding") or "").lower():
        while True:
            size = int(f.readline().strip().split(b";")[0], 16)
            if size == 0:
                while f.readline() not in (b"\r\n", b"\n", b""):
                    pass
                break
            body += f.read(size)
            f.readline()
    else:
        n = int(docs.hget(headers, "content-length") or 0)
        if n:
            body = f.read(n)
    host = (docs.hget(headers, "host") or "").split(":")[0]
    return {"method": method, "host": host, "target": target, "headers": headers, "body": body}


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        ctx = self.server.ctx
        try:
            s = ctx.wrap_socket(self.request, server_side=True)
        except (ssl.SSLError, OSError):
            return
        f = s.makefile("rb")
        try:
            while True:
                req = read_request(f, s)
                if req is None:
                    break
                resp = B.serve(req)
                reason = {200: "OK", 201: "Created", 204: "No Content", 403: "Forbidden", 404: "Not Found", 409: "Conflict"}.get(resp["status"], "X")
                hdr = [(k, v) for k, v in resp["headers"] if k.lower() not in ("content-length", "transfer-encoding", "connection")]
                out = "HTTP/1.1 %d %s\r\n" % (resp["status"], reason)
                out += "".join("%s: %s\r\n" % kv for kv in hdr)
                out += "Content-Length: %d\r\nConnection: keep-alive\r\n\r\n" % len(resp["body"])
                s.sendall(out.encode("latin-1") + (resp["body"] if req["method"] != "HEAD" else b""))
        except (ssl.SSLError, OSError, ValueError):
            pass
        finally:
            try:
                s.close()
            except OSError:
                pass


class TLSServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def start_boundary():
    global B
    B = Boundary()
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(W + "/pki/chain.crt", W + "/pki/leaf.key")
    ctx.set_alpn_protocols(["http/1.1"])
    srv = TLSServer(("127.0.0.30", 443), Handler)
    srv.ctx = ctx
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv


# ---------------------------------------------------------------- the steps
def cli_of(cmd):
    return cmd.split()[0]


def run_cli(cmd, cli=None):
    cli = cli or cli_of(cmd)
    noproxy = open(W + "/noproxy").read().strip(",")
    bundle = W + "/pki/bundle.crt"
    env = ["HOME=%s/home/%s" % (W, cli), "TERM=dumb", "LANG=C.UTF-8",
           "PATH=/opt/awsvenv/bin:/opt/azvenv/bin:/usr/local/bin:/usr/bin:/bin",
           "SSL_CERT_FILE=" + bundle, "REQUESTS_CA_BUNDLE=" + bundle, "CURL_CA_BUNDLE=" + bundle, "AWS_CA_BUNDLE=" + bundle,
           "HTTPS_PROXY=http://127.0.0.1:9", "https_proxy=http://127.0.0.1:9", "HTTP_PROXY=http://127.0.0.1:9", "http_proxy=http://127.0.0.1:9",
           "NO_PROXY=" + noproxy, "no_proxy=" + noproxy, "CI=1", "NO_COLOR=1"] + ENV[cli]
    os.makedirs("%s/home/%s" % (W, cli), exist_ok=True)
    t = time.time()
    p = subprocess.run(["env", "-i"] + env + ["bash", "-c", cmd], capture_output=True, text=True, timeout=120, cwd=W + "/work")
    return {"step": cmd[:300], "exit": p.returncode, "seconds": round(time.time() - t, 2), "out": (p.stdout + p.stderr)[-1500:]}


def run_step(step):
    kind = step[0]
    if kind == "cli":
        if step[1].startswith("az ") and not os.path.exists(W + "/home/az/.azure/azureProfile.json"):
            run_cli("az login --identity --allow-no-subscriptions")  # once: the made-up managed identity
        return run_cli(step[1])
    if kind == "curl":
        _, method, url, body = step
        cmd = "curl -sS -X %s -H 'Authorization: Bearer made-up' -H 'Content-Type: application/json' --data %s %s" % (
            method, shlex.quote(json.dumps(body)), shlex.quote(url))
        return run_cli(cmd, "curl")
    if kind == "human":
        _, key, fn = step
        with mock.LOCK:
            fn(mock.STATE[key])
        return {"step": "someone outside the computer changes %s" % key}
    if kind == "deny-read":
        mock.DENY_READ.add(step[1])
        return {"step": "the credential cannot read %s" % step[1]}
    if kind == "file":
        open(step[1], "w").write(step[2])
        return {"step": "file %s" % step[1]}
    if kind == "edit":
        open(step[1], "w").write(step[2](open(step[1]).read()))
        return {"step": "the agent edits %s" % step[1]}
    raise ValueError(kind)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--enforce")
    ap.add_argument("--out", default=os.path.join(HERE, "results", "run.json"))
    ap.add_argument("only", nargs="*")
    a = ap.parse_args()
    start_boundary()
    B.enforce = a.enforce
    results = []
    for sid, label, steps in scenarios.S:
        if a.only and sid not in a.only:
            continue
        mock.reset(scenarios.WORLD)
        B.start()
        out = []
        for s in steps:
            try:
                out.append(run_step(s))
            except Exception as e:  # a step that fails ends the scenario; the result says so
                out.append({"step": str(s[:2]), "error": repr(e)})
                break
        last = B.writes[-1] if B.writes else None
        results.append({"id": sid, "label": label, "steps": out, "writes": B.writes, "log": B.log, "last": last,
                        "state_after": {k: mock.STATE.get(k) for k in mock.STATE if last and k.split("|", 1)[0] == last["decisions"]["key"]["key"][0]}})
        d = last["decisions"]["key"] if last else {}
        print("%-28s %-13s %s" % (sid, label, " ".join("%s:%s" % (o, "H" if d.get(o, {}).get("hold") else "-") for o in options.OPTIONS) if d else "NO WRITE " + json.dumps(out[-1])[:300]), flush=True)
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    json.dump(results, open(a.out, "w"), indent=1, default=str)


if __name__ == "__main__":
    main()
