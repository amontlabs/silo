"""Test 37's outcome from its summary.env: "key|sentence".

    37-decide.py SUMMARY_ENV
    37-decide.py --selftest
"""
import sys


def load(path):
    d = {}
    for line in open(path):
        k, _, v = line.rstrip("\n").partition("=")
        if k:
            d[k] = v
    return d


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def decide(d):
    where = "Linux analogue: " if d.get("platform") == "linux" else ""
    awake = d.get("vm_holds_awake", "unknown")
    slept = num(d.get("slept_s"))
    if num(d.get("offset_before")) is None:
        return "inconclusive", "the computer did not answer before the sleep"
    if slept is None:
        return "inconclusive", "the Mac's clocks were not read across the sleep"
    if slept < 30:
        return "no-sleep", ("%sthe Mac did not sleep (the clocks saw %.0f s of sleep); the computer's process held the Mac awake: %s"
                            % (where, slept, awake))
    if d.get("vm_running") != "yes":
        return "run-lost", "the computer stopped across a sleep of %.0f s" % slept
    if d.get("ssh") != "yes":
        return "run-lost", "the computer still ran after a sleep of %.0f s but did not answer SSH for 2 minutes" % slept
    if d.get("run_alive") != "yes":
        return "run-lost", "the run left detached inside ended across a sleep of %.0f s" % slept
    o60 = num(d.get("offset_60"))
    if o60 is None:
        return "inconclusive", "the computer's clock was not read a minute after the wake"
    gap = d.get("guest_gap_s", "?")
    facts = ("a sleep of %.0f s by the Mac's clocks, %s s by the computer's; its clock %s s behind at the wake and %.1f s a minute later; "
             "the computer's process held the Mac awake: %s" % (slept, gap, d.get("offset_wake", "?"), o60, awake))
    if d.get("net") != "ok":
        return "network-lost", "the computer came back but its network did not: " + facts
    if abs(o60) > 5:
        return "clock-behind", facts
    return "wakes-whole", facts


BASE = {"platform": "mac", "vm_holds_awake": "no", "offset_before": "0.1", "slept_s": "185.2", "vm_running": "yes", "ssh": "yes",
        "offset_wake": "184.0", "run_alive": "yes", "guest_gap_s": "186.1", "net": "ok", "offset_60": "0.4"}
FIXTURES = [
    ({}, "wakes-whole"),
    ({"offset_60": "120.5"}, "clock-behind"),
    ({"net": "fail"}, "network-lost"),
    ({"run_alive": "no"}, "run-lost"),
    ({"vm_running": "no"}, "run-lost"),
    ({"ssh": "no"}, "run-lost"),
    ({"slept_s": "0.0", "vm_holds_awake": "yes"}, "no-sleep"),
    ({"offset_before": ""}, "inconclusive"),
    ({"offset_60": ""}, "inconclusive"),
]


def main():
    if sys.argv[1:] == ["--selftest"]:
        bad = 0
        for change, want in FIXTURES:
            d = dict(BASE, **change)
            got = decide(d)
            if got[0] != want:
                print("FAIL: %s gave %s (%s)" % (change, got[0], got[1]))
                bad += 1
        print("37-decide: %d fixtures, %d wrong" % (len(FIXTURES), bad))
        sys.exit(1 if bad else 0)
    k, s = decide(load(sys.argv[1]))
    print("%s|%s" % (k, s))


if __name__ == "__main__":
    main()
