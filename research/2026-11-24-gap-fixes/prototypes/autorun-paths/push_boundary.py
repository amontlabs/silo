#!/usr/bin/env python3
"""A stand-in for the boundary on the way of a `git push` over smart HTTP.

    python3 push_boundary.py <upstream.git> <workdir> <port> [--answer held|allow]

It serves the upstream bare repository through `git http-backend`, the CGI program Git ships, and
reads every POST to git-receive-pack before passing it on:

1. It parses the pkt-lines at the head of the body: one ref update per line (old id, new id, ref
   name), the capabilities after the first NUL, a flush, optional push options, then the pack.
2. It indexes the pack into a quarantine folder outside the computer, with the objects of a mirror
   of the upstream as the base, the way `git receive-pack` itself quarantines a push. A thin pack's
   missing bases come from that mirror, never from the computer.
3. It runs autorun.py on each ref update (and on each new commit, for what only an intermediate
   commit carries), and writes what it found to <workdir>/record.jsonl.
4. If something a rule would hold is found, it answers the push with a report-status that refuses
   each ref as held (with --answer held, the default), or passes the original body on unchanged
   (--answer allow, standing in for your yes). Nothing is ever changed in a push that passes.
"""
import gzip, http.server, json, os, shutil, subprocess, sys, tempfile, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ZERO = "0" * 40


# --- pkt-line --------------------------------------------------------------------------------------

def pkt_lines(buf, pos=0):
    """Yield (payload or None for a flush, next position) until a flush, the PACK, or the end."""
    while pos + 4 <= len(buf):
        if buf[pos:pos + 4] == b"PACK":
            return
        n = int(buf[pos:pos + 4], 16)
        if n == 0:
            yield None, pos + 4
            return
        if n in (1, 2):  # delim-pkt, response-end-pkt: protocol v2 only, not used by receive-pack
            yield None, pos + 4
            return
        if n < 4 or pos + n > len(buf):
            raise ValueError(f"bad pkt-line length {n} at {pos}")
        yield buf[pos + 4:pos + n], pos + n
        pos += n


def parse_receive_pack(body):
    """Ref updates, capabilities, push options and pack bytes of a receive-pack request body."""
    cmds, caps, opts, shallow, pos = [], [], [], [], 0
    for payload, pos in pkt_lines(body, 0):
        if payload is None:
            break
        line = payload.rstrip(b"\n")
        if line.startswith(b"shallow "):
            shallow.append(line[8:].decode()); continue
        if line.startswith(b"push-cert"):
            raise ValueError("signed pushes are not handled by this stand-in")
        if b"\0" in line:
            line, c = line.split(b"\0", 1)
            caps = c.decode().split()
        old, new, ref = line.decode().split(" ", 2)
        cmds.append({"old": old, "new": new, "ref": ref})
    if "push-options" in caps and body[pos:pos + 4] != b"PACK":
        for payload, pos in pkt_lines(body, pos):
            if payload is None:
                break
            opts.append(payload.rstrip(b"\n").decode())
    pack = body[pos:] if body[pos:pos + 4] == b"PACK" else b""
    return cmds, caps, opts, shallow, pack


def pkt(data):
    data = data if isinstance(data, bytes) else data.encode()
    return b"%04x" % (len(data) + 4) + data


def report_status(cmds, caps, reasons, messages):
    """A receive-pack result that refuses each ref as held, as Git prints it: ! [remote rejected]."""
    inner = pkt("unpack ok\n") + b"".join(pkt(f"ng {c['ref']} {reasons}\n") for c in cmds) + b"0000"
    if "side-band-64k" not in caps and "side-band" not in caps:
        return inner
    out = b"".join(pkt(b"\x02" + (m + "\n").encode()) for m in messages)  # band 2: shown as "remote: ..."
    for i in range(0, len(inner), 65000):
        out += pkt(b"\x01" + inner[i:i + 65000])
    return out + b"0000"


# --- reading the change outside the computer -------------------------------------------------------

def run(*args, env=None, inp=None):
    p = subprocess.run(args, capture_output=True, env=env, input=inp)
    if p.returncode not in (0, 1):
        raise RuntimeError(f"{' '.join(args)}: {p.stderr.decode(errors='replace')}")
    return p


def quarantine(mirror, pack):
    q = tempfile.mkdtemp(prefix="silo-push-")
    os.makedirs(f"{q}/objects/pack")
    env = dict(os.environ, GIT_OBJECT_DIRECTORY=f"{q}/objects", GIT_ALTERNATE_OBJECT_DIRECTORIES=f"{mirror}/objects")
    if pack:
        p = subprocess.run(["git", "-C", mirror, "index-pack", "--stdin", "--fix-thin", "--strict"], input=pack,
                           capture_output=True, env=env)
        if p.returncode != 0:
            shutil.rmtree(q)
            raise RuntimeError("the pack could not be indexed: " + p.stderr.decode(errors="replace").strip())
    return q, env


def autorun(mirror, env, old, new):
    p = run(sys.executable, "-I", f"{HERE}/autorun.py", mirror, old, new, "--json", env=env)
    return json.loads(p.stdout or b"[]")


def base_for(mirror, env, new):
    """For a new ref: where it leaves the history the upstream already has."""
    new_commits = run("git", "-C", mirror, "rev-list", "--first-parent", "--reverse", new, "--not", "--all", env=env).stdout.decode().split()
    if not new_commits:
        return new  # the commit is already upstream: nothing new
    p = run("git", "-C", mirror, "rev-parse", "--verify", "-q", new_commits[0] + "^", env=env)
    return p.stdout.decode().strip() or ZERO


