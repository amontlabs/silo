"""Runs one scenario of the ask path: the relay, askd and the phone as three processes, a fresh bare
repository behind the boundary, and the commands an agent would run, each at its own time.

    python3 rig.py <scenario.json> <outdir>

A scenario: {"holds": {...}, "policy": {...}, "gather": 2,
             "commands": [{"at": 0, "run": "git push ..."}, ...], "after": 5}
"{B}" in a command is the boundary's address, "{W}" the work clone, "{P}" this folder.
The outdir gets record.jsonl, phone.jsonl, commands.jsonl (each command's output, exit and times)
and timeline.txt (every event, in seconds from the first command).
"""
import json, os, random, shlex, subprocess, sys, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))


# The commands run with a user's plain environment: no proxy, no git settings of this session.
CLEAN = {k: v for k, v in os.environ.items()
         if not k.startswith("GIT_CONFIG") and k.lower() not in ("http_proxy", "https_proxy", "all_proxy")
         and not k.startswith("AWS_")}
CLEAN.update(AWS_ACCESS_KEY_ID="AKIAFAKE", AWS_SECRET_ACCESS_KEY="fake", AWS_DEFAULT_REGION="us-east-1")


def free_port():
    return random.randint(20000, 40000)


def run(scen, out):
    os.makedirs(out, exist_ok=True)
    for f in ("record.jsonl", "phone.jsonl", "commands.jsonl"):
        open(f"{out}/{f}", "w").close()
    subprocess.run([sys.executable, "-c", f"import sys; sys.path.insert(0, {HERE!r}); import link; link.pair({out + '/pairing.json'!r})"], check=True)
    git_root = f"{out}/git"; work = f"{out}/work"
    subprocess.run(f"rm -rf {git_root} {work}; mkdir -p {git_root}; git init -q --bare {git_root}/repo.git; "
                   f"git -C {git_root}/repo.git config http.receivepack true; "
                   f"git init -q -b main {work}; cd {work}; git config user.email a@b; git config user.name agent; "
                   f"echo one > f; git add f; git commit -qm one", shell=True, check=True)
    rp, bp = free_port(), free_port()
    env = dict(os.environ, PYTHONPATH=HERE, RELAY=f"http://127.0.0.1:{rp}", PAIRING=f"{out}/pairing.json")
    procs = [
        subprocess.Popen([sys.executable, f"{HERE}/relay.py"], env=dict(env, PORT=str(rp))),
        subprocess.Popen([sys.executable, f"{HERE}/askd.py"], env=dict(env, PORT=str(bp), RECORD=f"{out}/record.jsonl",
                         GIT_ROOT=git_root, HOLDS=json.dumps(scen.get("holds", {})), GATHER=str(scen.get("gather", 2)),
                         ASK_LIFE=str(scen.get("ask_life", 1800)), KEEPALIVE=str(scen.get("keepalive", 30)))),
        subprocess.Popen([sys.executable, f"{HERE}/phone.py"], env=dict(env, LOG=f"{out}/phone.jsonl",
                         POLICY=json.dumps(scen.get("policy", {"never": True})))),
    ]
    time.sleep(1.0)
    B = f"127.0.0.1:{bp}"
    t0 = time.time()
    lock = threading.Lock(); results = []

    def one(c):
        time.sleep(max(0, t0 + c["at"] - time.time()))
        cmd = c["run"].replace("{B}", B).replace("{W}", work).replace("{P}", HERE)
        s = time.time()
        p = subprocess.run(cmd, shell=True, cwd=work, capture_output=True, text=True,
                           env=dict(CLEAN, **c.get("env", {})), timeout=c.get("timeout", 1800))
        with lock:
            results.append({"at": c["at"], "run": c["run"], "start": round(s - t0, 2), "end": round(time.time() - t0, 2),
                            "exit": p.returncode, "out": (p.stdout + p.stderr)[-4000:]})
    ts = [threading.Thread(target=one, args=(c,)) for c in scen["commands"]]
    for t in ts: t.start()
    for t in ts: t.join()
    time.sleep(scen.get("after", 3))
    for p in procs: p.terminate()
    with open(f"{out}/commands.jsonl", "w") as f:
        for r in sorted(results, key=lambda r: r["start"]): f.write(json.dumps(r) + "\n")
    ev = [dict(json.loads(l), who="mac") for l in open(f"{out}/record.jsonl")] + \
         [dict(json.loads(l), who="phone") for l in open(f"{out}/phone.jsonl")]
    for r in results:
        ev.append({"t": t0 + r["start"], "who": "agent", "kind": "command.start", "run": r["run"]})
        ev.append({"t": t0 + r["end"], "who": "agent", "kind": "command.end", "exit": r["exit"], "run": r["run"]})
    ev.sort(key=lambda e: e["t"])
    with open(f"{out}/timeline.txt", "w") as f:
        for e in ev:
            t = e.pop("t"); who = e.pop("who"); kind = e.pop("kind")
            e.pop("lines", None) if kind == "ask.version" and len(e.get("lines", [])) > 3 else None
            f.write(f"{t - t0:8.2f}  {who:5}  {kind:18} {json.dumps(e)[:220]}\n")
    return out


if __name__ == "__main__":
    run(json.load(open(sys.argv[1])), sys.argv[2])
    print(open(f"{sys.argv[2]}/timeline.txt").read())
