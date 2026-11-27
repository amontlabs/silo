"""The record: append-only JSON lines, one file per computer and per kind, each line chained to the
one before it by a hash, so that an edit shows (`record-edits-show`, proposed in PR 362).

    record/<computer>/facts.jsonl     what Silo saw itself: control, file, connection, model call
    record/<computer>/reports.jsonl   what the probe inside told it: process, file report, owner
    record/<computer>/opinions.jsonl  what a classifier or a precaution concluded and acted on
    record/blobs/<sha256>             bulky text, stored once (`bulky-content-once`)

Every line carries: id, kind, line, at (with its offset), computer, source, prev.
"""
import fcntl, hashlib, json, os, threading, datetime

GENESIS = "0" * 64


def now():
    return datetime.datetime.now().astimezone().isoformat(timespec="milliseconds")


def at_of(ts):
    return datetime.datetime.fromtimestamp(ts).astimezone().isoformat(timespec="milliseconds")


def ts_of(at):
    return datetime.datetime.fromisoformat(at).timestamp()


def line_hash(raw):
    return hashlib.sha256(raw.encode()).hexdigest()


class Writer:
    PREFIX = {"facts": "f", "reports": "r", "opinions": "o"}

    def __init__(self, root, computer):
        self.root, self.computer = root, computer
        self.dir = os.path.join(root, computer)
        os.makedirs(self.dir, exist_ok=True)
        os.makedirs(os.path.join(root, "blobs"), exist_ok=True)
        self.lock = threading.Lock()
        self.state = {k: self.scan(k) for k in self.PREFIX}

    def scan(self, k):
        """Count the lines of one file and hash its last one: [count, last hash, size read]."""
        p = os.path.join(self.dir, k + ".jsonl")
        n, prev, size = 0, GENESIS, 0
        if os.path.exists(p):
            with open(p) as f:
                for raw in f:
                    raw = raw.rstrip("\n")
                    if raw:
                        n += 1; prev = line_hash(raw)
                size = f.tell()
        return [n, prev, size]

    def blob(self, text):
        if text is None:
            return None
        if not isinstance(text, str):
            text = json.dumps(text, ensure_ascii=False)
        h = hashlib.sha256(text.encode()).hexdigest()
        p = os.path.join(self.root, "blobs", h)
        if not os.path.exists(p):
            with open(p + ".tmp", "w") as f:
                f.write(text)
            os.replace(p + ".tmp", p)
        return "sha256:" + h

    def write(self, file, kind, line, source, at=None, **fields):
        # More than one process may write one computer's record (the engine, and `silo putback`).
        # A lock on the file, and a fresh count when someone else wrote since, keep the chain whole.
        path = os.path.join(self.dir, file + ".jsonl")
        with self.lock, open(path, "a") as f:
            fcntl.flock(f, fcntl.LOCK_EX)
            try:
                f.seek(0, os.SEEK_END)
                if f.tell() != self.state[file][2]:
                    self.state[file] = self.scan(file)
                n, prev, _ = self.state[file]
                n += 1
                obj = {"id": f"{self.computer}:{self.PREFIX[file]}{n}", "kind": kind, "line": line,
                       "at": at or now(), "computer": self.computer, "source": source}
                obj.update({k: v for k, v in fields.items() if v is not None})
                obj["prev"] = prev
                raw = json.dumps(obj, ensure_ascii=False, sort_keys=False)
                f.write(raw + "\n"); f.flush()
                self.state[file] = [n, line_hash(raw), f.tell()]
            finally:
                fcntl.flock(f, fcntl.LOCK_UN)
            return obj

    def fact(self, line, source, **kw):
        return self.write("facts", "fact", line, source, **kw)

    def report(self, line, source, **kw):
        return self.write("reports", "report", line, source, unverified=True, **kw)

    def opinion(self, by, about, says, rests_on, **kw):
        return self.write("opinions", "opinion", "opinion", by, by=by, about=about, says=says, rests_on=rests_on, **kw)


def read(root, computer=None, file=None):
    """All lines of the record, oldest first by time."""
    out = []
    comps = [computer] if computer else sorted(d for d in os.listdir(root) if d != "blobs" and os.path.isdir(os.path.join(root, d)))
    for c in comps:
        for k in (["facts", "reports", "opinions"] if not file else [file]):
            p = os.path.join(root, c, k + ".jsonl")
            if os.path.exists(p):
                out += [json.loads(x) for x in open(p) if x.strip()]
    out.sort(key=lambda o: o["at"])
    return out


def text(root, ref):
    if not ref:
        return None
    try:
        return open(os.path.join(root, "blobs", ref.split(":", 1)[1])).read()
    except FileNotFoundError:
        return None


def verify(root):
    """Walk every chain. Returns a list of (file, line number, problem)."""
    bad = []
    for c in sorted(os.listdir(root)):
        if c == "blobs" or not os.path.isdir(os.path.join(root, c)):
            continue
        for k in ("facts", "reports", "opinions"):
            p = os.path.join(root, c, k + ".jsonl")
            if not os.path.exists(p):
                continue
            prev = GENESIS
            for i, raw in enumerate(open(p), 1):
                raw = raw.rstrip("\n")
                o = json.loads(raw)
                if o.get("prev") != prev:
                    bad.append((f"{c}/{k}.jsonl", i, "the line before it was changed or removed"))
                prev = line_hash(raw)
    return bad
