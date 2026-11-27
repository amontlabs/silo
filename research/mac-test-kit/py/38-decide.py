"""Test 38's outcome from its summary.env: "key|sentence".

    38-decide.py SUMMARY_ENV
    38-decide.py --selftest
"""
import sys


def load(path):
    d = {}
    for line in open(path):
        k, _, v = line.rstrip("\n").partition("=")
        if k:
            d[k] = v
    return d


def n(d, k):
    try:
        return int(d.get(k, ""))
    except ValueError:
        return None


def decide(d):
    if d.get("running_full") == "no" or d.get("running_end") == "no":
        return "framework-stops", "the computer's run ended while the volume was full (see the events since the fill)"
    if d.get("ssh_full") == "no" and d.get("ssh_end") == "no":
        return "framework-stops", "the computer still ran but stopped answering SSH on the full volume and did not come back after room was made"
    if d.get("ssh_end") != "yes" or n(d, "probe_after") is None:
        return "inconclusive", "the computer could not be read after room was made"
    fails, stall = n(d, "probe_fail_full") or 0, n(d, "probe_stall_max") or 0
    after, ok_after = n(d, "probe_after"), n(d, "probe_ok_after")
    seen = ("while the volume was full, %d of the computer's writes failed (errno %s) and its longest wait between two was %d s; "
            "after room was made, %d of %d worked; its volume: %s" % (fails, d.get("probe_errors") or "none", stall, ok_after, after,
                                                                      d.get("verify", "not checked")))
    if d.get("verify") == "damaged":
        return "guest-damaged", seen
    if after == 0 or ok_after * 2 < after:
        return "framework-stops", "the computer ran, but its writes did not work again after room was made: " + seen
    if fails:
        return "io-error-recovers", seen
    if stall >= 20:
        return "writes-wait", seen
    return "inconclusive", "the computer's writes neither failed nor waited while the volume was full: " + seen


BASE = {"filled": "yes", "running_full": "yes", "ssh_full": "yes", "running_end": "yes", "ssh_end": "yes",
        "probe_fail_full": "40", "probe_errors": "5", "probe_stall_max": "2", "probe_ok_after": "50", "probe_after": "52", "verify": "ok"}
FIXTURES = [
    ({}, "io-error-recovers"),
    ({"probe_fail_full": "0", "probe_stall_max": "75"}, "writes-wait"),
    ({"running_full": "no"}, "framework-stops"),
    ({"verify": "damaged"}, "guest-damaged"),
    ({"probe_ok_after": "0"}, "framework-stops"),
    ({"ssh_full": "no", "ssh_end": "no"}, "framework-stops"),
    ({"probe_fail_full": "0", "probe_stall_max": "1"}, "inconclusive"),
    ({"ssh_end": "", "probe_after": ""}, "inconclusive"),
]


def main():
    if sys.argv[1:] == ["--selftest"]:
        bad = 0
        for change, want in FIXTURES:
            got = decide(dict(BASE, **change))
            if got[0] != want:
                print("FAIL: %s gave %s (%s)" % (change, got[0], got[1]))
                bad += 1
        print("38-decide: %d fixtures, %d wrong" % (len(FIXTURES), bad))
        sys.exit(1 if bad else 0)
    k, s = decide(load(sys.argv[1]))
    print("%s|%s" % (k, s))


if __name__ == "__main__":
    main()
