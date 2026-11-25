"""Samples the run's processes once a second: resident memory, its peak, threads, CPU seconds, and
the record's size on disk. usage: sample.py <out> <seconds>; prints JSON lines."""
import json, os, sys, time

OUT, SECS = sys.argv[1], float(sys.argv[2])
def pid(n):
    try: return int(open(f"{OUT}/{n}.pid").read())
    except Exception: return None
def proc(p):
    try:
        st = dict(l.split(":", 1) for l in open(f"/proc/{p}/status").read().splitlines() if ":" in l)
        f = open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()
        return {"rss_mb": int(st["VmRSS"].split()[0]) / 1024, "hwm_mb": int(st["VmHWM"].split()[0]) / 1024,
                "threads": int(st["Threads"]), "cpu_s": (int(f[11]) + int(f[12])) / os.sysconf("SC_CLK_TCK"),
                "fds": len(os.listdir(f"/proc/{p}/fd"))}
    except Exception: return None
def du(d):
    n = 0
    for root, _, fs in os.walk(d):
        for f in fs:
            try: n += os.path.getsize(os.path.join(root, f))
            except OSError: pass
    return n
t_end = time.time() + SECS
while time.time() < t_end:
    o = {"t": time.time()}
    for n in ("boundary", "recorder", "precaution"):
        p = pid(n)
        if p: o[n] = proc(p)
    o["record_bytes"] = du(f"{OUT}/record")
    print(json.dumps(o), flush=True)
    time.sleep(1)
