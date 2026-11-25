"""Tests of the ledger server: concurrent Macs, and the ledger killed while they ask.

python3 test_ledger.py            all tests, prints what each measured
python3 test_ledger.py crash      one test
"""
import http.client, json, os, random, shutil, signal, socket, statistics, subprocess, sys, tempfile, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


class Server:
    def __init__(self, budget, policy="reserve_typical", chunk=2.0, env=None):
        self.dir = tempfile.mkdtemp()
        self.port, self.budget, self.policy, self.chunk = free_port(), budget, policy, chunk
        self.env = dict(os.environ, **(env or {}))
        self.p = None
        self.mu = threading.Lock()
        self.starts = 0

    def start(self):
        self.starts += 1
        self.p = subprocess.Popen([sys.executable, os.path.join(HERE, "server.py"), "--db", os.path.join(self.dir, "l.db"),
                                   "--port", str(self.port), "--budget", f"team={self.budget}", "--policy", self.policy,
                                   "--chunk", str(self.chunk)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=self.env)
        self.p.stdout.readline()

    def kill(self):
        self.p.send_signal(signal.SIGKILL)
        self.p.wait()

    def stop(self):
        if self.p and self.p.poll() is None:
            self.kill()
        shutil.rmtree(self.dir, ignore_errors=True)


def call(port, method, path, body=None, timeout=5):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    try:
        c.request(method, path, json.dumps(body) if body is not None else None, {"content-type": "application/json"})
        return json.loads(c.getresponse().read())
    finally:
        c.close()


def test_central():
    """12 Macs admit and settle calls as fast as they can under $15. Each call costs at most what it
    reserved, so the ledger must never let the total pass the budget."""
    s = Server(15.0)
    s.start()
    lat, lock = [], threading.Lock()

    def mac(i):
        rng = random.Random(i)
        n, refused = 0, 0
        while refused < 3:
            n += 1
            cid = f"m{i}-{n}"
            t = time.perf_counter()
            r = call(s.port, "POST", "/admit", {"budget": "team", "call": cid, "input_cost": 0.01, "reserve": 0.25})
            with lock:
                lat.append(time.perf_counter() - t)
            if not r["ok"]:
                refused += 1
                time.sleep(0.01)
                continue
            time.sleep(rng.uniform(0, 0.02))
            call(s.port, "POST", "/settle", {"budget": "team", "call": cid, "cost": rng.uniform(0.05, 0.25)})
    ts = [threading.Thread(target=mac, args=(i,)) for i in range(12)]
    t0 = time.perf_counter()
    [t.start() for t in ts]
    [t.join() for t in ts]
    took = time.perf_counter() - t0
    st = call(s.port, "GET", "/state?budget=team")
    s.stop()
    lat.sort()
    ok = st["settled"] <= 15.0 and st["in_flight"] == 0
    print(f"central: {len(lat)} admissions from 12 Macs in {took:.1f} s; settled ${st['settled']:.4f} of $15; "
          f"admission p50 {1000 * lat[len(lat) // 2]:.2f} ms, p99 {1000 * lat[int(len(lat) * 0.99)]:.2f} ms on localhost with fsync. "
          f"{'PASS' if ok else 'FAIL'}")
    return ok


def test_crash(with_req=True, kills=25, macs=12, crash_share=0.05):
    """12 Macs ask for slices of $300 while the ledger dies: it is killed with SIGKILL 25 times, and
    it also dies on its own right after committing 5% of its grants, before answering. A Mac counts
    a grant only once it has the answer, and sends the same ask again after a failed request. The
    ledger must never have given more than the budget, and no grant a Mac holds may be missing from
    its database. A grant committed whose answer was lost is never used: the test reports it as
    stranded."""
    s = Server(300.0, chunk=0.05, env={"LEDGER_CRASH_AFTER_COMMIT": str(crash_share)})
    s.start()
    seen = [0.0] * macs
    stop = threading.Event()
    errors = [0]

    def mac(i):
        rng, spent, req, zero = random.Random(i), 0.0, 0, 0
        while not stop.is_set() and zero < 5:
            req += 1
            while not stop.is_set():
                try:
                    body = {"budget": "team", "mac": f"m{i}", "spent": spent}
                    if with_req:
                        body["req"] = f"m{i}-{req}"
                    r = call(s.port, "POST", "/lease", body, timeout=2)
                    break
                except (OSError, http.client.HTTPException, json.JSONDecodeError):
                    errors[0] += 1
                    time.sleep(0.02)
            else:
                return
            g = r["granted"]
            seen[i] += g
            zero = zero + 1 if g <= 1e-9 else 0
            spent += g  # this Mac spends its whole slice before asking again
            time.sleep(rng.uniform(0.02, 0.1))

    def supervise():
        rng = random.Random(99)
        next_kill, done_kills = time.time() + rng.uniform(0.05, 0.3), 0
        while not stop.is_set():
            with s.mu:
                if s.p.poll() is not None:
                    s.start()
                elif done_kills < kills and time.time() >= next_kill:
                    s.kill()
                    s.start()
                    done_kills += 1
                    next_kill = time.time() + rng.uniform(0.05, 0.3)
            time.sleep(0.005)
    ts = [threading.Thread(target=mac, args=(i,)) for i in range(macs)]
    sup = threading.Thread(target=supervise)
    sup.start()
    [t.start() for t in ts]
    [t.join() for t in ts]
    stop.set()
    sup.join()
    with s.mu:
        if s.p.poll() is not None:
            s.start()
    st = call(s.port, "GET", "/state?budget=team")
    starts = s.starts
    s.stop()
    db = {l["mac"]: l["granted"] for l in st["leases"]}
    lost = [i for i in range(macs) if seen[i] > db.get(f"m{i}", 0) + 1e-9]
    granted = sum(db.values())
    stranded = granted - sum(seen)
    ok = granted <= 300.0 + 1e-9 and not lost
    print(f"crash ({'with' if with_req else 'without'} request ids): the ledger started {starts} times, {errors[0]} failed requests; "
          f"granted ${granted:.4f} of $300, Macs hold ${sum(seen):.4f}, stranded ${stranded:.4f}; "
          f"grants missing from the database: {len(lost)}. {'PASS' if ok else 'FAIL'}")
    return ok, stranded


if __name__ == "__main__":
    which = sys.argv[1:] or ["central", "crash"]
    results = []
    if "central" in which:
        results.append(test_central())
    if "crash" in which:
        ok1, stranded_without = test_crash(with_req=False)
        ok2, stranded_with = test_crash(with_req=True)
        results += [ok1, ok2]
        print(f"stranded by lost answers: ${stranded_without:.2f} without request ids, ${stranded_with:.2f} with them")
    sys.exit(0 if all(results) else 1)
