"""What each part going wrong does to a program in a computer, and to the record.

usage: failures.py <out>   (a run started by up.sh with "finish" "0.002" and
       '{"decision_room":40,"observation_room":20,"decision_wait_ms":3000}', so that c1 has a
       budget of $0.002 and the record's room is small)
"""
import json, os, signal, subprocess, sys, threading, time

R = os.path.dirname(os.path.abspath(__file__)); E = os.path.dirname(R); OUT = os.path.abspath(sys.argv[1])
ENV = dict(l.strip().split("=", 1) for l in open(f"{OUT}/env") if "=" in l)
SILOREC = os.path.join(os.path.dirname(E), "2026-11-25-record-format", "proto", "zig-out", "bin", "silorec")

def run(c, *cmd, timeout=60):
    t = time.time()
    p = subprocess.run([f"{R}/in.sh", OUT, c, *cmd], capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout.strip(), p.stderr.strip(), time.time() - t

def req(c, timeout=20):
    """One request with a placeholder swapped: a decision and a change to traffic."""
    rc, out, err, dt = run(c, "curl", "-sS", "-m", str(timeout), "-o", "/dev/null", "-w", "%{http_code}", "https://storage.googleapis.com/storage/v1/b", "-H", f"authorization: Bearer ya29.silo-placeholder-{c}", timeout=timeout + 5)
    return (out or "000"), err.splitlines()[-1][:90] if err else "", dt

def say(name, what):
    print(f"{name}: {what}", flush=True)

def pid(name): return int(open(f"{OUT}/{name}.pid").read())

def verify(c):
    p = subprocess.run([SILOREC, "verify", f"{OUT}/record/{c}", c], capture_output=True, text=True)
    lines = subprocess.run([SILOREC, "print", f"{OUT}/record/{c}"], capture_output=True, text=True).stdout.splitlines()
    kinds = {}
    for l in lines:
        o = json.loads(l); k = o["kind"] + ("/" + o["what"] if "what" in o else "")
        kinds[k] = kinds.get(k, 0) + 1
    gaps = [json.loads(l)["missing"] for l in lines if '"kind":"gap"' in l]
    return p.stdout.strip() or p.stderr.strip(), kinds, gaps

def source_status(name):
    last = None
    for l in open(f"{OUT}/trace.jsonl"):
        if f'"source":"boundary:{name}"' in l: last = json.loads(l)
    return {k: last[k] for k in ("connected", "acked", "last", "decisions_waiting", "observations_waiting", "dropped", "decisions_waited", "decisions_refused")} if last else None

# 0. Everything up.
for c in ("c1", "c2"): say(f"{c} baseline", req(c))

# 1. A budget reached mid-run (#374's, composed): c1 has $0.002.
MSG = json.dumps({"model": "claude-opus-5-5", "max_tokens": 4000, "messages": [{"role": "user", "content": "work " * 400}]})
codes = []
for i in range(6):
    rc, out, err, dt = run("c1", "curl", "-sS", "-m", "30", "https://api.anthropic.com/v1/messages", "-H", "x-api-key: sk-ant-api03-silo-placeholder-c1-0000", "-H", "content-type: application/json", "-d", MSG)
    codes.append(out[:150] if '"error"' in out else "200")
say("c1 budget of $0.002, six calls", codes)

# 2. The recorder stalls (stopped, its socket still open): decisions fill their room, then wait.
dl = {}
def download():
    rc, out, err, dt = run("c1", "curl", "-sS", "-m", "60", "--cacert", f"{ENV['TLS']}/up-ca.crt", "-o", "/dev/null", "-w", "%{http_code} %{size_download}", "--limit-rate", "20M", "https://github.com/big?bytes=200000000", timeout=70)
    dl.update(out=out, err=err, dt=dt)
t = threading.Thread(target=download); t.start(); time.sleep(0.5)
os.kill(pid("recorder"), signal.SIGSTOP); t0 = time.time()
results = [req("c1", timeout=10) for _ in range(30)]
ok = sum(1 for r in results if r[0] == "200"); refused = [r for r in results if r[0] != "200"]
say("recorder stopped: 30 requests from c1", f"{ok} passed, {len(refused)} did not; first that did not: {refused[0] if refused else None}; slowest {max(r[2] for r in results):.1f}s")
say("recorder stopped: c1's source", source_status("c1"))
os.kill(pid("recorder"), signal.SIGCONT); time.sleep(1.5)
t.join()
say("a download allowed before the stall, during it", dl)
say("recorder back: c1 request", req("c1"))
say("recorder back: c1's source", source_status("c1"))

# 3. The recorder killed and started again on the same record.
os.kill(pid("recorder"), signal.SIGKILL); time.sleep(0.2)
during = [req("c2", timeout=10) for _ in range(5)]
say("recorder killed: 5 requests from c2", [d[0] for d in during])
p = subprocess.Popen([f"{E}/recorder/zig-out/bin/recorder", f"{OUT}/config.json", f"{OUT}/record", f"{OUT}/recorder.sock"], stderr=open(f"{OUT}/recorder2.err", "w"))
open(f"{OUT}/recorder.pid", "w").write(str(p.pid)); time.sleep(1.5)
say("recorder started again: c2 request", req("c2"))
say("recorder started again: c2's source", source_status("c2"))

# 3b. The recorder away while only observations come (c2's lookups): the buffer of 20 overflows,
# the oldest are dropped, and the recorder writes a gap when it is back.
os.kill(pid("recorder"), signal.SIGKILL); time.sleep(0.2)
for i in range(30): run("c2", "getent", "hosts", f"n{i}.example", timeout=10)
say("recorder killed: 30 lookups from c2", source_status("c2"))
p = subprocess.Popen([f"{E}/recorder/zig-out/bin/recorder", f"{OUT}/config.json", f"{OUT}/record", f"{OUT}/recorder.sock"], stderr=open(f"{OUT}/recorder3.err", "w"))
open(f"{OUT}/recorder.pid", "w").write(str(p.pid)); time.sleep(1.5)
say("recorder started again: c2's source", source_status("c2"))

# 4. A host down.
hp = [int(x) for x in open(f"{OUT}/pids").read().split()][1]
os.kill(hp, signal.SIGSTOP)
for c in ("c1", "c2"): say(f"{c} host not answering (stopped)", req(c, timeout=8))
os.kill(hp, signal.SIGKILL); time.sleep(0.3)
for c in ("c1", "c2"): say(f"{c} host gone (connection refused)", req(c, timeout=8))

# 5. The boundary down, then back.
bp = pid("boundary"); os.kill(bp, signal.SIGKILL); time.sleep(0.3)
for c in ("c1", "c2"): say(f"{c} boundary down", req(c, timeout=8))
rc, out, err, dt = run("c2", "getent", "hosts", "api.anthropic.com", timeout=30)
say("c2 boundary down: a name lookup", f"rc={rc} {out!r} in {dt:.1f}s")
env = dict(os.environ, SILO_CONFIG=f"{OUT}/config.json", RECORD=f"{OUT}/trace.jsonl")
b = subprocess.Popen([f"{E}/boundary/zig-out/bin/boundary"], env=env, stderr=open(f"{OUT}/boundary2.err", "w"))
open(f"{OUT}/boundary.pid", "w").write(str(b.pid)); time.sleep(1)
for c in ("c1", "c2"): say(f"{c} boundary back (host still gone)", req(c, timeout=8))

# 6. The relay (c1) and gvproxy (c2) down.
subprocess.run(["pkill", "-f", f"SOCK={OUT}/c1.connect.sock"], capture_output=True)
subprocess.run(["pkill", "-f", "run/relay.py"], capture_output=True)
say("c1 relay down", req("c1", timeout=8))
subprocess.run(["pkill", "-f", f"unix://{OUT}/net.sock"], capture_output=True); time.sleep(0.3)
say("c2 gvproxy down", req("c2", timeout=8))

# The record after all this.
for c in ("c1", "c2"):
    v, kinds, gaps = verify(c)
    say(f"record {c}", f"{v}; gaps {gaps}; {kinds}")
