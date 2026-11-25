"""Hostile tests of the channel. Each serves static files from a local HTTP server, as a static
host would, and runs the app's side against them.

run: python3 -m unittest discover -s test -v   (from research/2026-11-25-price-channel)"""
import copy, datetime, http.server, json, os, shutil, sys, tempfile, threading, unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE / "channel"))
import sig, publish, client, cuts

TABLE = HERE.parent / "2026-11-25-model-prices" / "table" / "prices-2026112501.json"
NOW = datetime.datetime(2026, 11, 25, 14, 0, tzinfo=datetime.UTC)


class Recorder(http.server.SimpleHTTPRequestHandler):
    seen = []
    def do_GET(self):
        Recorder.seen.append(self.requestline + "\r\n" + str(self.headers))
        return super().do_GET()
    def log_message(self, *a): pass


def serve(folder):
    h = lambda *a, **k: Recorder(*a, directory=str(folder), **k)
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), h)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{srv.server_address[1]}"


class World:
    """Keys, a site, and an app that shipped root-1 and #396's table."""
    def __init__(self, tmp):
        self.tmp = Path(tmp)
        self.keys, self.site = self.tmp / "keys", self.tmp / "site"
        self.keys.mkdir(); self.site.mkdir()
        self.root_a = sig.keygen(self.keys / "root-a.key")
        self.online1 = sig.keygen(self.keys / "online-1.key")
        self.root1 = publish.make_root(self.keys, 1, ["root-a"], [("online-1", self.online1, "2027-03-01T00:00:00+00:00")])
        publish.write_signed(self.site, "root-1.json", self.root1, [("root-a", self.keys / "root-a.key")])
        self.shipped = TABLE.read_bytes()
        self.srv, self.url = serve(self.site)
        self.index_v = 0

    def app(self, name="mac-a", **kw):
        return client.Channel(self.tmp / name, (self.root1, (self.site / "root-1.json.sig").read_text()), self.shipped, "0.3.0", **kw)

    def release(self, table, *, online="online-1", root=False, at=NOW, days=2, app="0.3.0", index_v=None):
        data = json.dumps(table, indent=1).encode()
        signers = [(online, self.keys / f"{online}.key")] + ([("root-a", self.keys / "root-a.key")] if root else [])
        publish.write_signed(self.site, f"prices-{table['version']}.json", data, signers)
        self.index_v = index_v or self.index_v + 1
        publish.write_signed(self.site, "index.json", publish.make_index(self.index_v, data, app, at, days), [(online, self.keys / f"{online}.key")])

    def fetcher(self):
        return client.Fetcher([self.url])


def next_table(v, change=None):
    t = json.loads(TABLE.read_text())
    t["version"] = v
    if change: change(t)
    return t


def row(t, model, tier=None, frm=None):
    for r in t["rows"]:
        if r["model"] == model and r.get("tier") == tier and r.get("from") == frm: return r
    raise KeyError(model)


