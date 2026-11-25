"""One real run of a team: #385's stand-in provider, the ledger, a relay per Mac, and one process per
Mac, all on this machine. Faults are done to the real processes: the ledger killed with SIGKILL
and started again on its database, a Mac cut off from the ledger, a Mac stopped with SIGSTOP as a
laptop that sleeps.

python3 run.py '{"way": "lease", "seed": 1}'     one run, prints its result as JSON

A run's settings, with their defaults, are DEFAULTS below. `macs` lists each Mac's members; an
idle member has no Mac process, since it never calls. The provider bills every call to its own
ledger.jsonl, a cut one for what was generated: that bill is what the team spent.
"""
import json, os, signal, socket, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
LEDGER = os.path.join(HERE, "..", "2026-11-25-team-ledger", "ledger", "server.py")
# #385's stand-in provider, at the commit of amontlabs/silo#385 it was measured with.
PROVIDER_REV = "f1f3a320352cf0c37df9ca139de385a815af727c"
PROVIDER_PATH = "research/2026-11-24-spend-budgets/fake/provider.py"
OUT = os.environ.get("LIVE_OUT", os.path.join(HERE, "out"))

DEFAULTS = {
    "way": "lease",              # local (one boundary for all), central, lease
    "macs": [["heavy"], ["heavy"], ["medium"], ["light"]],
    "budget": 15.0,
    "policy": "reserve_typical",  # with cut: typical-cut; finish_tps with cut: cut
    "cut": True,
    "tps": 500.0,
    "rtt_ms": 30.0,
    "chunk": 2.0,
    "recall": True,
    "ttl": 0.0,
    "margin": 5.0,
    "on_down": "hold",
    "keep": 1.0,                 # leases: room a Mac keeps on a recall while its calls run (0: #388's rule)
    "rearm": True,               # leases: an answer that left the Mac over its slice is not "no answer"
    "seed": 1,
    "kill": None,                # [from, to]: the ledger is dead from `from` to `to`, seconds after the start
    "partition": None,           # [from, to, [mac, ...]]: those Macs cannot reach the ledger
    "sleep": None,               # [from, to, mac]: that Mac's process is stopped
    "batch": None,               # [mac, at, requests, max_tokens, result_seconds]
    "until": 900.0,
}


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def provider_file():
    dst = os.path.join(OUT, "provider-385.py")
    if not os.path.exists(dst):
        os.makedirs(OUT, exist_ok=True)
        src = subprocess.run(["git", "show", f"{PROVIDER_REV}:{PROVIDER_PATH}"], cwd=HERE, capture_output=True, text=True, check=True).stdout
        with open(dst, "w") as f:
            f.write(src)
    return dst


def cert():
    c, k = os.path.join(OUT, "cert.pem"), os.path.join(OUT, "key.pem")
    if not os.path.exists(c):
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "30", "-subj", "/CN=api.anthropic.com",
                        "-keyout", k, "-out", c], check=True, capture_output=True)
    return c, k


def price(rec):
    batch = rec.get("tier") == "batch"
    p_in, p_out, p_cr, p_cw, p_cl = (2e-6, 10e-6, 0.1e-6, 2.5e-6, 4e-6) if batch else (4e-6, 20e-6, 0.2e-6, 5e-6, 8e-6)
    out = rec["output"] if rec.get("completed", True) else rec.get("billed_output", 0)
    return (rec.get("input", 0) * p_in + rec.get("cache_read", 0) * p_cr + rec.get("cache_write_short", 0) * p_cw
            + rec.get("cache_write_long", 0) * p_cl + out * p_out), out * p_out


