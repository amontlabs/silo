"""Test 32's outcome from its runs: prints "key|sentence".

  32-decide.py CSV
      CSV has one row per run (file, ckpt, py, py-tcp: time to SSH and the guest's flushed-write
      latencies) and one row "checkpoint" (held, the moment's pause, the guest's longest write).
"""
import csv, sys
rows = {r["run"]: r for r in csv.DictReader(open(sys.argv[1]))}
f = lambda r, k: float(rows[r][k]) if r in rows and rows[r].get(k) not in (None, "") else None
fs, fw = f("file", "ssh_s"), f("file", "p50_full_ms")
booted = [r for r in ("ckpt", "py", "py-tcp") if f(r, "ssh_s") is not None]
held = rows.get("checkpoint", {}).get("held", "")
notes = "; ".join("%s: %s" % (r, rows[r]["note"]) for r in rows if rows[r].get("note"))
def desc(r):
    t = "%s server: SSH after %.0f s" % (r, f(r, "ssh_s"))
    if f(r, "p50_full_ms") is not None: t += ", flushed 4 KiB write p50 %.2f ms" % f(r, "p50_full_ms")
    if f(r, "p50_fsync_ms") is not None: t += " (fsync only %.2f ms)" % f(r, "p50_fsync_ms")
    return t
if fs is None or fw is None:
    print("inconclusive|the computer did not start from its own file, or its writes were not measured; " + notes); sys.exit()
base = "file: SSH after %.0f s, flushed 4 KiB write p50 %.2f ms" % (fs, fw)
if not booted:
    print("no-boot|%s; no NBD start reached SSH; %s" % (base, notes)); sys.exit()
p = booted[0]
s = "; ".join([base] + [desc(r) for r in booted])
ck = rows.get("checkpoint", {})
if ck:
    s += "; checkpoint through the py server: %s" % (held or "not run")
    if ck.get("pause_ms"): s += ", writes held %s ms for its moment" % ck["pause_ms"]
    if ck.get("guest_max_wait_ms"): s += ", the guest's longest flushed write while checkpoints were taken %s ms" % ck["guest_max_wait_ms"]
if notes: s += "; " + notes
if p != "ckpt": s += " (the prototype's own server did not run: the costs are the Python server's, an upper bound)"
if held not in ("held", ""):
    print("checkpoint-broke|" + s); sys.exit()
if held == "":
    print("inconclusive|no checkpoint was taken through the server; " + s); sys.exit()
ps, pw = f(p, "ssh_s"), f(p, "p50_full_ms")
if pw is None:
    print("inconclusive|" + s); sys.exit()
print(("boots-cheap|" if ps <= 1.5 * fs and pw <= 2 * fw else "boots-slow|") + s)
