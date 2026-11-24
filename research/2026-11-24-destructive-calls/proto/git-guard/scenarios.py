"""Runs real git pushes through guard.py in front of git http-backend and records what happened.
Usage: python3 scenarios.py <workdir>   (writes results/*.json next to this file)
Nothing leaves 127.0.0.1. No token is used."""
import json, os, shutil, signal, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
W = os.path.abspath(sys.argv[1])
REPS = int(os.environ.get("REPS", "5"))
MODES = {"pass": 8443, "observe": 8444, "backup": 8445, "refuse": 8446, "hold": 8447, "split": 8448}
ZERO = "0" * 40


def sh(*a, cwd=None, env=None, check=True):
    r = subprocess.run(a, cwd=cwd, env=env, capture_output=True, text=True)
    if check and r.returncode:
        raise RuntimeError(f"{a}: {r.stderr}")
    return r


def pki():
    d = os.path.join(W, "pki"); os.makedirs(d, exist_ok=True)
    if os.path.exists(f"{d}/leaf.crt"):
        return d
    sh("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", f"{d}/ca.key",
       "-out", f"{d}/ca.crt", "-days", "2", "-subj", "/CN=git-guard test CA")
    sh("openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", f"{d}/leaf.key",
       "-out", f"{d}/leaf.csr", "-subj", "/CN=127.0.0.1")
    open(f"{d}/ext", "w").write("subjectAltName=IP:127.0.0.1\n")
    sh("openssl", "x509", "-req", "-in", f"{d}/leaf.csr", "-CA", f"{d}/ca.crt", "-CAkey", f"{d}/ca.key",
       "-CAcreateserial", "-out", f"{d}/leaf.crt", "-days", "2", "-extfile", f"{d}/ext")
    return d


def client_env():
    home = os.path.join(W, "home"); os.makedirs(home, exist_ok=True)
    cfg = os.path.join(home, "gitconfig")
    open(cfg, "w").write("[user]\n\tname = Agent\n\temail = agent@example.invalid\n"
                         "[init]\n\tdefaultBranch = main\n[advice]\n\tdetachedHead = false\n")
    env = {k: v for k, v in os.environ.items()
           if not k.lower().endswith("_proxy") and not k.startswith("GIT") and "TOKEN" not in k}
    env.update(HOME=home, GIT_CONFIG_GLOBAL=cfg, GIT_CONFIG_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0",
               GIT_SSL_CAINFO=os.path.join(W, "pki", "ca.crt"), NO_PROXY="*", LC_ALL="C")
    return env


ENV = None
SRV = None


def git(*a, cwd, check=True):
    return sh("git", *a, cwd=cwd, env=ENV, check=check)


def commit(cwd, name, content=None, size=None):
    p = os.path.join(cwd, name)
    if size:
        with open(p, "wb") as f:
            f.write(os.urandom(size))
    else:
        with open(p, "a") as f:
            f.write((content or name) + "\n")
    git("add", name, cwd=cwd)
    git("commit", "-q", "-m", f"change {name}", cwd=cwd)
    return git("rev-parse", "HEAD", cwd=cwd).stdout.strip()


def fresh(name, hide=False):
    """A bare repo on the 'server' with main (3 commits), feature, old-feature, tag v1,
    and a client clone of it."""
    bare = os.path.join(SRV, "o", name + ".git")
    shutil.rmtree(bare, ignore_errors=True)
    sh("git", "init", "-q", "--bare", "-b", "main", bare, env=ENV)
    for k, v in [("http.receivepack", "true"), ("receive.advertisePushOptions", "true"),
                 ("receive.denyDeletes", "false"), ("receive.denyNonFastForwards", "false"),
                 ("receive.certNonceSeed", "test-seed")]:
        sh("git", "-C", bare, "config", k, v, env=ENV)
    if hide:
        sh("git", "-C", bare, "config", "transfer.hideRefs", "refs/silo", env=ENV)
    seed = os.path.join(W, "seed", name)
    shutil.rmtree(seed, ignore_errors=True)
    os.makedirs(seed)
    git("init", "-q", cwd=seed)
    for i in range(3):
        commit(seed, f"f{i}")
    git("branch", "feature", cwd=seed); git("branch", "old-feature", cwd=seed)
    git("tag", "-a", "v1", "-m", "v1", cwd=seed)
    git("checkout", "-q", "feature", cwd=seed); commit(seed, "feat0"); git("checkout", "-q", "main", cwd=seed)
    git("push", "-q", bare, "--all", cwd=seed); git("push", "-q", bare, "--tags", cwd=seed)
    return bare


def clone(name, mode):
    c = os.path.join(W, "clients", f"{name}-{mode}")
    shutil.rmtree(c, ignore_errors=True)
    git("clone", "-q", url(name, mode), c, cwd=W)
    git("fetch", "-q", "origin", "+refs/heads/*:refs/remotes/origin/*", cwd=c)
    return c