def run(cfg):
    c = dict(DEFAULTS, **cfg)
    name = c.get("name") or f"{c['way']}-{c['seed']}"
    d = tempfile.mkdtemp(prefix=name.replace("/", " per ") + "-", dir=os.path.join(OUT, "runs") if os.path.isdir(os.path.join(OUT, "runs")) else None)
    os.makedirs(d, exist_ok=True)
    crt, key = cert()
    procs = []
    sc = {"tps": c["tps"], "main_steps": 100000, "sub_steps": 100000, "seed": c["seed"],
          "batch_seconds": (c["batch"][4] if c["batch"] else 2)}
    with open(os.path.join(d, "scenario.json"), "w") as f:
        json.dump(sc, f)
    pport = free_port()
    prov = subprocess.Popen([sys.executable, provider_file()], env=dict(os.environ, PORT=str(pport), TLS_CERT=crt, TLS_KEY=key, OUT=d,
                                                                       SCENARIO=os.path.join(d, "scenario.json")),
                            stdout=subprocess.DEVNULL, stderr=open(os.path.join(d, "provider.err"), "w"))
    procs.append(prov)
    lport = free_port()
    ledger_args = [sys.executable, LEDGER, "--db", os.path.join(d, "ledger.db"), "--port", str(lport), "--budget", f"team={c['budget']}",
                   "--policy", c["policy"], "--chunk", str(c["chunk"]), "--tps", str(c["tps"]), "--ttl", str(c["ttl"])]
    if c["cut"]:
        ledger_args.append("--cut")
    if not c["recall"]:
        ledger_args.append("--no-recall")
    ledger_args += ["--recall-wait", str(max(0.25, 2 * c["rtt_ms"] / 1000 + 0.1))]
    ledger = None

    def start_ledger():
        p = subprocess.Popen(ledger_args, stdout=subprocess.PIPE, stderr=open(os.path.join(d, "ledger.err"), "a"))
        p.stdout.readline()
        return p
    ctl = os.path.join(d, "ctl.json")
    with open(ctl, "w") as f:
        json.dump({"down": []}, f)
    macs = c["macs"] if c["way"] != "local" else [[m for ms in c["macs"] for m in ms]]
    mports = []
    if c["way"] != "local":
        ledger = start_ledger()
        mports = [free_port() for _ in macs]
        net = subprocess.Popen([sys.executable, os.path.join(HERE, "netem.py"), "--ctl", ctl]
                               + sum((["--map", f"{p}:{lport}:{c['rtt_ms']}"] for p in mports), []), stdout=subprocess.PIPE)
        net.stdout.readline()
        procs.append(net)
    # wait for the provider
    for _ in range(100):
        try:
            socket.create_connection(("127.0.0.1", pport), timeout=0.2).close()
            break
        except OSError:
            time.sleep(0.05)
    t0 = time.time() + 1.0
    outs, mprocs = [], []
    for i, ms in enumerate(macs):
        o = os.path.join(d, f"mac{i}.jsonl")
        outs.append(o)
        args = [sys.executable, os.path.join(HERE, "mac.py"), "--mac", str(i), "--members", ",".join(ms), "--way", c["way"],
                "--policy", c["policy"], "--budget", str(c["budget"]), "--tps", str(c["tps"]), "--rtt", str(c["rtt_ms"] / 1000),
                "--margin", str(c["margin"]), "--on-down", c["on_down"], "--keep", str(c["keep"]), "--provider-port", str(pport), "--cafile", crt,
                "--seed", str(c["seed"]), "--t0", str(t0), "--out", o]
        if mports:
            args += ["--ledger-port", str(mports[i])]
        if c["cut"]:
            args.append("--cut")
        if not c["rearm"]:
            args.append("--no-rearm")
        if c["batch"] and c["batch"][0] == i:
            args += ["--batch", json.dumps(c["batch"][1:])]
        mprocs.append(subprocess.Popen(args, stderr=open(os.path.join(d, f"mac{i}.err"), "w")))
    events = []
    killed = stopped = False
    parted = False
    while any(p.poll() is None for p in mprocs) and time.time() - t0 < c["until"]:
        now = time.time() - t0
        if c["kill"] and ledger is not None:
            a, b = c["kill"]
            if a <= now < b and not killed:
                ledger.send_signal(signal.SIGKILL)
                ledger.wait()
                killed = True
                events.append(["ledger killed", round(now, 1)])
            elif now >= b and killed and ledger.poll() is not None:
                ledger = start_ledger()
                events.append(["ledger started again", round(now, 1)])
        if c["partition"]:
            a, b, who = c["partition"]
            want = a <= now < b
            if want != parted:
                with open(ctl, "w") as f:
                    json.dump({"down": [mports[i] for i in who] if want else []}, f)
                parted = want
                events.append(["cut off" if want else "back", round(now, 1)])
        if c["sleep"]:
            a, b, who = c["sleep"]
            p = mprocs[who]
            if a <= now < b and not stopped and p.poll() is None:
                p.send_signal(signal.SIGSTOP)
                stopped = True
                events.append([f"mac{who} asleep", round(now, 1)])
            elif now >= b and stopped:
                p.send_signal(signal.SIGCONT)
                stopped = False
                events.append([f"mac{who} awake", round(now, 1)])
        time.sleep(0.05)
    timed_out = any(p.poll() is None for p in mprocs)
    for p in mprocs:
        if p.poll() is None:
            p.send_signal(signal.SIGCONT)
            p.kill()
    state = None
    if ledger is not None and ledger.poll() is None:
        try:
            import http.client
            h = http.client.HTTPConnection("127.0.0.1", lport, timeout=3)
            h.request("GET", "/state?budget=team")
            state = json.loads(h.getresponse().read())
        except Exception:
            pass
        ledger.kill()
    time.sleep(0.5)
    for p in procs:
        p.kill()
    return summarize(c, d, outs, events, state, timed_out)


