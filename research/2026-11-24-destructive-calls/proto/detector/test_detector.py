"""Run: python3 -m pytest -q   (from this directory)"""
import base64
import json
import os

import corpus_build
import capture_labels
import sqlclass
from detector import Detector

HERE = os.path.dirname(os.path.abspath(__file__))
D = Detector.load(os.path.join(HERE, "rules.json"))

# Requests the detector gets wrong on purpose: each is written up in details/detector.md.
KNOWN_WRONG = {
    "split: deploy a service that deletes the volume from inside Railway (never crosses the boundary)",
    "git push --force rewrites main (boundary cannot tell from a fast-forward)",
    "WHERE id IS NOT NULL (every row, but a column is named)",
    "calls a function created earlier that drops tables",
    "PostgREST DELETE with a filter that matches every row",
    "DynamoDB DeleteItem (one row)",
    "harmless-sounding tool that wipes data on another server",
}


def req_of(c):
    r = dict(c["req"])
    r["body"] = base64.b64decode(r.pop("body_b64", ""))
    return r


def check(host, method, target, body=b"", headers=None, facts=None):
    if isinstance(body, str):
        body = body.encode()
    return D.check({"host": host, "method": method, "target": target,
                    "headers": [["host", host]] + (headers or []), "body": body, "facts": facts or {}})


def test_corpus_matches_labels():
    wrong = []
    for c in corpus_build.main() + capture_labels.cases():
        got = D.check(req_of(c))["decision"]
        if got != c["label"] and c["note"] not in KNOWN_WRONG:
            wrong.append((c["id"], c["note"], c["label"], got))
    assert not wrong, wrong


def test_known_wrong_are_still_wrong():
    # when one of these starts passing, move it out of KNOWN_WRONG and update the write-up
    for c in corpus_build.main():
        if c["note"] in KNOWN_WRONG:
            assert D.check(req_of(c))["decision"] != c["label"], c["note"]


def test_corpus_size():
    cs = corpus_build.main()
    assert sum(c["kind"] == "plain" for c in cs) >= 150
    assert sum(c["kind"] == "evasion" for c in cs) >= 30


def test_pocketos_sentence():
    r = check("backboard.railway.app", "POST", "/graphql/v2",
              json.dumps({"query": 'mutation { volumeDelete(volumeId: "5f2c9b1e") }'}),
              [["content-type", "application/json"]])
    assert r["decision"] == "hold"
    assert r["says"] == ["deletes the Railway volume 5f2c9b1e, with the backups stored on it"]
    assert r["matches"][0]["facts"]["volumeId"] == "5f2c9b1e"
    assert r["facts"]["graphql"] == ["mutation volumeDelete"]


def test_patch_commit_names_what_it_deletes():
    r = check("backboard.railway.com", "POST", "/graphql/v2",
              json.dumps({"query": "mutation ($p: EnvironmentConfig!) { environmentPatchCommit(environmentId: \"e1\", patch: $p) }",
                          "variables": {"p": {"volumes": {"vol_9": {"isDeleted": True}}}}}),
              [["content-type", "application/json"]])
    assert r["says"] == ["applies a patch to the Railway environment e1 that deletes volumes.vol_9"]


def test_facts_never_hold_credentials():
    for c in corpus_build.main():
        r = D.check(req_of(c))
        blob = json.dumps({"says": r["says"], "matches": r["matches"], "facts": r["facts"]})
        assert "PASSWORD_PLACEHOLDER" not in blob, c["id"]
        assert "sk_live_x" not in blob and "sk_live_y" not in blob, c["id"]
        assert "AKIDEXAMPLE" not in blob, c["id"]


def test_neon_sentence_names_database_without_password():
    r = check("api.us-east-2.aws.neon.tech", "POST", "/sql", json.dumps({"query": "DROP TABLE users", "params": []}),
              [["content-type", "application/json"],
               ["neon-connection-string", "postgresql://app:PASSWORD_PLACEHOLDER@ep-cool-1.us-east-2.aws.neon.tech/neondb"]])
    assert r["says"] == ["runs DROP TABLE on users in the Neon database ep-cool-1.us-east-2.aws.neon.tech/neondb"]


def test_stripe_key_mode():
    body, h = "charge=ch_1", [["content-type", "application/x-www-form-urlencoded"]]
    assert check("api.stripe.com", "POST", "/v1/refunds", body, h, {"secret": {"name": "s", "prefix": "sk_live_"}})["decision"] == "hold"
    assert check("api.stripe.com", "POST", "/v1/refunds", body, h, {"secret": {"name": "s", "prefix": "sk_test_"}})["decision"] == "allow"
    assert check("api.stripe.com", "POST", "/v1/refunds", body, h)["decision"] == "hold"   # cannot tell: hold


def test_uncovered_host_is_not_covered():
    r = check("example.org", "DELETE", "/anything")
    assert r["verdict"] == "not covered" and r["decision"] == "allow"


def test_sql_classifier():
    cases = {
        "DROP TABLE x": "destructive", "truncate x": "destructive", "DELETE FROM t": "destructive",
        "DELETE FROM t WHERE 1=1": "destructive", "DELETE FROM t WHERE id = 1 OR 1 = 1": "destructive",
        "UPDATE t SET a = 1": "destructive", "ALTER TABLE t DROP COLUMN c": "destructive",
        "WITH d AS (DELETE FROM t RETURNING 1) SELECT 1": "destructive",
        "DELETE FROM t WHERE id = 1": "harmless", "SELECT 'DROP TABLE x'": "harmless",
        "-- DROP TABLE x\nSELECT 1": "harmless", "CREATE TABLE t (id int)": "harmless",
        "DO $$ BEGIN END $$": "unknown", "SELECT dblink_exec('x')": "unknown", "NOT SQL AT ALL (": "unknown",
        "CREATE FUNCTION f() RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql": "unknown",
        "EXPLAIN ANALYZE DELETE FROM t": "destructive", "EXPLAIN SELECT 1": "harmless",
    }
    for sql, want in cases.items():
        assert sqlclass.classify(sql)["verdict"] == want, sql


def test_timing_budget():
    import measure
    o = measure.run(reps=5)
    assert o["ms_median"] < 1.0
