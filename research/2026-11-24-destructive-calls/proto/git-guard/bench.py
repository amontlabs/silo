"""Scale check for the ancestry methods on bigger pushes, without HTTP.
Builds a synthetic history with git fast-import, makes the pack a push would send
(pack-objects --revs --thin --delta-base-offset, as send-pack does), and times:
read_pack, the pack walk, and git merge-base --is-ancestor on the server repo.
Usage: python3 bench.py <workdir>"""
import json, os, random, shutil, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import receivepack as rp

W = os.path.abspath(sys.argv[1])
HERE = os.path.dirname(os.path.abspath(__file__))
ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT")}
ENV.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", GIT_AUTHOR_NAME="A", GIT_AUTHOR_EMAIL="a@example.invalid", GIT_COMMITTER_NAME="A", GIT_COMMITTER_EMAIL="a@example.invalid")


def g(*a, inp=None):
    r = subprocess.run(["git", "-C", REPO, *a], input=inp, capture_output=True, env=ENV)
    if r.returncode not in (0, 1):
        raise RuntimeError(r.stderr.decode())
    return r


def fast_import(n_main, files=400, branch_at=None, n_branch=0, rebase_n=0):
    """main: n_main commits, each edits one text file (so commits and blobs delta well)."""
    rnd = random.Random(7)
    content = {f"src/f{i:04d}.txt": "\n".join(f"line {j} of {i}" for j in range(60)) + "\n" for i in range(files)}
    out = []
    mark = 0
    def commit(ref, parent_mark, msg, edits):
        nonlocal mark
        mark += 1
        out.append(f"commit {ref}\nmark :{mark}\ncommitter A <a@example.invalid> {1700000000 + mark} +0000\n"
                   f"data {len(msg)}\n{msg}\n")
        if parent_mark:
            out.append(f"from :{parent_mark}\n")
        for p, c in edits:
            b = c.encode()
            out.append(f"M 100644 inline {p}\ndata {len(b)}\n{c}\n")
        return mark
    first = commit("refs/heads/main", None, "init", list(content.items()))
    marks = [first]
    for k in range(n_main - 1):
        p = f"src/f{rnd.randrange(files):04d}.txt"
        lines = content[p].split("\n")
        lines[rnd.randrange(60)] = f"edited {k} {rnd.random()}"
        content[p] = "\n".join(lines)
        marks.append(commit("refs/heads/main", marks[-1], f"commit {k} on main\n\nsome body text {k}", [(p, content[p])]))
    return "".join(out).encode() + b"done\n", marks


def run_case(name, n_main, old_idx, new_spec):
    """old_idx: index of main's commit the server has as main.
    new_spec: ("ff", None) the tip of main; ("rebase", k): k commits rewritten on top of old's parent."""
    global REPO
    REPO = os.path.join(W, "bench", name)
    shutil.rmtree(REPO, ignore_errors=True)
    os.makedirs(REPO)
    g("init", "-q", "--bare")
    stream, _ = fast_import(n_main)
    t0 = time.perf_counter()
    subprocess.run(["git", "-C", REPO, "fast-import", "--quiet", "--done"], input=stream, check=True, env=ENV)
    revs = g("rev-list", "--reverse", "main").stdout.decode().split()
    old = revs[old_idx]
    if new_spec[0] == "ff":
        new = revs[-1]
    else:
        # rewrite the last k commits before old: cherry-pick-like by amending messages
        k = new_spec[1]
        base = revs[old_idx - k]
        new = base
        for c in revs[old_idx - k + 1: old_idx + 1]:
            tree = g("rev-parse", c + "^{tree}").stdout.decode().strip()
            new = g("commit-tree", tree, "-p", new, "-m", "rewritten " + c[:8]).stdout.decode().strip()
    # The server repo: everything up to old only. Simulate by a second bare repo that has old.
    SRV = REPO + "-server"
    shutil.rmtree(SRV, ignore_errors=True)
    subprocess.run(["git", "init", "-q", "--bare", SRV], check=True, env=ENV)
    subprocess.run(["git", "-C", REPO, "push", "-q", SRV, f"{old}:refs/heads/main"], check=True, env=ENV)
    # the pack a push would send
    pack = subprocess.run(["git", "-C", REPO, "pack-objects", "--revs", "--thin", "--stdout",
                           "--delta-base-offset", "-q"],
                          input=f"{new}\n^{old}\n".encode(), capture_output=True, check=True, env=ENV).stdout
    body = rp.pkt(f"{old} {new} refs/heads/main\0report-status side-band-64k\n".encode()) + b"0000" + pack
    res = {"case": name, "server_commits": old_idx + 1, "pack_mb": round(len(pack) / 1e6, 2)}
    t0 = time.perf_counter()
    push = rp.parse_request(body)
    commits, stats = rp.read_pack(body, push.pack_offset)
    res["read_pack_ms"] = round((time.perf_counter() - t0) * 1000, 1)
    res.update(stats)
    t0 = time.perf_counter()
    c2, st2 = rp.read_pack(body, push.pack_offset, commits_only=True)
    res["read_commits_only_ms"] = round((time.perf_counter() - t0) * 1000, 1)
    res["commits_only_same_graph"] = c2 == commits
    t0 = time.perf_counter()
    verdict, boundary = rp.walk(old, new, commits)
    res["walk_ms"] = round((time.perf_counter() - t0) * 1000, 2)
    res["pack_verdict"] = verdict
    res["boundary"] = len(boundary)
    def anc(a, b, graph):
        t0 = time.perf_counter()
        r = subprocess.run(["git", "-C", SRV, "-c", f"core.commitGraph={graph}", "merge-base",
                            "--is-ancestor", a, b], capture_output=True, env=ENV).returncode
        return r, round((time.perf_counter() - t0) * 1000, 1)
    if boundary:
        b = next(iter(boundary))
        r, ms = anc(old, b, "false")
        res["server_is_ancestor(old, boundary)"] = {0: True, 1: False}.get(r, r)
        res["server_ask_ms_no_commit_graph"] = ms
        subprocess.run(["git", "-C", SRV, "commit-graph", "write", "--reachable"], check=True, env=ENV)
        res["server_ask_ms_commit_graph"] = anc(old, b, "true")[1]
    # worst case for a server ask: old far back, b near the tip of a long history
    res["server_ask_root_vs_old_ms"] = anc(revs[0], old, "true")[1]
    res["decision"] = verdict if verdict != "unknown" else (
        "ff" if res.get("server_is_ancestor(old, boundary)") else "force")
    return res


if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    out = []
    for args in [("ff-10-of-5k", 5000, 4989, ("ff", None)),
                 ("ff-1000-of-5k", 5000, 3999, ("ff", None)),
                 ("ff-10000-of-20k", 20000, 9999, ("ff", None)),
                 ("rebase-20-of-5k", 5000, 4999, ("rebase", 20)),
                 ("rebase-500-of-20k", 20000, 19999, ("rebase", 500))]:
        r = run_case(*args)
        print(json.dumps(r), flush=True)
        out.append(r)
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    json.dump(out, open(os.path.join(HERE, "results", "bench.json"), "w"), indent=1)