def examine(mirror, cmds, pack):
    q, env = quarantine(mirror, pack)
    try:
        out = []
        for c in cmds:
            if c["new"] == ZERO:
                out.append({**c, "deleted": True, "findings": []}); continue
            base = c["old"] if c["old"] != ZERO else base_for(mirror, env, c["new"])
            net = autorun(mirror, env, base, c["new"])
            # What only an intermediate commit carries still runs on a checkout of that commit.
            rng = [c["new"], "--not", "--all"] if c["old"] == ZERO else [f"{c['old']}..{c['new']}"]
            commits = run("git", "-C", mirror, "rev-list", "--first-parent", *rng, env=env).stdout.decode().split()
            seen = {(f["path"], f["entry"], tuple(f["details"])) for f in net}
            for sha in commits:
                parent = run("git", "-C", mirror, "rev-parse", "--verify", "-q", sha + "^", env=env).stdout.decode().strip() or ZERO
                for f in autorun(mirror, env, parent, sha):
                    key = (f["path"], f["entry"], tuple(f["details"]))
                    if key not in seen and f["hold"]:
                        seen.add(key)
                        net.append({**f, "details": f["details"] + [f"only in intermediate commit {sha[:12]}"]})
            out.append({**c, "base": base, "commits": len(commits), "findings": net})
        return out
    finally:
        shutil.rmtree(q)


# --- the HTTP side ---------------------------------------------------------------------------------

class Boundary(http.server.BaseHTTPRequestHandler):
    upstream = mirror = record = answer = None

    def body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            data = b""
            while True:
                n = int(self.rfile.readline().strip().split(b";")[0], 16)
                if n == 0:
                    self.rfile.readline(); break
                data += self.rfile.read(n); self.rfile.readline()
        else:
            data = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if self.headers.get("Content-Encoding", "") == "gzip":
            data = gzip.decompress(data)
        return data

    def cgi(self, body):
        root, name = os.path.split(self.upstream.rstrip("/"))
        path, _, query = self.path.partition("?")
        env = dict(os.environ, GIT_PROJECT_ROOT=root, GIT_HTTP_EXPORT_ALL="1", REQUEST_METHOD=self.command,
                   PATH_INFO=path, QUERY_STRING=query, CONTENT_TYPE=self.headers.get("Content-Type", ""),
                   CONTENT_LENGTH=str(len(body)), REMOTE_USER="silo-demo", REMOTE_ADDR="127.0.0.1",
                   GIT_PROTOCOL=self.headers.get("Git-Protocol", ""))
        p = subprocess.run(["git", "http-backend"], input=body, capture_output=True, env=env)
        head, _, rest = p.stdout.partition(b"\r\n\r\n")
        status, headers = 200, []
        for h in head.decode().split("\r\n"):
            k, _, v = h.partition(": ")
            if k.lower() == "status":
                status = int(v.split()[0])
            elif k:
                headers.append((k, v))
        self.send_response(status)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(rest)))
        self.end_headers()
        self.wfile.write(rest)

    def do_GET(self):
        self.cgi(b"")

    def do_POST(self):
        body = self.body()
        if not self.path.endswith("/git-receive-pack"):
            return self.cgi(body)
        entry = {"path": self.path, "body_bytes": len(body), "head": body[:200].decode("latin-1")}
        cmds, caps = [], []
        try:
            cmds, caps, opts, shallow, pack = parse_receive_pack(body)
            run("git", "-C", self.mirror, "fetch", "-q", "--prune", self.upstream, "+refs/*:refs/*")
            result = examine(self.mirror, cmds, pack)
        except (ValueError, RuntimeError) as e:
            # The change could not be read: a rule cannot establish what the push carries, so it holds.
            entry.update(error=str(e), decision="held")
            self.write_record(entry)
            return self.reply(report_status(cmds, caps, "held by Silo: the push could not be read", [str(e)]))
        holds = [f for r in result for f in r["findings"] if f["hold"]]
        entry.update(updates=[{k: r[k] for k in ("ref", "old", "new") if k in r} for r in result], caps=caps,
                     pack_bytes=len(pack), findings=result, decision="held" if holds and self.answer == "held" else "allowed")
        self.write_record(entry)
        if holds and self.answer == "held":
            msgs = ["Silo held this push: it changes files that tools on your Mac run on their own."]
            msgs += [f"  {f['path']}: {f['tool']}: {'; '.join(f['details'][:2])}" for f in holds]
            return self.reply(report_status(cmds, caps, "held by Silo: changes files that run on their own", msgs))
        self.cgi(body)  # passed on byte for byte

    def reply(self, data):
        self.send_response(200)
        self.send_header("Content-Type", "application/x-git-receive-pack-result")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def write_record(self, entry):
        with open(self.record, "a") as f:
            f.write(json.dumps(entry) + "\n")

    def log_message(self, *a):
        pass


def main():
    upstream, work, port = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2]), int(sys.argv[3])
    answer = sys.argv[sys.argv.index("--answer") + 1] if "--answer" in sys.argv else "held"
    os.makedirs(work, exist_ok=True)
    mirror = f"{work}/mirror.git"
    if not os.path.isdir(mirror):
        run("git", "clone", "-q", "--mirror", upstream, mirror)
    run("git", "-C", upstream, "config", "http.receivepack", "true")
    Boundary.upstream, Boundary.mirror, Boundary.record, Boundary.answer = upstream, mirror, f"{work}/record.jsonl", answer
    http.server.ThreadingHTTPServer(("127.0.0.1", port), Boundary).serve_forever()


if __name__ == "__main__":
    main()
