"""The test: after the Mac sleeps 20 s with main running, main's clock is within
1 s of the Mac's 2 s after the wake, and stays so a minute on.

  test_clock.py none          no fix: fails (main is ~20 s behind)
  test_clock.py keeper        the helper's keeper, on wake and every 60 s: passes
  test_clock.py periodic      the keeper with no wake signal, every 60 s: fails at
                              2 s, right by the minute (shows why the wake signal matters)
Exit 0 on pass, 1 on fail.
"""
import os, sys, time
sys.path.insert(0, os.path.dirname(__file__))
from vm import Guest
from keeper import ClockKeeper

mode = sys.argv[1] if len(sys.argv) > 1 else "keeper"
g = Guest()
g.set_clock()  # start right, as the helper does at start
k = ClockKeeper(g, period=60) if mode in ("keeper", "periodic") else None
ok = True
try:
    g.sleep(20)
    woke = time.time()
    if mode == "keeper":
        k.woke()
    time.sleep(max(0, woke + 2 - time.time()))
    off2, _ = g.offset()
    time.sleep(max(0, woke + 65 - time.time()))
    off65, _ = g.offset()
    for at, off in ((2, off2), (65, off65)):
        good = abs(off) < 1
        ok &= good
        print(f"{mode}: {at:>2} s after the wake main is {off:+.3f} s off the Mac: {'pass' if good else 'FAIL'}")
    if k:
        print(f"{mode}: steps {[(w, round(a, 3), round(b, 3)) for w, a, b in k.log]}")
finally:
    if k:
        k.close()
    g.close()
sys.exit(0 if ok else 1)