class Channel(unittest.TestCase):
    def setUp(self):
        self.tmpd = tempfile.mkdtemp()
        self.w = World(self.tmpd)

    def tearDown(self):
        self.w.srv.shutdown(); shutil.rmtree(self.tmpd)

    def test_takes_a_release_with_a_new_model(self):
        def add(t): t["rows"].append({"provider": "openai", "model": "gpt-6-new", "input": 3, "output": 12})
        self.w.release(next_table(2026112601, add))
        a = self.w.app()
        ev = a.update(self.w.fetcher(), NOW)
        self.assertIn("prices_taken", ev)
        self.assertEqual(a.table()["version"], 2026112601)

    def test_hand_edited_table_refused(self):
        self.w.release(next_table(2026112601))
        p = self.w.site / "prices-2026112601.json"
        p.write_bytes(p.read_bytes().replace(b'"input": 8.0', b'"input": 0.8', 1))
        a = self.w.app()
        self.assertIn("prices_wrong_hash", a.update(self.w.fetcher(), NOW))
        self.assertEqual(a.table()["version"], 2026112501)

    def test_index_by_unknown_key_refused(self):
        sig.keygen(self.w.keys / "stranger.key")
        self.w.release(next_table(2026112601), online="stranger")
        self.assertIn("index_bad_signature", self.w.app().update(self.w.fetcher(), NOW))

    def test_older_index_put_back_refused_across_restart(self):
        self.w.release(next_table(2026112601))
        old = {n: (self.w.site / n).read_bytes() for n in ("index.json", "index.json.sig")}
        self.w.release(next_table(2026112602))
        a = self.w.app()
        a.update(self.w.fetcher(), NOW)
        for n, b in old.items(): (self.w.site / n).write_bytes(b)
        a2 = self.w.app()  # a restart: same state folder
        self.assertIn("index_rolled_back", a2.update(self.w.fetcher(), NOW))
        self.assertEqual(a2.table()["version"], 2026112602)

    def test_frozen_index_reported_not_taken(self):
        self.w.release(next_table(2026112601))
        a = self.w.app()
        a.update(self.w.fetcher(), NOW)
        later = NOW + datetime.timedelta(days=5)  # a mirror or a network that keeps serving the same index
        self.assertIn("index_expired", a.update(self.w.fetcher(), later))
        st = a.status(later)
        self.assertTrue(st["index_expired"]); self.assertEqual(st["confirmed"], NOW.isoformat())
        self.assertEqual(st["table"], 2026112601)

    def test_mix_and_match_refused(self):
        self.w.release(next_table(2026112601))
        good = (self.w.site / "prices-2026112601.json").read_bytes()
        self.w.release(next_table(2026112602))
        (self.w.site / "prices-2026112602.json").write_bytes(good)
        self.assertIn("prices_wrong_hash", self.w.app().update(self.w.fetcher(), NOW))

    def test_index_naming_an_older_table_refused(self):
        self.w.release(next_table(2026112602))
        a = self.w.app(); a.update(self.w.fetcher(), NOW)
        self.w.release(next_table(2026112601))
        self.assertIn("prices_rolled_back", a.update(self.w.fetcher(), NOW))

    def test_online_key_cannot_cut_beyond_half(self):
        def cut(t): row(t, "claude-opus-5-5")["output"] /= 5
        self.w.release(next_table(2026112601, cut))
        a = self.w.app()
        self.assertIn("prices_cut_needs_root", a.update(self.w.fetcher(), NOW))
        self.assertEqual(a.table()["version"], 2026112501)

    def test_without_the_floor_a_stolen_online_key_cuts_freely(self):
        # #396's boundary checks the signature and the version only: floor 0 is that check.
        def cut(t): row(t, "claude-opus-5-5")["output"] /= 100
        self.w.release(next_table(2026112601, cut))
        a = self.w.app(floor=0)
        self.assertIn("prices_taken", a.update(self.w.fetcher(), NOW))

    def test_root_key_allows_a_real_cut(self):
        def cut(t): row(t, "claude-opus-5-5")["output"] /= 5
        self.w.release(next_table(2026112601, cut), root=True)
        a = self.w.app()
        self.assertIn("prices_taken", a.update(self.w.fetcher(), NOW))

    def test_cut_by_half_needs_only_online_key(self):
        # a promotion at half price, as GPT-5.6 Sol's and Gemini 3.6 Flash's were
        def promo(t): row(t, "claude-opus-5-5")["input"] /= 2
        self.w.release(next_table(2026112601, promo))
        self.assertIn("prices_taken", self.w.app().update(self.w.fetcher(), NOW))

    def test_withdrawn_dated_doubling_needs_only_online_key(self):
        # Anthropic withdrew Sonnet 5's announced increase; here Gemini 4 Carbon's 1 December one
        def withdraw(t): t["rows"] = [r for r in t["rows"] if not (r["model"] == "gemini-4-carbon" and r.get("from") == "2026-12-01")]
        self.w.release(next_table(2026112601, withdraw))
        self.assertIn("prices_taken", self.w.app().update(self.w.fetcher(), NOW))

    def test_removing_a_row_falls_back_to_the_bound(self):
        def drop(t): t["rows"] = [r for r in t["rows"] if r["model"] != "gpt-5.4"]
        self.w.release(next_table(2026112601, drop))
        self.assertIn("prices_taken", self.w.app().update(self.w.fetcher(), NOW))

    def test_discount_multiplier_refused(self):
        def disc(t): t["multipliers"].append({"provider": "anthropic", "factor": 0.1})
        self.w.release(next_table(2026112601, disc))
        self.assertIn("prices_cut_needs_root", self.w.app().update(self.w.fetcher(), NOW))

    def test_us_only_multiplier_cut_refused(self):
        def geo(t): t["multipliers"][0]["factor"] = 0.3
        self.w.release(next_table(2026112601, geo))
        self.assertIn("prices_cut_needs_root", self.w.app().update(self.w.fetcher(), NOW))

    def test_dated_cut_in_the_future_refused(self):
        def later(t): t["rows"].append(dict(row(t, "claude-opus-5-5"), **{"from": "2026-12-20", "input": 0.1, "output": 0.5}))
        self.w.release(next_table(2026112601, later))
        self.assertIn("prices_cut_needs_root", self.w.app().update(self.w.fetcher(), NOW))

    def test_prefix_row_does_not_reach_an_exact_one(self):
        def pre(t): t["rows"].append({"provider": "anthropic", "model": "claude-*", "input": 0, "output": 0})
        self.w.release(next_table(2026112601, pre))
        a = self.w.app()
        self.assertIn("prices_taken", a.update(self.w.fetcher(), NOW))
        # what it does reach, the residual the README names: an id no row priced before
        t = a.table()
        self.assertEqual(cuts.pick(t["rows"], "anthropic", "claude-opus-5-5", "standard", "2026-11-25")["output"],
                         row(next_table(0), "claude-opus-5-5")["output"])
        self.assertEqual(cuts.pick(t["rows"], "anthropic", "claude-opus-6", "standard", "2026-11-25")["output"], 0)

    def test_root_rotation_and_revoked_online_key(self):
        a = self.w.app(); self.w.release(next_table(2026112601)); a.update(self.w.fetcher(), NOW)
        rb = sig.keygen(self.w.keys / "root-b.key"); o2 = sig.keygen(self.w.keys / "online-2.key")
        r2 = publish.make_root(self.w.keys, 2, ["root-b"], [("online-2", o2, "2027-03-01T00:00:00+00:00")])
        # signed by the new root only: refused
        publish.write_signed(self.w.site, "root-2.json", r2, [("root-b", self.w.keys / "root-b.key")])
        self.assertIn("root_refused", a.update(self.w.fetcher(), NOW))
        # signed by the old and the new: taken, and online-1 is no longer good
        publish.write_signed(self.w.site, "root-2.json", r2, [("root-a", self.w.keys / "root-a.key"), ("root-b", self.w.keys / "root-b.key")])
        self.w.release(next_table(2026112602))  # still by online-1
        ev = a.update(self.w.fetcher(), NOW)
        self.assertIn("root_taken", ev); self.assertIn("index_bad_signature", ev)
        self.w.release(next_table(2026112602), online="online-2")
        self.assertIn("prices_taken", a.update(self.w.fetcher(), NOW))

    def test_expired_online_key_refused(self):
        self.w.release(next_table(2026112601), at=datetime.datetime(2027, 3, 2, tzinfo=datetime.UTC))
        self.assertIn("index_bad_signature", self.w.app().update(self.w.fetcher(), datetime.datetime(2027, 3, 2, 1, tzinfo=datetime.UTC)))

    def test_newer_app_named(self):
        self.w.release(next_table(2026112601), app="0.4.0")
        a = self.w.app()
        self.assertIn("newer_app", a.update(self.w.fetcher(), NOW))
        self.assertEqual(a.status(NOW)["newer_app"], "0.4.0")

    def test_fetch_off_sends_nothing(self):
        self.w.release(next_table(2026112601))
        Recorder.seen.clear()
        f = self.w.fetcher()
        a = self.w.app(fetch_enabled=False)
        self.assertEqual(a.update(f, NOW), ["fetch_off"])
        self.assertEqual(f.requests, 0); self.assertEqual(Recorder.seen, [])
        self.assertEqual(a.table()["version"], 2026112501)  # the table the app shipped with

    def test_files_brought_by_hand_get_the_same_checks(self):
        self.w.release(next_table(2026112601))
        usb = Path(self.tmpd) / "usb"; shutil.copytree(self.w.site, usb)
        a = self.w.app(fetch_enabled=False)
        self.assertIn("prices_taken", a.update(client.LocalFetcher(usb), NOW))
        p = usb / "prices-2026112601.json"; p.write_bytes(p.read_bytes() + b" ")
        self.w.release(next_table(2026112602))
        shutil.copy(self.w.site / "index.json", usb); shutil.copy(self.w.site / "index.json.sig", usb)
        shutil.copy(self.w.site / "prices-2026112602.json", usb)
        q = usb / "prices-2026112602.json"; q.write_bytes(q.read_bytes().replace(b"8.0", b"0.8", 1))
        self.assertIn("prices_wrong_hash", a.update(client.LocalFetcher(usb), NOW))

    def test_a_teams_own_table(self):
        team = sig.keygen(Path(self.tmpd) / "team.key")
        a = self.w.app(own_keys={"team": team})
        mine = json.dumps({"rows": [{"provider": "anthropic", "model": "claude-opus-5-5", "input": 3, "output": 15}]}).encode()
        self.assertTrue(a.import_own(mine, sig.sign(mine, "team", Path(self.tmpd) / "team.key"), NOW))
        self.assertFalse(a.import_own(mine.replace(b"3,", b"0,"), sig.sign(mine, "team", Path(self.tmpd) / "team.key"), NOW))
        self.assertEqual(a.local()["rows"][0]["input"], 3)

    def test_mirror_when_the_first_address_fails(self):
        self.w.release(next_table(2026112601))
        f = client.Fetcher(["http://127.0.0.1:9", self.w.url], timeout=2)
        self.assertIn("prices_taken", self.w.app().update(f, NOW))

    def test_two_macs_send_the_same_bytes(self):
        # Two installs at different versions, with different tables and histories, ask for the
        # index with byte-identical requests: nothing in it tells one Mac from another.
        self.w.release(next_table(2026112601))
        a = self.w.app("mac-a"); a.update(self.w.fetcher(), NOW)
        self.w.release(next_table(2026112602))
        Recorder.seen.clear()
        a.update(self.w.fetcher(), NOW)
        b = client.Channel(Path(self.tmpd) / "mac-b", (self.w.root1, (self.w.site / "root-1.json.sig").read_text()), self.w.shipped, "0.2.9")
        b.update(self.w.fetcher(), NOW)
        reqs = [r for r in Recorder.seen if r.startswith("GET /index.json ")]
        self.assertEqual(len(reqs), 2)
        self.assertEqual(reqs[0], reqs[1])
        Path(HERE / "results").mkdir(exist_ok=True)
        (HERE / "results" / "request-index.txt").write_text(reqs[0].replace(self.w.url.split("//")[1], "<host>"))


if __name__ == "__main__":
    unittest.main()
