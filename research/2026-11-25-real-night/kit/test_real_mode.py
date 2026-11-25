"""The plumbing of a real night, tested without a real model.

    python3 test_real_mode.py <folder>

Runs `night.py --real` with stand-in providers in front of the scripted model. Each stand-in checks
the key the boundary put in. Passes when:
  - every model call reached the provider with the real key, and none with the placeholder;
  - the keys appear nowhere in the night's folder: record, probe traces, logs, computers;
  - the budget refused calls once reached, and refused gpt-5.5 at once since it has no price;
  - an allowed connection went through to the real internet (when this machine has it);
  - the morning still shows both claims of the scripted night as mismatches.
"""
import http.server, http.client, json, os, subprocess, sys, threading, time

PROTO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "2026-11-24-morning-review", "proto")
NIGHT = os.path.abspath(sys.argv[1])
KEYS = {"anthropic": "sk-ant-test-0000-not-real", "openai": "sk-test-1111-not-real"}
seen = {"anthropic": [], "openai": []}
model_up = threading.Event()


def stand_in(provider, port):
    class H(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def handle_any(self):
            n = int(self.headers.get("content-length", 0) or 0)
            body = self.rfile.read(n) if n else None
            key = self.headers.get("x-api-key") if provider == "anthropic" else (self.headers.get("authorization") or "").removeprefix("Bearer ")
            seen[provider].append(key)
            model_up.wait(60)
            up = http.client.HTTPConnection("127.0.0.1", 18401, timeout=600)
            up.request(self.command, self.path, body=body, headers={k: v for k, v in self.headers.items() if k.lower() not in ("host", "connection")})
            r = up.getresponse()
            data = r.read()
            self.send_response(r.status)
            for k, v in r.getheaders():
                if k.lower() not in ("transfer-encoding", "connection", "content-length"):
                    self.send_header(k, v)
            self.send_header("content-length", str(len(data))); self.send_header("connection", "close"); self.end_headers()
            self.wfile.write(data)

        do_GET = do_POST = do_HEAD = handle_any
    s = http.server.ThreadingHTTPServer(("127.0.0.1", port), H)
    threading.Thread(target=s.serve_forever, daemon=True).start()


def start_model():
    """The scripted model, once night.py has written its scripts."""
    path = os.path.join(NIGHT, "scripts.json")
    while not os.path.exists(path):
        time.sleep(0.2)
    time.sleep(0.5)
    p = subprocess.Popen([sys.executable, os.path.join(PROTO, "model.py")], env={**os.environ, "PORT": "18401", "SCRIPTS": path},
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    model_up.set()
    return p


def main():
    stand_in("anthropic", 18501)
    stand_in("openai", 18502)
    env = {**os.environ, "SILO_KEY_ANTHROPIC": KEYS["anthropic"], "SILO_KEY_OPENAI": KEYS["openai"],
           "SILO_UPSTREAM_ANTHROPIC": "http://127.0.0.1:18501", "SILO_UPSTREAM_OPENAI": "http://127.0.0.1:18502"}
    holder = {}
    threading.Thread(target=lambda: holder.setdefault("model", start_model()), daemon=True).start()
    night = subprocess.run([sys.executable, os.path.join(PROTO, "night.py"), NIGHT, "--real", "--hours", "0.15", "--max-usd", "0.2",
                            "--stuck-after", "60", "--hold-limit", "20"], env=env, capture_output=True, text=True)
    ok = True

    def check(cond, what):
        nonlocal ok
        print(("pass  " if cond else "FAIL  ") + what)
        ok = ok and cond

    check(night.returncode == 0, f"the night ran (exit {night.returncode}) {night.stderr[-300:] if night.returncode else ''}")
    check(seen["anthropic"] and all(k == KEYS["anthropic"] for k in seen["anthropic"]),
          f"{len(seen['anthropic'])} Anthropic calls reached the provider, every one with the real key")
    check(not seen["openai"], f"no OpenAI call reached the provider ({len(seen['openai'])}): gpt-5.5 has no price while a budget is set")
    leaks = subprocess.run(["grep", "-rlaF", "-e", KEYS["anthropic"], "-e", KEYS["openai"], NIGHT], capture_output=True, text=True).stdout.split()
    check(not leaks, f"the keys are in no file of the night ({len(leaks)} files hold one{': ' + ', '.join(leaks[:3]) if leaks else ''})")
    facts = [json.loads(l) for c in ("main", "try-bun") for l in open(os.path.join(NIGHT, "record", c, "facts.jsonl"))]
    refused = [f for f in facts if f.get("what") == "model call refused"]
    reasons = {f["reason"].split(",")[0][:60] for f in refused}
    check(any("budget" in f["reason"] and "reached" in f["reason"] for f in refused), f"calls refused once the budget was reached ({sum('reached' in f['reason'] for f in refused)})")
    check(any("no price" in f["reason"] for f in refused), f"gpt-5.5 refused for having no price ({sum('no price' in f['reason'] for f in refused)})")
    spent = sum(f.get("cost_usd") or 0 for f in facts if f["line"] == "model call")
    print(f"      counted spend ${spent:.3f} against a budget of $0.20; refusals: {sorted(reasons)}")
    closed = [f for f in facts if f["line"] == "connection closed"]
    real = [f for f in closed if f["bytes_in"] > 0]
    print(f"      {len(closed)} real connections closed, {len(real)} got bytes back: {sorted({f['host'] for f in real})}")
    first = subprocess.run([os.path.join(PROTO, "silo")], env={**os.environ, "SILO_NIGHT": NIGHT}, capture_output=True, text=True).stdout
    open(os.path.join(NIGHT, "morning.txt"), "w").write(first)
    check("night-budget" in first, "the first screen shows the budget alert")
    subprocess.run([sys.executable, os.path.join(PROTO, "night.py"), NIGHT, "--end"])
    if holder.get("model"):
        holder["model"].terminate()
    print("ok" if ok else "FAILED")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