def summarize(c, d, outs, events, state, timed_out):
    billed = cut_paid = 0.0
    n_calls = n_cut = 0
    with open(os.path.join(d, "ledger.jsonl")) as f:
        for line in f:
            rec = json.loads(line)
            if rec.get("provider") != "anthropic":
                continue
            cost, out = price(rec)
            billed += cost
            if not rec.get("completed", True):
                cut_paid += out
                n_cut += 1
            elif rec.get("kind") != "batch":
                n_calls += 1
    rows, sums = [], []
    for o in outs:
        with open(o) as f:
            for line in f:
                r = json.loads(line)
                (sums if r.get("summary") else rows).append(r)
    calls = [r for r in rows if "call" in r and not r.get("refused")]
    counted = sum(s["counted"] for s in sums)
    msgs = sum(s.get("messages", 0) for s in sums)
    waits = sorted(r["wait"] for r in calls) or [0.0]
    batch = [r for r in rows if "batch" in r and "admitted" in r]
    budget = c["budget"]
    return {
        "name": c.get("name"), "way": c["way"], "seed": c["seed"], "policy": c["policy"], "cut": c["cut"], "budget": budget,
        "billed": round(billed, 4), "over_pct": round(100 * (billed - budget) / budget, 2),
        "counted": round(counted, 4), "count_err_pct": round(100 * (counted - billed) / billed, 2) if billed else None,
        "cut_paid": round(cut_paid, 4), "calls_done": n_calls, "calls_cut": n_cut,
        "refused": sum(1 for r in rows if r.get("refused")),
        "wait_p50_ms": round(1000 * waits[len(waits) // 2], 1), "wait_p99_ms": round(1000 * waits[min(len(waits) - 1, int(len(waits) * 0.99))], 1),
        "wait_max_s": round(waits[-1], 2), "waited_10s": sum(1 for x in waits if x > 10),
        "messages_per_call": round(msgs / max(1, n_calls), 2),
        "failures": sum(s.get("failures", 0) for s in sums),
        "asks": sum(s.get("asks", 0) for s in sums), "gives": sum(s.get("gives", 0) for s in sums), "renews": sum(s.get("renews", 0) for s in sums),
        "ended_s": round(max((s["ended"] for s in sums), default=0), 1),
        "batch_admitted": batch[0]["admitted"] if batch else None,
        "ledger": None if not state else {"settled": state["settled"], "free": state["free"],
                                          "leases": sum(l["granted"] - l["given"] for l in state["leases"])},
        "events": events, "timed_out": timed_out, "dir": d,
    }


if __name__ == "__main__":
    print(json.dumps(run(json.loads(sys.argv[1] if len(sys.argv) > 1 else "{}"))))
