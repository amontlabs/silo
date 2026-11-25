"""Run: python3 -m unittest test_restore_list -v   (from this folder)"""
import unittest
from restore_list import restore_list, since, render

CP = {"t": "2026-11-24T14:00:00Z", "type": "checkpoint", "id": "cp"}


def http(method, path, status=200, host="api.example.com", **kw):
    return dict({"t": "2026-11-24T14:01:00Z", "type": "http", "host": host, "port": 443, "opened": True,
                 "method": method, "path": path, "status": status, "bytes_out": 100, "bytes_in": 10}, **kw)


def lines(res):
    return [x["line"] for k in ("changed_elsewhere", "not_read", "may_have_changed") for x in res[k]]


class RestoreList(unittest.TestCase):
    def test_only_after_the_checkpoint(self):
        rec = [http("POST", "/before"), CP, http("POST", "/after")]
        self.assertEqual(lines(restore_list(since(rec, "cp"))), ["POST api.example.com/after, the host answered 200"])

    def test_reads_are_counted_not_listed(self):
        res = restore_list([http("GET", "/a"), http("HEAD", "/b")])
        self.assertEqual(lines(res), []); self.assertEqual(res["reads_count"], 2)

    def test_git_fetch_is_a_listed_read(self):
        res = restore_list([http("POST", "/o/r.git/git-upload-pack", host="github.com")])
        self.assertEqual(lines(res), []); self.assertEqual(res["reads_count"], 1)

    def test_push_lists_each_ref(self):
        res = restore_list([http("POST", "/o/r.git/git-receive-pack", host="github.com", secret="gh",
                                 push_refs=[{"ref": "refs/heads/main", "old": "a", "new": "b", "force": True},
                                            {"ref": "refs/tags/v1", "old": "0", "new": "c"}])])
        self.assertEqual(lines(res), [
            "pushed refs/heads/main a -> b, forced to github.com/o/r.git, with the secret gh, the host answered 200",
            "pushed refs/tags/v1 0 -> c to github.com/o/r.git, with the secret gh, the host answered 200"])

    def test_denied_left_nothing(self):
        res = restore_list([http("POST", "/x", status=None, denied="host not allowed", bytes_out=0)])
        self.assertEqual(lines(res), []); self.assertEqual(res["sent_total"], 0)

    def test_model_traffic_is_not_listed(self):
        self.assertEqual(lines(restore_list([http("POST", "/v1/messages", model_traffic=True)])), [])

    def test_refused_and_unanswered_writes_stay_listed(self):
        res = restore_list([http("DELETE", "/r/1", status=403), http("POST", "/deploy", status=None, ended="reset")])
        self.assertEqual(lines(res), ["DELETE api.example.com/r/1, the host answered 403, a refusal",
                                      "POST api.example.com/deploy, no answer seen (reset): it may or may not have happened"])

    def test_unread_connections(self):
        res = restore_list([{"t": "2026-11-24T14:02:00Z", "type": "tcp", "host": "10.0.0.5", "port": 22, "opened": False,
                             "protocol": "ssh", "bytes_out": 2000, "duration_s": 40},
                            {"t": "2026-11-24T14:03:00Z", "type": "tcp", "host": "db", "port": 5432, "opened": False, "bytes_out": 10}])
        self.assertEqual(lines(res), [
            "10.0.0.5:22: an SSH session of 40 s, 2 kB sent. Any command could have run there",
            "db:5432: an encrypted connection, 10 B sent. Silo did not read it",
            "db:5432 is the usual port of a database"])
        self.assertEqual(res["may_have_changed"][0]["kind"], "opinion")

    def test_database_statements(self):
        def db(s):
            return {"t": "2026-11-24T14:04:00Z", "type": "db", "host": "db", "port": 5432, "opened": True,
                    "statement": s, "status": "ok", "bytes_out": 1}
        res = restore_list([db("SELECT * FROM t"), db("select count(*) from t"), db("DROP TABLE t"),
                            db("WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x"), db("SELECT purge()")])
        self.assertEqual(res["reads_count"], 2)
        self.assertEqual([x["line"] for x in res["changed_elsewhere"]],
                         ["db: `DROP TABLE t`, the database answered ok",
                          "db: `WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x`, the database answered ok"])
        self.assertEqual(len(res["may_have_changed"]), 1)

    def test_get_with_side_effect_words_is_an_opinion(self):
        res = restore_list([http("GET", "/hooks/deploy?env=prod")])
        self.assertEqual(res["changed_elsewhere"], [])
        self.assertEqual(res["may_have_changed"][0]["kind"], "opinion")
        self.assertEqual(res["reads_count"], 1)

    def test_copy_to_the_mac(self):
        res = restore_list([{"t": "2026-11-24T14:05:00Z", "type": "mac", "channel": "files", "what": "copied a.pdf to your Mac", "bytes_out": 5}])
        self.assertEqual(lines(res), ["copied a.pdf to your Mac: it stays on your Mac"])

    def test_render_always_says_what_silo_cannot_see(self):
        text = render(restore_list([]), "cp")
        self.assertIn("Silo sees what left this computer, not what the hosts did with it.", text)
        self.assertIn("A host can still change something on a read", text)


if __name__ == "__main__":
    unittest.main()
