"""The rules of asks.py that must never bend.   python3 -m pytest -q test_asks.py"""
import json
import os
import tempfile
import threading
import time

import asks


def rec(i, inv=None, ident=None, host="s3"):
    return {"id": i, "op": "s3:DeleteObject", "path": f"/b/k{i}", "invocation_id": inv,
            "identity": ident or f"id{i}", "host": host, "conn": 1, "ua": "aws-cli/1.0"}


def run(cfg, arrivals):
    """arrivals: [(delay, rec)]. Returns (results by id, asks log)."""
    log = tempfile.mktemp()
    t0 = time.monotonic()
    eng = asks.Engine(cfg, log, lambda: time.monotonic() - t0)
    out = {}

    def go(r):
        out[r["id"]] = eng.hold(r)

    ths = []
    for delay, r in arrivals:
        time.sleep(delay)
        th = threading.Thread(target=go, args=(r,))
        th.start()
        ths.append(th)
    for th in ths:
        th.join()
    lines = [json.loads(l) for l in open(log)]
    os.unlink(log)
    return out, lines


BASE = {"hold_ops": ".*", "group": "computer", "window": 0.3, "answer_after": 0.6, "hold_limit": 10, "retry_join": True}


def test_yes_covers_only_what_was_on_screen():
    asks.SEEN_LAG = 0.3
    # k1 opens the ask; k2 joins early; k3 joins 0.1 s before the tap: not seen long enough
    out, log = run(BASE, [(0, rec(1)), (0.1, rec(2)), (0.7, rec(3))])
    assert [l["id"] for l in log[0]["lines"]] == [1, 2]
    assert log[0]["moved_to_new_ask"] == [3]
    assert [l["id"] for l in log[1]["lines"]] == [3]
    assert all(v[0] == "yes" for v in out.values())


def test_retry_joins_its_line_and_passes_once():
    asks.SEEN_LAG = 0.3
    out, log = run(BASE, [(0, rec(1, inv="A")), (0.2, rec(2, inv="A"))])
    assert len(log) == 1 and len(log[0]["lines"]) == 1 and log[0]["lines"][0]["copies"] == 2
    assert sorted(v[0] for v in out.values()) == ["replaced", "yes"]
    assert out[2][0] == "yes"          # the newest copy is the one released


def test_one_per_request_is_serial():
    asks.SEEN_LAG = 0.1
    cfg = dict(BASE, group="request", window=0, answer_after=0.3)
    out, log = run(cfg, [(0, rec(1)), (0, rec(2)), (0, rec(3))])
    assert len(log) == 3
    assert log[2]["answered"] - log[0]["answered"] >= 0.55


def test_no_refuses_every_copy():
    asks.SEEN_LAG = 0.3
    out, log = run(dict(BASE, answer="no"), [(0, rec(1, inv="A")), (0.1, rec(2, inv="A"))])
    assert {v[0] for v in out.values()} == {"no"}
