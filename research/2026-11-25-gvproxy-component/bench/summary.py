"""Median of each measure per build and MTU, from bench.sh's lines. usage: summary.py <results>..."""
import sys, statistics, collections
rows = collections.OrderedDict()
for f in sys.argv[1:]:
    for l in open(f):
        p = l.split()
        if len(p) < 3 or "=" not in p[1]: continue
        kv = dict(x.split("=") for x in p[1:])
        rows.setdefault((p[0], kv["mtu"]), []).append(kv)
keys = ["up_gbps", "down_gbps", "rtt_p50_us", "rtt_p99_us", "conns_per_s", "gvproxy_rss_kb"]
print("| build | MTU | computer to internet, Gbit/s | internet to computer, Gbit/s | round trip p50 / p99, µs | new connections per s | gvproxy memory, MB | runs |")
print("|---|---|---|---|---|---|---|---|")
for (label, mtu), runs in rows.items():
    m = {k: statistics.median(float(r[k]) for r in runs) for k in keys}
    print(f"| {label} | {mtu} | {m['up_gbps']:.2f} | {m['down_gbps']:.2f} | {m['rtt_p50_us']:.0f} / {m['rtt_p99_us']:.0f} | {m['conns_per_s']:.0f} | {m['gvproxy_rss_kb']/1024:.0f} | {len(runs)} |")
