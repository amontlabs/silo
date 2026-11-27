#!/usr/bin/env python3
"""A prototype of the record's head kept off the Mac.

The recorder sends each new head to an anchor: a place a program running as you on the Mac can
add to but cannot rewrite. `check` then reads every head the anchor ever received and checks each
one against the record: the line it names is there, and its hash is the hash of that line.

A head is {"v":1,"computer":"c1","seq":…,"hash":…,"time":…}, signed by the Mac's key. On a Mac the
key would be a P-256 key in the Secure Enclave that only Silo's signed app may use; here it is a
P-256 key in a file, which is what the enclave key stands in for.

Two anchors are prototyped:

  GitAnchor     a Git repository whose branch refuses force pushes and deletions (a GitHub
                ruleset, here receive.denyNonFastForwards and receive.denyDeletes on a bare
                repository). `check` reads the heads of every commit of the branch, not only the
                last file, since a later commit can remove lines from the file.
  LedgerAnchor  the team's host: a small HTTP service that keeps every head it receives, refuses a
                head whose signature is not the computer's key, and refuses a head older than the
                last one it holds.
"""
import base64, fcntl, hashlib, http.server, json, os, subprocess, threading, urllib.request

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.exceptions import InvalidSignature

import recfmt


def durable(fd):
    """The data on disk before the head is answered as kept. On a Mac, os.fsync is a plain
    fsync(2), which leaves it in the drive's cache; F_FULLFSYNC flushes that cache. A volume that
    refuses it falls back to fsync (research/2026-11-25-durability-on-a-mac)."""
    if hasattr(fcntl, 'F_FULLFSYNC'):
        try:
            fcntl.fcntl(fd, fcntl.F_FULLFSYNC)
            return
        except OSError:
            pass
    os.fsync(fd)


def canon(head):
    return json.dumps({k: head[k] for k in ('v', 'computer', 'seq', 'hash', 'time')}, separators=(',', ':')).encode()


def sign(key, head):
    s = dict(head)
    s['sig'] = base64.b64encode(key.sign(canon(head), ec.ECDSA(hashes.SHA256()))).decode()
    return s


def signed_by(pub, head):
    try:
        pub.verify(base64.b64decode(head['sig']), canon(head), ec.ECDSA(hashes.SHA256()))
        return True
    except (InvalidSignature, KeyError, ValueError):
        return False


def head_of(record):
    ls = recfmt.lines(record)
    last = json.loads(ls[-1])
    return {'v': 1, 'computer': last['computer'], 'seq': last['seq'], 'hash': hashlib.sha256(ls[-1]).hexdigest(),
            'time': last['time']}


def head_at(record, seq):
    for l in recfmt.lines(record):
        j = json.loads(l)
        if j['seq'] == seq:
            return {'v': 1, 'computer': j['computer'], 'seq': seq, 'hash': hashlib.sha256(l).hexdigest(), 'time': j['time']}
    raise KeyError(seq)


class GitAnchor:
    def __init__(self, bare, work):
        self.bare, self.work = bare, work
        if not os.path.exists(bare):
            run('git', 'init', '-q', '--bare', '-b', 'main', bare)
            run('git', '-C', bare, 'config', 'receive.denyNonFastForwards', 'true')
            run('git', '-C', bare, 'config', 'receive.denyDeletes', 'true')
        if not os.path.exists(work):
            run('git', 'clone', '-q', bare, work)
            run('git', '-C', work, 'config', 'user.email', 'recorder@silo.invalid')
            run('git', '-C', work, 'config', 'user.name', 'recorder')
            run('git', '-C', work, 'checkout', '-q', '-B', 'main')

    def push(self, head):
        p = os.path.join(self.work, head['computer'] + '.heads')
        with open(p, 'a') as f:
            f.write(json.dumps(head, separators=(',', ':')) + '\n')
        run('git', '-C', self.work, 'add', '-A')
        run('git', '-C', self.work, 'commit', '-q', '-m', f"{head['computer']} {head['seq']}")
        return git_push(self.work)

    def heads(self, computer):
        """Every head ever added in any commit of the branch, as the remote holds it: every line
        the history of the file ever added, read in one pass of `git log -p`."""
        log = run('git', '-C', self.bare, 'log', '-p', '--format=', '--unified=0', 'main', '--',
                  f'{computer}.heads', check=False)
        seen, out = set(), []
        for l in log.splitlines():
            if l.startswith('+{') and l[1:] not in seen:
                seen.add(l[1:])
                out.append(json.loads(l[1:]))
        return sorted(out, key=lambda x: x['seq'])