def url(name, mode):
    return f"https://127.0.0.1:{MODES[mode]}/o/{name}.git"


def server_refs(bare):
    out = sh("git", "-C", bare, "for-each-ref", "--format=%(objectname) %(refname)", env=ENV).stdout
    return {l.split()[1]: l.split()[0] for l in out.splitlines()}


# ---------------------------------------------------------------- scenarios
# each: (name, setup(c) -> push args, expected {ref: class}); setup runs in the client clone.

def s_normal(c):
    commit(c, "n1"); commit(c, "n2")
    return ["push", "origin", "main"]

def s_new_branch(c):
    git("checkout", "-q", "-b", "topic", cwd=c); commit(c, "t1")
    return ["push", "origin", "topic"]

def s_delete_branch(c):
    return ["push", "origin", "--delete", "feature"]

def s_force_main(c):
    git("reset", "-q", "--hard", "HEAD~1", cwd=c); commit(c, "rewritten")
    return ["push", "--force", "origin", "main"]

def s_rewind(c):
    return ["push", "--force", "origin", "HEAD~2:main"]

def s_force_rebase(c):
    # main moves on the server meanwhile? No: the agent rebases feature onto main and force pushes.
    git("checkout", "-q", "feature", cwd=c); git("reset", "-q", "--hard", "main", cwd=c)
    commit(c, "feat-redo")
    return ["push", "--force", "origin", "feature"]

def s_merge_ff(c):
    git("checkout", "-q", "-b", "side", "main~1", cwd=c); commit(c, "side1")
    git("checkout", "-q", "main", cwd=c); git("merge", "-q", "--no-ff", "-m", "merge side", "side", cwd=c)
    return ["push", "origin", "main"]

def s_multi(c):
    git("reset", "-q", "--hard", "HEAD~1", cwd=c); commit(c, "rewritten")
    git("checkout", "-q", "feature", cwd=c); commit(c, "feat1"); git("checkout", "-q", "main", cwd=c)
    return ["push", "--force", "origin", "main", "feature"]

def s_atomic(c):
    s_multi(c)
    return ["push", "--atomic", "--force", "origin", "main", "feature"]

def s_mirror(c):
    commit(c, "m1")
    git("branch", "-D", "old-feature", cwd=c, check=False)
    git("tag", "-d", "v1", cwd=c)
    return ["push", "--mirror", "origin"]

def s_tag_delete(c):
    return ["push", "origin", "--delete", "refs/tags/v1"]

def s_push_options(c):
    commit(c, "po1")
    return ["push", "-o", "ci.skip", "-o", "merge_request.create", "origin", "main"]

def s_large(c):
    commit(c, "big.bin", size=3 * 1024 * 1024)
    return ["push", "origin", "main"]

def s_large_force(c):
    git("reset", "-q", "--hard", "HEAD~1", cwd=c); commit(c, "big.bin", size=3 * 1024 * 1024)
    return ["push", "--force", "origin", "main"]

def s_signed_force(c):
    gh = os.path.join(W, "home", "gnupg")
    ENV["GNUPGHOME"] = gh
    if not os.path.exists(gh):
        os.makedirs(gh, mode=0o700)
        sh("gpg", "--batch", "--passphrase", "", "--quick-gen-key", "Agent <agent@example.invalid>",
           "ed25519", "sign", "1d", env=ENV)
    git("reset", "-q", "--hard", "HEAD~1", cwd=c); commit(c, "rewritten")
    return ["push", "--signed", "--force", "origin", "main"]

def s_touch_silo(c):
    return ["push", "origin", "main:refs/silo/backup/heads/main/forged"]


SCEN = [
    ("normal", s_normal, {"refs/heads/main": "ff"}),
    ("new-branch", s_new_branch, {"refs/heads/topic": "create"}),
    ("delete-branch", s_delete_branch, {"refs/heads/feature": "delete"}),
    ("force-main", s_force_main, {"refs/heads/main": "force"}),
    ("rewind-main", s_rewind, {"refs/heads/main": "force"}),
    ("force-rebase", s_force_rebase, {"refs/heads/feature": "force"}),
    ("merge-ff", s_merge_ff, {"refs/heads/main": "ff"}),
    ("multi", s_multi, {"refs/heads/main": "force", "refs/heads/feature": "ff"}),
    ("atomic", s_atomic, {"refs/heads/main": "force", "refs/heads/feature": "ff"}),
    ("mirror", s_mirror, {"refs/heads/main": "ff", "refs/heads/feature": "delete", "refs/heads/old-feature": "delete",
                          "refs/tags/v1": "delete"}),
    ("tag-delete", s_tag_delete, {"refs/tags/v1": "delete"}),
    ("push-options", s_push_options, {"refs/heads/main": "ff"}),
    ("large", s_large, {"refs/heads/main": "ff"}),
    ("large-force", s_large_force, {"refs/heads/main": "force"}),
    ("signed-force", s_signed_force, {"refs/heads/main": "force"}),
    ("touch-silo", s_touch_silo, {"refs/silo/backup/heads/main/forged": "create"}),
]


