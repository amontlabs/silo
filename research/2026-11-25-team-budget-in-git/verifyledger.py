"""Check every change to a team's ledger branch, as each Mac would before it trusts the file.

With leases in Git, every member can push to the ledger branch, so every member can write any
number in it. A Mac therefore checks each commit since the last head it trusted:

  1. the last trusted head is an ancestor of the new head (no history rewritten);
  2. each commit changes the ledger only for the Mac its message names ("lease NAME ...");
  3. that Mac's grant grows by no more than was free in the parent, and its "given" and
     "reported" never go down;
  4. the budget never changes (a change of budget is the lead's, and would be a signed commit by
     the lead: not modelled here);
  5. no other Mac's entry changes.

On a real team, "the Mac its message names" must be the commit's verified signer (a ruleset that
requires signed commits), not the message; here the message stands in for it.

python3 verifyledger.py GIT_DIR BRANCH [TRUSTED_HEAD]     prints each bad commit, exits 1 if any
python3 verifyledger.py test                               the tamper test on a local repository
"""
import json, os, shutil, subprocess, sys, tempfile

LEDGER = "ledger.json"


def git(cwd, *args, input=None):
    r = subprocess.run(["git", *args], cwd=cwd, input=input, capture_output=True, text=True)
    return r.returncode, r.stdout.strip(), r.stderr.strip()


def ledger_at(gd, c):
    rc, text, _ = git(gd, "show", f"{c}:{LEDGER}")
    return json.loads(text) if rc == 0 else None


def used(st):
    return sum(max(m["granted"] - m["given"], m["reported"]) for m in st["macs"].values())


def check(gd, branch, trusted=None):
    bad = []
    _, head, _ = git(gd, "rev-parse", branch)
    if trusted:
        rc, _, _ = git(gd, "merge-base", "--is-ancestor", trusted, head)
        if rc != 0:
            return [(head, "history rewritten: the trusted head is not in the new history")]
    rng = f"{trusted}..{head}" if trusted else head
    _, revs, _ = git(gd, "rev-list", "--reverse", "--first-parent", rng)
    for c in revs.split():
        _, parents, _ = git(gd, "rev-list", "--parents", "-n", "1", c)
        ps = parents.split()[1:]
        cur = ledger_at(gd, c)
        prev = ledger_at(gd, ps[0]) if ps else None
        if cur is None or prev is None:
            continue  # before the ledger began, or after it was removed
        _, msg, _ = git(gd, "log", "-1", "--format=%s", c)
        if cur["budget"] != prev["budget"]:
            bad.append((c, f"budget changed {prev['budget']} -> {cur['budget']}"))
        who = msg.split()[1] if msg.startswith("lease ") else None
        for name in set(cur["macs"]) | set(prev["macs"]):
            a = prev["macs"].get(name, {"granted": 0.0, "given": 0.0, "reported": 0.0, "asked": 0})
            b = cur["macs"].get(name)
            if b is None:
                bad.append((c, f"{name}'s entry removed"))
                continue
            if a == b:
                continue
            if name != who:
                bad.append((c, f"{msg!r} changed {name}'s entry"))
                continue
            free = max(0.0, prev["budget"] - used(prev))
            if b["granted"] - a["granted"] > free + 1e-6:
                bad.append((c, f"{name} took {b['granted'] - a['granted']:.4f} with {free:.4f} free"))
            if b["granted"] < a["granted"] - 1e-9 or b["given"] < a["given"] - 1e-9 or b["reported"] < a["reported"] - 1e-9:
                bad.append((c, f"{name}'s grant, given or reported went down"))
    return bad


def test():
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import hostedlease
    d = tempfile.mkdtemp()
    bare = os.path.join(d, "team.git")
    git(d, "init", "-q", "--bare", bare)
    seed = os.path.join(d, "seed")
    git(d, "init", "-q", seed)
    git(seed, "remote", "add", "origin", bare)
    git(seed, "-c", "user.name=s", "-c", "user.email=s@example.invalid", "commit", "-q", "--allow-empty", "-m", "start")
    git(seed, "push", "-q", "origin", "HEAD:refs/heads/team")
    out = os.path.join(d, "run.json")
    assert hostedlease.run(bare, "team", "optimistic", 5, 15.0, 2.0, out, 0.02)
    _, honest, _ = git(bare, "rev-parse", "team")
    results = {"honest run": check(bare, "team")}

    def cheat(label, mutate, msg):
        git(bare, "update-ref", "refs/heads/team", honest)
        m = hostedlease.Mac(d, bare, "team", f"cheat{len(results)}", 2.0, "optimistic")
        m.fetch()
        st = m.state()
        mutate(st)
        assert m.write(st, msg), m.errors
        results[label] = check(bare, "team", honest)

    def take_more(st):
        st["macs"]["m3"]["granted"] += 5.0
    cheat("a Mac takes $5 more than is free", take_more, "lease m3 5.0000")

    def lower_other(st):
        st["macs"]["m1"]["granted"] -= 2.0
    cheat("a Mac lowers another Mac's grant", lower_other, "lease m3 0.0000")

    def raise_budget(st):
        st["budget"] += 10.0
    cheat("a Mac raises the budget", raise_budget, "lease m3 0.0000")

    # The Mac that reported the most lowers its own report (a Mac that never reported has nothing
    # to lower, so the test picks one that did).
    top = max(ledger_at(bare, honest)["macs"].items(), key=lambda kv: kv[1]["reported"])[0]

    def own_reported_down(st):
        st["macs"][top]["reported"] = 0.0
    cheat("a Mac lowers its own reported spend", own_reported_down, f"lease {top} 0.0000")

    # A rewritten history: the branch set back to before the run.
    _, first, _ = git(bare, "rev-list", "--max-parents=0", honest)
    git(bare, "update-ref", "refs/heads/team", first)
    results["history rewritten"] = check(bare, "team", honest)
    shutil.rmtree(d)
    ok = not results["honest run"] and all(v for k, v in results.items() if k != "honest run")
    for k, v in results.items():
        print(f"{k}: {'flagged: ' + v[0][1] if v else 'clean'}")
    print("PASS" if ok else "FAIL")
    return ok


if __name__ == "__main__":
    if sys.argv[1:2] == ["test"]:
        sys.exit(0 if test() else 1)
    a = sys.argv[1:]
    bad = check(a[0], a[1], a[2] if len(a) > 2 else None)
    for c, why in bad:
        print(c[:12], why)
    sys.exit(1 if bad else 0)
