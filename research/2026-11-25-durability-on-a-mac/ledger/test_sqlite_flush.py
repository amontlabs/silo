"""The team ledger (#388, research/2026-11-25-team-ledger/ledger/server.py) commits each grant to
SQLite with synchronous=FULL before it answers. On a Mac that is a plain fsync(2), which leaves
the commit in the drive's cache: a power cut can lose a grant a Mac already spends from, and the
ledger then grants the same dollars twice. SQLite asks for F_FULLFSYNC only with
`PRAGMA fullfsync` (each commit) and `PRAGMA checkpoint_fullfsync` (each WAL checkpoint), both
off by default, on macOS too. On Linux both are read back as set and change nothing.

usage: python3 -I test_sqlite_flush.py <path to ledger/server.py>
"""
import importlib.util, os, sys, tempfile

spec = importlib.util.spec_from_file_location("server", sys.argv[1])
server = importlib.util.module_from_spec(spec); spec.loader.exec_module(server)
with tempfile.TemporaryDirectory() as d:
    led = server.Ledger(os.path.join(d, "ledger.db"), "leases")
    got = {p: led.db.execute(f"PRAGMA {p}").fetchone()[0] for p in ("synchronous", "fullfsync", "checkpoint_fullfsync")}
    want = {"synchronous": 2, "fullfsync": 1, "checkpoint_fullfsync": 1}
    print(f"sqlite {server.sqlite3.sqlite_version}: {got}")
    bad = {k: v for k, v in got.items() if v != want[k]}
    print("FAIL: " + ", ".join(f"{k} is {v}, needs {want[k]}" for k, v in bad.items()) if bad else "PASS: every commit and checkpoint asks for F_FULLFSYNC on a Mac")
    sys.exit(1 if bad else 0)