class Ledger(http.server.ThreadingHTTPServer):
    """The team's host. Keys are registered when a computer joins."""

    def __init__(self, path, keys):
        self.path, self.keys, self.lock = path, keys, threading.Lock()
        super().__init__(('127.0.0.1', 0), LedgerHandler)

    def all(self, computer):
        if not os.path.exists(self.path):
            return []
        return [h for h in map(json.loads, open(self.path)) if h['computer'] == computer]


class LedgerHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        body = json.dumps(self.server.all(self.path.rsplit('/', 1)[-1])).encode()
        self.send_response(200)
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        head = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        srv = self.server
        with srv.lock:
            pub = srv.keys.get(head.get('computer'))
            have = srv.all(head.get('computer'))
            if pub is None or not signed_by(pub, head):
                code, why = 403, 'not signed by this computer'
            elif have and head['seq'] < have[-1]['seq']:
                code, why = 409, 'older than the last head held'
            else:
                with open(srv.path, 'a') as f:
                    f.write(json.dumps(head, separators=(',', ':')) + '\n')
                    f.flush()
                    durable(f.fileno())
                code, why = 201, 'kept'
        self.send_response(code)
        self.end_headers()
        self.wfile.write(why.encode())


class LedgerAnchor:
    def __init__(self, url):
        self.url = url

    def push(self, head):
        req = urllib.request.Request(f'{self.url}/heads', data=json.dumps(head).encode(), method='POST')
        try:
            with urllib.request.urlopen(req) as r:
                return r.status, r.read().decode()
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode()

    def heads(self, computer):
        with urllib.request.urlopen(f'{self.url}/heads/{computer}') as r:
            return json.loads(r.read())


def check(record, heads, pub=None):
    """What the anchored heads say of the record. Returns (ok, message)."""
    ls = recfmt.lines(record)
    by_seq = {json.loads(l)['seq']: l for l in ls}
    last = max(by_seq) if by_seq else 0
    good = [h for h in heads if pub is None or signed_by(pub, h)]
    if pub is not None and len(good) < len(heads):
        bad = next(h for h in heads if not signed_by(pub, h))
        return False, f"a head for line {bad['seq']} is not signed by this computer's key"
    for hd in good:
        if hd['seq'] > last:
            return False, f"the record ends at line {last}, before line {hd['seq']}, anchored at {hd['time']}: lines were removed at the end"
        if hashlib.sha256(by_seq[hd['seq']]).hexdigest() != hd['hash']:
            return False, f"line {hd['seq']} is not the line anchored at {hd['time']}: the record was rewritten at or before it"
    newest = max((h['seq'] for h in good), default=0)
    return True, f"{len(good)} anchored heads hold; {last - newest} lines after the newest one are not anchored yet"


def run(*a, check=True):
    r = subprocess.run(a, capture_output=True, text=True)
    if check and r.returncode != 0:
        raise RuntimeError(r.stderr)
    return r.stdout


def git_push(work, *extra):
    r = subprocess.run(['git', '-C', work, 'push', '-q', *extra, 'origin', 'HEAD:main'], capture_output=True, text=True)
    return r.returncode, (r.stderr.strip().splitlines() or [''])[-1]


def new_key():
    return ec.generate_private_key(ec.SECP256R1())
