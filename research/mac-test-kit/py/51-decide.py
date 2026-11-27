"""Test 51's outcome from its cuts (51-cuts.csv): M, F and O lines, tab-separated."""
import csv, sys

rows = list(csv.DictReader(open(sys.argv[1])))


def out(*f):
    print("\t".join(str(x) for x in f))


def held(r):
    try:
        return r["started"] == "yes" and r["verify_ok"] == "yes" and int(r["lines"] or 0) >= int(r["last_acked"] or 0)
    except ValueError:
        return False


by = {}
for r in rows:
    by.setdefault(r["mode"], []).append(r)
    out("F", "cut_%s_%s" % (r["mode"], r["cut"]), "acknowledged %s, present %s, started %s, verify %s, cache %s kept %s, flushes %s%s"
        % (r["last_acked"], r["lines"], r["started"], r["verify_ok"], r["cached"], r["kept"], r["flushes"], (", " + r["note"]) if r["note"] else ""))
    if r["flushes"] and r["acks"] and int(r["acks"]) > 0:
        out("M", "%s_flushes_per_ack_%s" % (r["mode"], r["cut"]), round(int(r["flushes"]) / int(r["acks"]), 2), "", "flushes the drive received per acknowledged line")
if any(r["started"] == "no-first" for r in rows):
    out("O", "inconclusive", "the computer did not start from the stand-in drive")
    sys.exit()
if any(r["started"] == "no-boot" for r in rows):
    r = next(r for r in rows if r["started"] == "no-boot")
    out("O", "no-start", "after a cut with %s, the computer did not start" % r["mode"])
    sys.exit()
ok = {m: all(held(r) for r in rs) for m, rs in by.items()}
s = "; ".join("%s: %s" % (m, "every acknowledged line kept" if ok[m] else "lost or did not start in %d of %d cuts" % (sum(not held(r) for r in rs), len(rs)))
              for m, rs in by.items())
if not by:
    out("O", "inconclusive", "no cut ran")
elif "full" in ok and not ok["full"]:
    out("O", "full-loses", s)
elif "barrier" in ok and not ok["barrier"]:
    out("O", "barrier-loses", s)
elif ok.get("fsync", False):
    out("O", "fsync-holds", s)
else:
    out("O", "full-holds-fsync-loses", s)
