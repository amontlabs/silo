"""The app's side of the channel: one signed index, fetched from a public static address, that
names the latest app version and the latest price table.

Trust: the app ships the first root file. The root key (offline, the owner's) signs each root
file, which names the online keys and until when each is good. An online key (the daily pipeline's)
signs the index and the price tables. A table that lowers a price below `floor` times what the app
holds needs the root key's signature too.

What a fetch sends: the path, a fixed User-Agent, and nothing else. No version, no cookie, no
ETag or If-Modified-Since, no query. The app compares versions itself.

State, in `state_dir`: root.json(.sig), index.json(.sig), prices.json(.sig), local.json (yours),
seen.json (the highest versions taken), log.jsonl (each event, as the record would keep it)."""
import hashlib, json, os, socket, ssl, urllib.request, urllib.error, datetime, random
from pathlib import Path
import sig, cuts

UA = "Silo"
HEADERS = {"User-Agent": UA, "Accept": "*/*", "Accept-Encoding": "identity", "Connection": "close"}


class Fetcher:
    """GET with only HEADERS. One connection per request: no keep-alive, and no TLS session
    reused across requests, so two fetches cannot be linked by a session ticket."""
    def __init__(self, bases, timeout=10, cafile=None):
        self.bases = bases
        self.timeout = timeout
        self.cafile = cafile
        self.requests = 0

    def get(self, path):
        last = None
        for base in self.bases:
            url = base.rstrip("/") + "/" + path
            req = urllib.request.Request(url, headers=HEADERS)
            # A bare opener: no cookie jar, no proxy auth, no redirects to another scheme.
            ctx = ssl.create_default_context(cafile=self.cafile) if url.startswith("https") else None
            opener = urllib.request.build_opener(urllib.request.HTTPSHandler(context=ctx)) if ctx else urllib.request.build_opener()
            opener.addheaders = []
            self.requests += 1
            try:
                with opener.open(req, timeout=self.timeout) as r:
                    return r.read()
            except urllib.error.HTTPError as e:
                if e.code == 404:
                    last = FileNotFoundError(path)
                    continue
                last = e
            except (urllib.error.URLError, socket.timeout, ConnectionError) as e:
                last = e
        raise last


class LocalFetcher:
    """Files copied by hand (a USB key, a team's share): the same checks as a fetch."""
    def __init__(self, folder):
        self.folder = Path(folder)
        self.requests = 0

    def get(self, path):
        p = self.folder / path
        if not p.exists(): raise FileNotFoundError(path)
        return p.read_bytes()


def _day(t): return t.strftime("%Y-%m-%d")


