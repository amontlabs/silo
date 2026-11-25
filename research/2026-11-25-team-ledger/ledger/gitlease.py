"""Leases with no server: the team's ledger is one JSON file on a branch of a Git repository the
team already has. A push that is not a fast-forward is refused, so a push is a compare-and-swap:
a Mac reads the file, adds its grant, commits on top of what it read and pushes; if another Mac
pushed first, the push is refused and the Mac reads again.

Only leases work this way. An admission per model call would be a push per call.

python3 gitlease.py test [macs] [budget]   12 Macs against a bare repository in a temporary folder
"""
import json, os, random, shutil, subprocess, sys, tempfile, threading, time

BRANCH = "refs/heads/silo-ledger"


def git(cwd, *args, input=None):
    r = subprocess.run(["git", *args], cwd=cwd, input=input, capture_output=True, text=True)
    return r.returncode, r.stdout.strip(), r.stderr.strip()


class GitLedger:
    def __init__(self, clone, mac, chunk=2.0, branch=BRANCH):
        self.clone, self.mac, self.chunk, self.branch = clone, mac, chunk, branch
        self.tries = 0
        self.refused = 0
        self.timing = []  # (seconds to read, seconds to push, pushed)

    def _read(self):
        rc, _, _ = git(self.clone, "fetch", "-q", "origin", f"+{self.branch}:refs/remotes/origin/ledger")
        rc, head, _ = git(self.clone, "rev-parse", "refs/remotes/origin/ledger")
        _, text, _ = git(self.clone, "show", f"{head}:ledger.json")
        return head, json.loads(text)

    def _write(self, parent, state, msg):
        _, blob, _ = git(self.clone, "hash-object", "-w", "--stdin", input=json.dumps(state, indent=1, sort_keys=True) + "\n")
        _, tree, _ = git(self.clone, "mktree", input=f"100644 blob {blob}\tledger.json\n")
        _, commit, _ = git(self.clone, "commit-tree", tree, "-p", parent, "-m", msg)
        rc, _, err = git(self.clone, "push", "-q", "origin", f"{commit}:{self.branch}")
        if rc != 0:
            self.refused += 1
        return rc == 0

    def lease(self, spent):
        """Ask for a slice. Returns the dollars granted, 0 when nothing is left."""
        while True:
            self.tries += 1
            t0 = time.perf_counter()
            head, st = self._read()
            t1 = time.perf_counter()
            me = st["macs"].setdefault(self.mac, {"granted": 0.0, "given": 0.0, "reported": 0.0, "asked": 0})
            me["reported"] = max(me["reported"], spent)
            now = time.time()
            me["asked"] = now
            used = sum(max(m["granted"] - m["given"], m["reported"]) for m in st["macs"].values())
            free = max(0.0, st["budget"] - used)
            active = sum(1 for m in st["macs"].values() if now - m["asked"] < 60)
            g = min(free, max(self.chunk, free / (2 * max(1, active))))
            me["granted"] += g
            ok = self._write(head, st, f"lease {self.mac} {g:.4f}")
            self.timing.append((t1 - t0, time.perf_counter() - t1, ok))
            if ok:
                return g


def setup(budget):
    d = tempfile.mkdtemp()
    bare = os.path.join(d, "team.git")
    git(d, "init", "-q", "--bare", bare)
    seed = os.path.join(d, "seed")
    git(d, "init", "-q", seed)
    git(seed, "remote", "add", "origin", bare)
    _, blob, _ = git(seed, "hash-object", "-w", "--stdin", input=json.dumps({"budget": budget, "macs": {}}) + "\n")
    _, tree, _ = git(seed, "mktree", input=f"100644 blob {blob}\tledger.json\n")
    _, commit, _ = git(seed, "commit-tree", tree, "-m", "a team budget")
    git(seed, "push", "-q", "origin", f"{commit}:{BRANCH}")
    return d, bare


def test(macs=12, budget=30.0):
    d, bare = setup(budget)
    seen, tries, lat = [0.0] * macs, [0] * macs, []
    lock = threading.Lock()

    def mac(i):
        clone = os.path.join(d, f"mac{i}")
        git(d, "init", "-q", clone)
        git(clone, "remote", "add", "origin", bare)
        for k, v in (("user.name", f"mac{i}"), ("user.email", f"mac{i}@example.invalid")):
            git(clone, "config", k, v)
        led, spent, zero, rng = GitLedger(clone, f"m{i}"), 0.0, 0, random.Random(i)
        while zero < 2:
            t = time.perf_counter()
            g = led.lease(spent)
            with lock:
                lat.append(time.perf_counter() - t)
            seen[i] += g
            spent += g
            zero = zero + 1 if g <= 1e-9 else 0
            time.sleep(rng.uniform(0.0, 0.05))
        tries[i] = led.tries
    ts = [threading.Thread(target=mac, args=(i,)) for i in range(macs)]
    t0 = time.perf_counter()
    [t.start() for t in ts]
    [t.join() for t in ts]
    took = time.perf_counter() - t0
    _, text, _ = git(d, "--git-dir", bare, "show", f"{BRANCH}:ledger.json")
    st = json.loads(text)
    _, n, _ = git(d, "--git-dir", bare, "rev-list", "--count", BRANCH)
    shutil.rmtree(d)
    granted = sum(m["granted"] for m in st["macs"].values())
    lost = [i for i in range(macs) if seen[i] > st["macs"].get(f"m{i}", {}).get("granted", 0) + 1e-9]
    lat.sort()
    asks = len(lat)
    ok = granted <= budget + 1e-9 and not lost and abs(granted - sum(seen)) < 1e-9
    print(f"git leases: {macs} Macs, {asks} asks in {took:.1f} s, {sum(tries)} pushes tried for {int(n) - 1} commits; "
          f"granted ${granted:.4f} of ${budget}, Macs hold ${sum(seen):.4f}; "
          f"an ask took p50 {1000 * lat[asks // 2]:.0f} ms, p99 {1000 * lat[int(asks * 0.99)]:.0f} ms on a local bare repository. "
          f"{'PASS' if ok else 'FAIL'}")
    return ok


if __name__ == "__main__":
    if sys.argv[1:2] == ["test"]:
        a = sys.argv[2:]
        sys.exit(0 if test(int(a[0]) if a else 12, float(a[1]) if len(a) > 1 else 30.0) else 1)
    print(__doc__)