def guard_records(mode, since):
    out = []
    for l in open(os.path.join(W, f"guard-{mode}.log")):
        r = json.loads(l)
        if r["t"] >= since and r["path"].endswith("git-receive-pack"):
            out.append(r)
    return out


def backend_records(since):
    return [json.loads(l) for l in open(os.path.join(W, "backend.log"))
            if json.loads(l)["t"] >= since and json.loads(l)["path"].endswith("git-receive-pack")]


def run_one(name, fn, mode, hide=False, rep=0):
    tag = f"{name}{'-hide' if hide else ''}"
    bare = fresh(tag, hide)
    c = clone(tag, mode)
    if name == "mirror":
        # in backup mode a previous backup ref exists on the server: mirror will try to delete it
        sh("git", "-C", bare, "update-ref", "refs/silo/backup/heads/main/earlier",
           server_refs(bare)["refs/heads/main"], env=ENV)
    args = fn(c)
    before = server_refs(bare)
    t = time.time()
    t0 = time.perf_counter()
    r = git(*args, cwd=c, check=False)
    wall = round((time.perf_counter() - t0) * 1000, 1)
    after = server_refs(bare)
    rec = {"scenario": tag, "mode": mode, "rep": rep, "cmd": "git " + " ".join(args),
           "exit": r.returncode, "wall_ms": wall, "stderr": r.stderr.strip().splitlines()[-12:],
           "changed": {k: [before.get(k), after.get(k)] for k in set(before) | set(after)
                       if before.get(k) != after.get(k)}}
    time.sleep(0.05)
    rec["guard"] = guard_records(mode, t)
    rec["backend"] = backend_records(t)
    if mode in ("backup", "hold"):
        # do the backups keep the old commits alive after an aggressive gc?
        sh("git", "-C", bare, "reflog", "expire", "--expire=now", "--all", env=ENV)
        sh("git", "-C", bare, "gc", "-q", "--prune=now", env=ENV)
        kept = {}
        for g in rec["guard"]:
            for b in g.get("backups", []):
                kept[b["ref"]] = sh("git", "-C", bare, "cat-file", "-e", b["sha"], env=ENV,
                                    check=False).returncode == 0
        rec["old_commit_survives_gc"] = kept
        # what a plain clone of the server fetches
        cc = os.path.join(W, "clients", f"{tag}-{mode}-reclone")
        shutil.rmtree(cc, ignore_errors=True)
        git("clone", "-q", "--mirror", url(tag, "pass"), cc, cwd=W)
        rec["mirror_clone_sees_silo_refs"] = any(k.startswith("refs/silo/") for k in server_refs(cc))
        rec["ls_remote_shows_silo_refs"] = "refs/silo/" in git("ls-remote", url(tag, "pass"), cwd=W).stdout
    return rec


def main():
    global ENV, SRV
    os.makedirs(W, exist_ok=True)
    d = pki()
    ENV = client_env()
    SRV = os.path.join(W, "srv"); os.makedirs(os.path.join(SRV, "o"), exist_ok=True)
    for f in os.listdir(W):
        if f.endswith(".log"):
            os.remove(os.path.join(W, f))
    procs = [subprocess.Popen([sys.executable, os.path.join(HERE, "backend.py"), "--root", SRV,
                               "--log", os.path.join(W, "backend.log")])]
    for m, port in MODES.items():
        procs.append(subprocess.Popen([sys.executable, os.path.join(HERE, "guard.py"), "--port", str(port),
                                       "--root", SRV, "--mode", m, "--hold", "2",
                                       "--cert", f"{d}/leaf.crt", "--key", f"{d}/leaf.key",
                                       "--log", os.path.join(W, f"guard-{m}.log")]))
    time.sleep(1.0)
    results = []
    try:
        only = os.environ.get("ONLY")
        for name, fn, exp in SCEN:
            if only and name not in only.split(","):
                continue
            for mode in MODES:
                reps = REPS if mode in ("pass", "observe", "backup") else 1
                for k in range(reps):
                    rec = run_one(name, fn, mode, rep=k)
                    rec["expected"] = exp
                    results.append(rec)
                    print(name, mode, k, rec["exit"], rec["wall_ms"], flush=True)
            if name == "mirror":
                for mode in ("backup", "split"):
                    rec = run_one(name, fn, mode, hide=True)
                    rec["expected"] = exp
                    results.append(rec)
                    print(name + "-hide", mode, rec["exit"], rec["wall_ms"], flush=True)
    finally:
        for p in procs:
            p.send_signal(signal.SIGTERM)
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    out = os.path.join(HERE, "results", os.environ.get("OUT", "runs.json"))
    json.dump(results, open(out, "w"), indent=1)
    print("wrote", out)


if __name__ == "__main__":
    main()
