"""Test 36's outcome from its summary.env: "key|sentence".

    36-decide.py SUMMARY_ENV
    36-decide.py --selftest

Checks through gvproxy (g_*) are held to what a connection must carry: every body byte for byte,
the first event under a second, every silent reply whole, every line echoed in order. Apple's NAT
(n_*, on a Mac) is the reference: a check that fails on both is the network's, not gvproxy's.
After gvproxy's restart, the computer's own probe says how long its requests failed: direct_back
(the computer's link straight on gvproxy) and relay_back (through the relay).
"""
import sys


def load(path):
    d = {}
    for line in open(path):
        k, _, v = line.rstrip("\n").partition("=")
        if k:
            d[k] = v
    return d


def shortfalls(d, p):
    """What the checks with prefix p carried less than whole, as phrases; None if they did not run."""
    if (p + "_bytes") not in d:
        return None
    out = []
    b = d.get(p + "_bytes", "")
    try:
        same, n = (int(x) for x in b.split("/"))
        if same != n:
            out.append("%d of %d bodies came back changed or not at all" % (n - same, n))
    except ValueError:
        out.append("the bytes check did not run")
    f = d.get(p + "_first_ms", "")
    try:
        f = float(f)
        if f < 0:
            out.append("a streamed reply did not arrive whole")
        elif f > 1000:
            out.append("the first event of a streamed reply was held back %.0f ms" % f)
    except ValueError:
        out.append("the streamed-reply check did not run")
    for k in sorted(k for k in d if k.startswith(p + "_slow_")):
        if d[k] != "1":
            out.append("the reply silent for %s s was cut" % k.rsplit("_", 1)[1])
    x = d.get(p + "_duplex", "")
    try:
        echoed, sent, ooo = (int(v) for v in x.split("/"))
        if echoed != sent or ooo:
            out.append("%d of %d lines came back on the two-way connection, %d out of order" % (echoed, sent, ooo))
    except ValueError:
        out.append("the two-way check did not run")
    return out


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def decide(d):
    if "g_fail" in d:
        return "inconclusive", d["g_fail"]
    g = shortfalls(d, "g")
    if g is None:
        return "inconclusive", "the checks through gvproxy did not run"
    n = shortfalls(d, "n")
    if n is not None:
        g_only = [x for x in g if x not in n]
        if n:
            note = "; Apple's NAT also gave less (%s), so that much is the network's" % "; ".join(n)
        else:
            note = ""
    else:
        g_only, note = g, ""
    where = "" if d.get("platform") == "mac" else "Linux analogue: "
    if g_only:
        return "alters", "%sthrough gvproxy %s%s" % (where, "; ".join(g_only), note)
    direct, relay = num(d.get("direct_back")), num(d.get("relay_back"))
    wait = d.get("back_wait", "60")
    if direct is not None and direct <= 30:
        return "transparent", ("%severy check held through gvproxy%s, and after gvproxy restarted the computer's requests "
                               "worked again after %.1f s with nothing done to it" % (where, " as through Apple's NAT" if n is not None else "", direct))
    if direct is None and "direct_back" not in d:
        return "inconclusive", "the restart of gvproxy under the computer did not run"
    if relay is not None and relay <= 30:
        return "restart-needs-helper", ("%severy check held through gvproxy, but after gvproxy restarted the computer's network "
                                        "stayed dead for %s s (%s); through the relay that connects again it came back after %.1f s"
                                        % (where, wait, "never back" if direct is None else "back only after %.0f s" % direct, relay))
    if "relay_back" not in d:
        return "inconclusive", "%sthe computer's network did not come back after gvproxy restarted, and the run through the relay did not happen" % where
    return "restart-breaks", ("%severy check held through gvproxy, but after gvproxy restarted the computer's network did not "
                              "come back within %s s, directly or through the relay" % (where, wait))


FIXTURES = [
    ({"platform": "mac", "g_bytes": "100/100", "g_first_ms": "3.1", "g_slow_60": "1", "g_slow_330": "1", "g_duplex": "600/600/0",
      "n_bytes": "100/100", "n_first_ms": "2.0", "n_slow_60": "1", "n_slow_330": "1", "n_duplex": "600/600/0",
      "direct_back": "4.2", "relay_back": "3.0"}, "transparent"),
    ({"platform": "mac", "g_bytes": "100/100", "g_first_ms": "3.1", "g_slow_60": "1", "g_slow_330": "1", "g_duplex": "600/600/0",
      "direct_back": "never", "relay_back": "2.5"}, "restart-needs-helper"),
    ({"platform": "mac", "g_bytes": "100/100", "g_first_ms": "3.1", "g_slow_60": "1", "g_slow_330": "1", "g_duplex": "600/600/0",
      "direct_back": "never", "relay_back": "never"}, "restart-breaks"),
    ({"platform": "mac", "g_bytes": "100/100", "g_first_ms": "3.1", "g_slow_60": "1", "g_slow_330": "0", "g_duplex": "600/600/0",
      "n_bytes": "100/100", "n_first_ms": "2.0", "n_slow_60": "1", "n_slow_330": "1", "n_duplex": "600/600/0",
      "direct_back": "2"}, "alters"),
    # The same cut on Apple's NAT: the network's, not gvproxy's.
    ({"platform": "mac", "g_bytes": "100/100", "g_first_ms": "3.1", "g_slow_60": "1", "g_slow_330": "0", "g_duplex": "600/600/0",
      "n_bytes": "100/100", "n_first_ms": "2.0", "n_slow_60": "1", "n_slow_330": "0", "n_duplex": "600/600/0",
      "direct_back": "2"}, "transparent"),
    ({"platform": "linux", "g_bytes": "99/100", "g_first_ms": "3", "g_slow_5": "1", "g_duplex": "10/10/0"}, "alters"),
    ({"platform": "linux", "g_bytes": "100/100", "g_first_ms": "2500", "g_slow_5": "1", "g_duplex": "10/10/0"}, "alters"),
    ({"platform": "mac", "g_fail": "the computer did not answer SSH through gvproxy"}, "inconclusive"),
    ({"platform": "mac", "g_bytes": "100/100", "g_first_ms": "3.1", "g_slow_60": "1", "g_duplex": "600/600/0",
      "direct_back": "never"}, "inconclusive"),
]


def main():
    if sys.argv[1:] == ["--selftest"]:
        bad = 0
        for d, want in FIXTURES:
            got = decide(d)
            if got[0] != want:
                print("FAIL: %s gave %s (%s)" % (d, got[0], got[1]))
                bad += 1
        print("36-decide: %d fixtures, %d wrong" % (len(FIXTURES), bad))
        sys.exit(1 if bad else 0)
    k, s = decide(load(sys.argv[1]))
    print("%s|%s" % (k, s))


if __name__ == "__main__":
    main()
