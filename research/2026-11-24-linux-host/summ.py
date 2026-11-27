"""summ.py <latencies.json> <windows.txt> <label>: the neighbour's writes per second, slowest
write and 99th percentile, inside the copy windows and outside them (0.5 s margin)."""
import json, sys
lat = json.load(open(sys.argv[1])); wins = [tuple(map(float, l.split())) for l in open(sys.argv[2])]
t0, t1 = lat[0][0], lat[-1][0]
inside = lambda t, m: any(a - m <= t <= b + m for a, b in wins)
tin = sum(b - a for a, b in wins); tout = (t1 - t0) - sum(b - a + 1.0 for a, b in wins)
def stats(xs, secs):
    xs = sorted(xs); return f"ops/s={len(xs)/secs:.0f} max={xs[-1]*1000:.0f}ms p99={xs[int(len(xs)*.99)]*1000:.1f}ms"
a = [d for t, d in lat if inside(t, 0)]; b = [d for t, d in lat if not inside(t, 0.5)]
print(f"{sys.argv[3]}: copies {' '.join(f'{y-x:.2f}s' for x,y in wins)} | B during copies: {stats(a, tin)} | B otherwise: {stats(b, tout)}")