class Channel:
    def __init__(self, state_dir, shipped_root, shipped_table, app_version, fetch_enabled=True, floor=0.5, own_keys=None):
        """`shipped_root` is (bytes, sig text) of the root file inside the app; `shipped_table` the
        price table inside the app, which the signed app vouches for. `own_keys` are keys you trust
        for a table of your own (a team's), whose rows count as yours."""
        self.d = Path(state_dir)
        self.d.mkdir(parents=True, exist_ok=True)
        self.app_version = app_version
        self.fetch_enabled = fetch_enabled
        self.floor = floor
        self.own_keys = own_keys or {}
        self.events = []
        if not (self.d / "root.json").exists():
            (self.d / "root.json").write_bytes(shipped_root[0])
            (self.d / "root.json.sig").write_text(shipped_root[1])
        if not (self.d / "prices.json").exists():
            (self.d / "prices.json").write_bytes(shipped_table)
            self._seen_set("prices", json.loads(shipped_table)["version"])

    # state
    def _seen(self):
        p = self.d / "seen.json"
        return json.loads(p.read_text()) if p.exists() else {"root": 0, "index": 0, "prices": 0}

    def _seen_set(self, k, v):
        s = self._seen(); s[k] = v
        tmp = self.d / "seen.json.tmp"; tmp.write_text(json.dumps(s)); os.replace(tmp, self.d / "seen.json")

    def _log(self, ev, now, **kw):
        with open(self.d / "log.jsonl", "a") as f:
            f.write(json.dumps({"at": now.isoformat(), "event": ev, **kw}) + "\n")
        self.events.append(ev)

    def root(self):
        return json.loads((self.d / "root.json").read_text())

    def _keys(self, role, now):
        r = self.root()
        return {k["id"]: k["public"] for k in r["keys"] if k["role"] == role and (role == "root" or k.get("expires", "9999") > now.isoformat())}

    def table(self):
        return json.loads((self.d / "prices.json").read_text())

    def local(self):
        p = self.d / "local.json"
        return json.loads(p.read_text()) if p.exists() else None

    def status(self, now):
        s = self._seen()
        conf = (self.d / "confirmed").read_text() if (self.d / "confirmed").exists() else None
        idx = json.loads((self.d / "index.json").read_text()) if (self.d / "index.json").exists() else None
        t = self.table()
        return {
            "table": t["version"], "check_by": t.get("check_by"),
            "past_check_by": bool(t.get("check_by")) and t["check_by"] < _day(now),
            "confirmed": conf,
            "index_expired": idx is None or idx["expires"] <= now.isoformat(),
            "newer_app": idx["app"]["version"] if idx and _vt(idx["app"]["version"]) > _vt(self.app_version) else None,
            "seen": s,
        }

    # the update
    def update(self, fetcher, now):
        self.events = []
        if not self.fetch_enabled and not isinstance(fetcher, LocalFetcher):
            self._log("fetch_off", now)
            return self.events
        try:
            self._update_root(fetcher, now)
            self._update_index(fetcher, now)
        except (FileNotFoundError, urllib.error.URLError, ConnectionError, OSError) as e:
            self._log("unreachable", now, error=type(e).__name__)
        return self.events

    def _update_root(self, fetcher, now):
        while True:
            v = self.root()["version"] + 1
            try:
                data, st = fetcher.get(f"root-{v}.json"), fetcher.get(f"root-{v}.json.sig").decode()
            except FileNotFoundError:
                return
            new = json.loads(data)
            old_ok = sig.signers(data, st, self._keys("root", now))
            new_ok = sig.signers(data, st, {k["id"]: k["public"] for k in new["keys"] if k["role"] == "root"})
            if new.get("version") != v or len(old_ok) < self.root()["threshold"] or len(new_ok) < new["threshold"]:
                self._log("root_refused", now, version=v)
                return
            (self.d / "root.json").write_bytes(data); (self.d / "root.json.sig").write_text(st)
            self._seen_set("root", v)
            self._log("root_taken", now, version=v)

    def _update_index(self, fetcher, now):
        data, st = fetcher.get("index.json"), fetcher.get("index.json.sig").decode()
        if not sig.signers(data, st, self._keys("online", now)):
            self._log("index_bad_signature", now); return
        idx = json.loads(data)
        seen = self._seen()
        if idx["version"] < seen["index"]:
            self._log("index_rolled_back", now, version=idx["version"], seen=seen["index"]); return
        if idx["expires"] <= now.isoformat():
            self._log("index_expired", now, version=idx["version"], expires=idx["expires"]); return
        (self.d / "index.json").write_bytes(data); (self.d / "index.json.sig").write_text(st)
        self._seen_set("index", idx["version"])
        (self.d / "confirmed").write_text(now.isoformat())
        self._log("index_taken" if idx["version"] > seen["index"] else "index_unchanged", now, version=idx["version"])
        if _vt(idx["app"]["version"]) > _vt(self.app_version):
            self._log("newer_app", now, version=idx["app"]["version"])
        self._update_prices(fetcher, idx["prices"], now)

    def _update_prices(self, fetcher, ref, now):
        seen = self._seen()["prices"]
        if ref["version"] == seen: return
        if ref["version"] < seen:
            self._log("prices_rolled_back", now, version=ref["version"], seen=seen); return
        data = fetcher.get(ref["path"])
        if hashlib.sha256(data).hexdigest() != ref["sha256"]:
            self._log("prices_wrong_hash", now, version=ref["version"]); return
        new = json.loads(data)
        if new.get("version") != ref["version"]:
            self._log("prices_wrong_version", now, version=new.get("version")); return
        found = cuts.cuts(self.table(), new, _day(now), self.floor)
        rooted = False
        if found:
            try: st = fetcher.get(ref["path"] + ".sig").decode()
            except FileNotFoundError: st = ""
            rooted = len(sig.signers(data, st, self._keys("root", now))) >= self.root()["threshold"]
            if not rooted:
                self._log("prices_cut_needs_root", now, version=ref["version"], cuts=found[:5], count=len(found)); return
        tmp = self.d / "prices.json.tmp"; tmp.write_bytes(data); os.replace(tmp, self.d / "prices.json")
        self._seen_set("prices", ref["version"])
        self._log("prices_taken", now, version=ref["version"], cuts=len(found), root_signed=rooted)

    # your own prices
    def import_own(self, data, sigtext, now):
        """A table of your own (a team's, signed with a key you added). Its rows win, as #396's
        `local_prices` do. A file you write by hand needs no signature: use write_local."""
        if not sig.signers(data, sigtext, self.own_keys):
            self._log("own_bad_signature", now); return False
        return self.write_local(data, now)

    def write_local(self, data, now):
        json.loads(data)
        (self.d / "local.json").write_bytes(data)
        self._log("local_taken", now)
        return True


def _vt(v): return tuple(int(x) for x in v.split("."))


def next_fetch(now, rng=random):
    """Once a day at a random minute, not at launch, so the fetch's time says little about when
    the Mac is used."""
    base = (now + datetime.timedelta(days=1)).replace(hour=0, minute=0, second=0, microsecond=0)
    return base + datetime.timedelta(minutes=rng.randrange(24 * 60))
