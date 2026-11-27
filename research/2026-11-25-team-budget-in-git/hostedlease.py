"""Leases through a branch of a hosted Git repository (GitHub), measured.

#388's gitlease.py, pointed at a real hosted repository instead of a local bare one. The ledger is
one file, ledger.json, at the top of the branch's tree; every other file of the tree is kept, so
the branch stays a normal branch. Each "Mac" is its own clone (depth 1) in a temporary folder.

Two ways of asking:
  read-first   fetch, add the grant, push on top of what was fetched (#388's way): 2 round trips
  optimistic   push on top of the last head this Mac knows; fetch only when the push is refused:
               1 round trip when nobody else pushed since

python3 hostedlease.py REMOTE BRANCH MODE MACS BUDGET CHUNK OUT.json [--gap SECONDS]

Writes every ask (Mac, start, end, tries, granted) to OUT.json and prints a summary. The run ends
when every Mac has been told twice that nothing is left. PASS when the ledger never granted more
than the budget and every grant a Mac holds is in the ledger.
"""
import json, os, random, shutil, subprocess, sys, tempfile, threading, time

LEDGER = "ledger.json"


def git(cwd, *args, input=None):
    r = subprocess.run(["git", *args], cwd=cwd, input=input, capture_output=True, text=True)
    return r.returncode, r.stdout.strip(), r.stderr.strip()


class Mac:
    def __init__(self, root, remote, branch, name, chunk, mode):
        self.dir = os.path.join(root, name)
        self.branch, self.name, self.chunk, self.mode = branch, name, chunk, mode
        git(root, "init", "-q", self.dir)
        git(self.dir, "remote", "add", "origin", remote)
        for k, v in (("user.name", f"ledger {name}"), ("user.email", f"{name}@example.invalid")):
            git(self.dir, "config", k, v)
        self.head = None
        self.pushes = self.fetches = self.refused = 0
        self.errors = []

    def fetch(self):
        self.fetches += 1
        rc, _, err = git(self.dir, "fetch", "-q", "--depth=1", "origin",
                         f"+refs/heads/{self.branch}:refs/remotes/origin/ledger")
        if rc != 0:
            self.errors.append(("fetch", err[-300:]))
            return False
        _, self.head, _ = git(self.dir, "rev-parse", "refs/remotes/origin/ledger")
        return True

    def state(self):
        _, text, _ = git(self.dir, "show", f"{self.head}:{LEDGER}")
        return json.loads(text)

    def write(self, st, msg):
        _, blob, _ = git(self.dir, "hash-object", "-w", "--stdin",
                         input=json.dumps(st, indent=1, sort_keys=True) + "\n")
        _, lstree, _ = git(self.dir, "ls-tree", self.head)
        lines = [l for l in lstree.splitlines() if l.split("\t", 1)[1] != LEDGER]
        lines.append(f"100644 blob {blob}\t{LEDGER}")
        _, tree, _ = git(self.dir, "mktree", "--missing", input="\n".join(lines) + "\n")
        _, commit, _ = git(self.dir, "commit-tree", tree, "-p", self.head, "-m", msg)
        self.pushes += 1
        rc, _, err = git(self.dir, "push", "-q", "origin", f"{commit}:refs/heads/{self.branch}")
        if rc == 0:
            self.head = commit
            return True
        if "rejected" in err or "non-fast-forward" in err or "fetch first" in err:
            self.refused += 1
        else:
            self.errors.append(("push", err[-300:]))
        return False

    def lease(self, spent):
        tries = 0
        if self.mode == "read-first" or self.head is None:
            self.fetch()
        while True:
            tries += 1
            st = self.state()
            me = st["macs"].setdefault(self.name, {"granted": 0.0, "given": 0.0, "reported": 0.0, "asked": 0})
            me["reported"] = max(me["reported"], spent)
            now = time.time()
            me["asked"] = now
            used = sum(max(m["granted"] - m["given"], m["reported"]) for m in st["macs"].values())
            free = max(0.0, st["budget"] - used)
            active = sum(1 for m in st["macs"].values() if now - m["asked"] < 60)
            g = round(min(free, max(self.chunk, free / (2 * max(1, active)))), 6)
            me["granted"] = round(me["granted"] + g, 6)
            if self.write(st, f"lease {self.name} {g:.4f}"):
                return g, tries
            while not self.fetch():
                time.sleep(0.5)


def run(remote, branch, mode, macs, budget, chunk, out, gap):
    root = tempfile.mkdtemp()
    seed = Mac(root, remote, branch, "seed", chunk, mode)
    assert seed.fetch(), seed.errors
    assert seed.write({"budget": budget, "macs": {}}, f"a team budget of ${budget}"), seed.errors
    ms = [Mac(root, remote, branch, f"m{i}", chunk, mode) for i in range(macs)]
    seen = [0.0] * macs
    asks, lock = [], threading.Lock()

    def go(i):
        m, spent, zero, rng = ms[i], 0.0, 0, random.Random(i)
        while zero < 2:
            t0 = time.time()
            g, tries = m.lease(spent)
            t1 = time.time()
            with lock:
                asks.append({"mac": m.name, "start": t0, "end": t1, "tries": tries, "granted": g})
            seen[i] += g
            spent += g
            zero = zero + 1 if g <= 1e-9 else 0
            time.sleep(rng.uniform(0, gap))

    ts = [threading.Thread(target=go, args=(i,)) for i in range(macs)]
    t0 = time.time()
    [t.start() for t in ts]
    [t.join() for t in ts]
    took = time.time() - t0
    seed.fetch()
    st = seed.state()
    granted = sum(m["granted"] for m in st["macs"].values())
    lost = [i for i in range(macs) if seen[i] > st["macs"].get(f"m{i}", {}).get("granted", 0) + 1e-6]
    ok = granted <= budget + 1e-6 and not lost and abs(granted - sum(seen)) < 1e-6
    lat = sorted(a["end"] - a["start"] for a in asks)
    q = lambda p: lat[min(len(lat) - 1, int(len(lat) * p))]
    res = {
        "remote": remote.split("@")[-1], "mode": mode, "macs": macs, "budget": budget, "chunk": chunk,
        "gap": gap, "asks": len(asks), "seconds": round(took, 1),
        "pushes": sum(m.pushes for m in ms), "refused": sum(m.refused for m in ms),
        "fetches": sum(m.fetches for m in ms), "errors": [e for m in ms for e in m.errors][:10],
        "granted": round(granted, 6), "held": round(sum(seen), 6), "lost": lost,
        "ask_p50_ms": round(1000 * q(0.5)), "ask_p90_ms": round(1000 * q(0.9)),
        "ask_p99_ms": round(1000 * q(0.99)), "ask_max_ms": round(1000 * lat[-1]), "pass": ok,
    }
    json.dump({"summary": res, "asks": asks}, open(out, "w"), indent=1)
    shutil.rmtree(root)
    print(json.dumps(res))
    return ok


if __name__ == "__main__":
    a = sys.argv[1:]
    gap = 0.05
    if "--gap" in a:
        k = a.index("--gap"); gap = float(a[k + 1]); del a[k:k + 2]
    remote, branch, mode, macs, budget, chunk, out = a
    sys.exit(0 if run(remote, branch, mode, int(macs), float(budget), float(chunk), out, gap) else 1)
