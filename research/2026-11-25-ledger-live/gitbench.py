"""Leases through a Git branch (#388's `gitlease.py`), measured over a network.

  python3 gitbench.py local RTT_MS MACS [PACE_S]   a bare repository served by `git daemon`, reached
                                                  through netem.py's relay with RTT_MS of delay
  python3 gitbench.py hosted URL BRANCH MACS [PACE_S]  a real hosted repository over HTTPS; BRANCH
                                                  is created with the ledger file as its only file

MACS Macs ask for slices of a budget of $2 per Mac and per round, as #388's test does: each spends
its whole slice and asks again, until the ledger says nothing is left. With PACE_S, each Mac waits
that long (uniform 0 to 2x) between asks, as Macs do in a team run (a dozen calls between asks);
without it, every Mac asks at once, the worst case. Prints one JSON line.
"""
import json, os, random, shutil, socket, subprocess, sys, tempfile, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "2026-11-25-team-ledger", "ledger"))
from gitlease import GitLedger, git  # noqa: E402


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def seed(url, branch, budget, d):
    s = os.path.join(d, "seed")
    git(d, "init", "-q", s)
    git(s, "remote", "add", "origin", url)
    _, blob, _ = git(s, "hash-object", "-w", "--stdin", input=json.dumps({"budget": budget, "macs": {}}) + "\n")
    _, tree, _ = git(s, "mktree", input=f"100644 blob {blob}\tledger.json\n")
    env_commit = ["-c", "user.name=ledger", "-c", "user.email=ledger@example.invalid"]
    # A new round starts on top of the branch when it is already there, so no push is ever forced.
    rc, _, _ = git(s, "fetch", "-q", "origin", f"+{branch}:refs/remotes/origin/ledger")
    parent = ["-p", "refs/remotes/origin/ledger"] if rc == 0 else []
    r = subprocess.run(["git", *env_commit, "commit-tree", tree, *parent, "-m", f"a team budget of ${budget}"], cwd=s, capture_output=True, text=True)
    commit = r.stdout.strip()
    rc, _, err = git(s, "push", "-q", "origin", f"{commit}:{branch}")
    if rc:
        raise SystemExit(f"could not create the ledger branch: {err}")


def bench(url, branch, macs, pace, d, label):
    budget = 2.0 * macs * 5
    seed(url, branch, budget, d)
    lat, lock = [], threading.Lock()
    seen, stats, timing = [0.0] * macs, [], []

    def mac(i):
        clone = os.path.join(d, f"mac{i}")
        git(d, "init", "-q", clone)
        git(clone, "remote", "add", "origin", url)
        for k, v in (("user.name", f"mac{i}"), ("user.email", f"mac{i}@example.invalid")):
            git(clone, "config", k, v)
        led, spent, rng = GitLedger(clone, f"m{i}", branch=branch), 0.0, random.Random(i)
        if pace:
            time.sleep(rng.uniform(0, pace))
        while True:
            t = time.perf_counter()
            g = led.lease(spent)
            with lock:
                lat.append(time.perf_counter() - t)
            seen[i] += g
            spent += g
            if g <= 1e-9:
                break
            time.sleep(rng.uniform(0, 2 * pace) if pace else rng.uniform(0.0, 0.05))
        stats.append((led.tries, led.refused))
        with lock:
            timing.extend(led.timing)
    ts = [threading.Thread(target=mac, args=(i,)) for i in range(macs)]
    t0 = time.perf_counter()
    [t.start() for t in ts]
    [t.join() for t in ts]
    took = time.perf_counter() - t0
    chk = os.path.join(d, "check")
    git(d, "init", "-q", chk)
    git(chk, "remote", "add", "origin", url)
    git(chk, "fetch", "-q", "origin", f"+{branch}:refs/remotes/origin/ledger")
    _, text, _ = git(chk, "show", "refs/remotes/origin/ledger:ledger.json")
    st = json.loads(text)
    granted = sum(m["granted"] for m in st["macs"].values())
    lat.sort()
    n = len(lat)
    tries = sum(t for t, _ in stats)
    refused = sum(r for _, r in stats)
    return {"where": label, "macs": macs, "pace_s": pace, "asks": n, "seconds": round(took, 1),
            "pushes_tried": tries, "pushes_refused": refused, "granted": round(granted, 4), "budget": budget,
            "held_by_macs": round(sum(seen), 4), "ok": granted <= budget + 1e-9 and abs(granted - sum(seen)) < 1e-6,
            "ask_p50_ms": round(1000 * lat[n // 2]), "ask_p90_ms": round(1000 * lat[int(n * 0.9)]),
            "ask_p99_ms": round(1000 * lat[min(n - 1, int(n * 0.99))]), "ask_max_ms": round(1000 * lat[-1]),
            "read_p50_ms": med([r for r, _, _ in timing]), "push_ok_p50_ms": med([p for _, p, ok in timing if ok]),
            "push_refused_p50_ms": med([p for _, p, ok in timing if not ok])}


def med(xs):
    xs = sorted(xs)
    return round(1000 * xs[len(xs) // 2]) if xs else None


def local(rtt_ms, macs, pace):
    d = tempfile.mkdtemp()
    try:
        root = os.path.join(d, "srv")
        os.makedirs(root)
        git(root, "init", "-q", "--bare", "team.git")
        git(os.path.join(root, "team.git"), "config", "receive.denyNonFastForwards", "true")
        gport, rport = free_port(), free_port()
        daemon = subprocess.Popen(["git", "daemon", "--reuseaddr", f"--base-path={root}", "--export-all", "--enable=receive-pack",
                                   f"--port={gport}", "--listen=127.0.0.1", root], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        ctl = os.path.join(d, "ctl.json")
        with open(ctl, "w") as f:
            json.dump({"down": []}, f)
        net = subprocess.Popen([sys.executable, os.path.join(HERE, "netem.py"), "--ctl", ctl, "--map", f"{rport}:{gport}:{rtt_ms}"],
                               stdout=subprocess.PIPE)
        net.stdout.readline()
        time.sleep(0.3)
        try:
            return bench(f"git://127.0.0.1:{rport}/team.git", "refs/heads/silo-ledger", macs, pace, d, f"git daemon, {rtt_ms} ms")
        finally:
            daemon.kill()
            net.kill()
    finally:
        shutil.rmtree(d, ignore_errors=True)


def hosted(url, branch, macs, pace):
    d = tempfile.mkdtemp()
    try:
        return bench(url, f"refs/heads/{branch}", macs, pace, d, "hosted")
    finally:
        shutil.rmtree(d, ignore_errors=True)


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[0] == "local":
        r = local(float(a[1]), int(a[2]), float(a[3]) if len(a) > 3 else 0.0)
    else:
        r = hosted(a[1], a[2], int(a[3]), float(a[4]) if len(a) > 4 else 0.0)
    print(json.dumps(r))
