"""How far main's clock falls behind across a sleep, and whether it comes back.

For each sleep length: the offset of main's wall clock from the host's before
the sleep, then 1 s, 10 s and 60 s after the wake; and the offset of main's
hardware clock (RTC), with QEMU's RTC on the host's clock and on the guest's.
Writes results.json next to this script's folder and prints a table.
"""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(__file__))
from vm import Guest

SLEEPS = [int(s) for s in os.environ.get("SLEEPS", "10 30 120").split()]
rows = []
for rtc in ("host", "vm"):
    g = Guest(rtc_clock=rtc)
    g.set_clock()
    for s in SLEEPS:
        g.set_clock()
        before, _ = g.offset()
        t = time.time()
        g.sleep(s)
        woke = time.time()
        after = {}
        for at in (1, 10, 60):
            time.sleep(max(0, woke + at - time.time()))
            after[at] = round(g.offset()[0], 3)
        row = {"rtc": rtc, "sleep_s": round(woke - t, 2), "before_s": round(before, 3),
               "after_s": after, "rtc_after_s": round(g.rtc_offset(), 1)}
        rows.append(row)
        print(json.dumps(row), flush=True)
    # a minute awake with no sleep: the guest's own drift under TCG
    g.set_clock(); time.sleep(60)
    rows.append({"rtc": rtc, "awake_60s_drift_s": round(g.offset()[0], 3)})
    print(json.dumps(rows[-1]), flush=True)
    g.close()
json.dump(rows, open(os.path.join(os.path.dirname(__file__), "..", "results", "measure.json"), "w"), indent=1)
