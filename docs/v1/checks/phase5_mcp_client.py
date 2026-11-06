#!/usr/bin/env python3
"""Minimal MCP stdio client for the phase 5 computer-use check.

usage: phase5_mcp_client.py SILO_BINARY CWD STEPS.json

STEPS.json is a list of steps run in ONE `silo mcp` session (so the per-fork driver session persists):
  {"call": "tool_name", "args": {...}, "save": "path-prefix"}   call a tool; images are saved as <prefix>-<n>.png
  {"sh": "shell command"}                                       run a host command between calls
  {"sleep": SECONDS}
  {"list": true}                                                tools/list, prints the names
Each step prints one JSON line: {"step": i, ...}. Exit code 0 when the client ran (tool errors are reported,
not fatal).
"""
import base64, json, struct, subprocess, sys, time, os

silo, cwd, steps_file = sys.argv[1:4]
steps = json.load(open(steps_file))
p = subprocess.Popen([silo, "mcp"], cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=subprocess.DEVNULL, text=True, bufsize=1)
nid = 0

def rpc(method, params=None, notify=False):
    global nid
    msg = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        msg["params"] = params
    if not notify:
        nid += 1
        msg["id"] = nid
    p.stdin.write(json.dumps(msg) + "\n")
    p.stdin.flush()
    if notify:
        return None
    while True:
        line = p.stdout.readline()
        if not line:
            return {"error": "server closed"}
        r = json.loads(line)
        if r.get("id") == msg["id"]:
            return r

def png_size(b):
    if b[:8] == b"\x89PNG\r\n\x1a\n":
        return struct.unpack(">II", b[16:24])
    return None

rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "phase5-check", "version": "1"}})
rpc("notifications/initialized", notify=True)
for i, s in enumerate(steps):
    t0 = time.time()
    if "sh" in s:
        r = subprocess.run(s["sh"], shell=True, capture_output=True, text=True)
        print(json.dumps({"step": i, "sh": s["sh"], "rc": r.returncode, "out": (r.stdout + r.stderr).strip()[:500]}), flush=True)
    elif "sleep" in s:
        time.sleep(s["sleep"]); print(json.dumps({"step": i, "slept": s["sleep"]}), flush=True)
    elif "list" in s:
        r = rpc("tools/list")
        names = [t["name"] for t in r.get("result", {}).get("tools", [])]
        print(json.dumps({"step": i, "tools": names}), flush=True)
    else:
        r = rpc("tools/call", {"name": s["call"], "arguments": s.get("args", {})})
        res = r.get("result")
        out = {"step": i, "call": s["call"], "secs": round(time.time() - t0, 1)}
        if res is None:
            out["error"] = r.get("error")
        else:
            out["isError"] = bool(res.get("isError"))
            texts, imgs = [], []
            for c in res.get("content", []):
                if c.get("type") == "text":
                    texts.append(c["text"])
                elif c.get("type") == "image":
                    raw = base64.b64decode(c["data"])
                    n = len(imgs)
                    if s.get("save"):
                        os.makedirs(os.path.dirname(s["save"]) or ".", exist_ok=True)
                        path = f'{s["save"]}-{n}.png'
                        open(path, "wb").write(raw)
                    else:
                        path = None
                    imgs.append({"mime": c.get("mimeType"), "bytes": len(raw), "size": png_size(raw), "path": path})
            out["text"] = "\n".join(texts)[:400000]
            out["images"] = imgs
        print(json.dumps(out), flush=True)
p.stdin.close()
try:
    p.wait(timeout=15)
except subprocess.TimeoutExpired:
    p.terminate()
