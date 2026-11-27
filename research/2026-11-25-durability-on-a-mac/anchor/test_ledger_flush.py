"""The anchor ledger of #394 (research/2026-11-25-record-head-off-the-mac/anchor/anchor.py) keeps
each head it accepts with os.fsync before it answers 201. Python's os.fsync is a plain fsync(2)
on macOS too, which leaves the head in the drive's cache; on a Mac that stays on as the team's host
(#388), a power cut can lose a head the computer was told was kept. The flush that reaches the
disk is fcntl(F_FULLFSYNC).

The test plays a Mac on Linux: it gives the fcntl module an F_FULLFSYNC and records which flush
each accepted head got. It passes when every head got F_FULLFSYNC.

usage: python3 -I test_ledger_flush.py <path to anchor/>
"""
import fcntl, importlib, json, os, sys, tempfile, threading

sys.path.insert(0, os.path.abspath(sys.argv[1]))
anchor = importlib.import_module("anchor")

calls = []
real_fcntl, real_fsync = fcntl.fcntl, os.fsync
fcntl.F_FULLFSYNC = 51
def fake_fcntl(fd, cmd, *a):
    if cmd == 51:
        calls.append("F_FULLFSYNC"); return real_fsync(fd) or 0
    return real_fcntl(fd, cmd, *a)
def fake_fsync(fd):
    calls.append("fsync"); return real_fsync(fd)
fcntl.fcntl, os.fsync = fake_fcntl, fake_fsync

with tempfile.TemporaryDirectory() as d:
    key = anchor.new_key()
    led = anchor.Ledger(os.path.join(d, "heads.ledger"), {"c1": key.public_key()})
    threading.Thread(target=led.serve_forever, daemon=True).start()
    la = anchor.LedgerAnchor(f"http://127.0.0.1:{led.server_address[1]}")
    codes = []
    for seq in (10, 20, 30):
        codes.append(la.push(anchor.sign(key, {"v": 1, "computer": "c1", "seq": seq, "hash": "%064x" % seq, "time": seq}))[0])
    led.shutdown()
ok = codes == [201, 201, 201] and calls and all(c == "F_FULLFSYNC" for c in calls)
print(json.dumps({"answers": codes, "flushes": calls}))
print("PASS: each head kept with F_FULLFSYNC" if ok else "FAIL: a head was answered 201 after a plain fsync, which a Mac's drive may not have written")
sys.exit(0 if ok else 1)
